use std::sync::Arc;
use aws_lambda_events::event::eventbridge::EventBridgeEvent;

use lambda_runtime::{tracing, Error, LambdaEvent};
use serde::Deserialize;
use pbtb_rust::usecase::{ReconcileOutcome, RecordRunningOutcome, StopInfo};
use crate::AppState;

#[derive(Debug, Deserialize)]
struct EcsTaskStateChangeDetail {
    #[serde(rename = "clusterArn")]
    cluster_arn: Option<String>,
    #[serde(rename = "taskArn")]
    task_arn: Option<String>,
    #[serde(rename = "lastStatus")]
    last_status: Option<String>,

    #[serde(rename = "stopCode")]
    stop_code: Option<String>,
    #[serde(rename = "stoppedReason")]
    stopped_reason: Option<String>,

    containers: Option<Vec<EcsContainer>>,

    overrides: Option<EcsOverrides>,
}

#[derive(Debug, Deserialize)]
struct EcsOverrides {
    #[serde(rename = "containerOverrides")]
    container_overrides: Option<Vec<EcsContainerOverride>>,
}

#[derive(Debug, Deserialize)]
struct EcsContainerOverride {
    environment: Option<Vec<EcsEnvVar>>,
}

#[derive(Debug, Deserialize)]
struct EcsEnvVar {
    name: Option<String>,
    value: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EcsContainer {
    name: Option<String>,
    #[serde(rename = "exitCode")]
    exit_code: Option<i32>,
}

/// Extract (user_id, bot_id) from the first container override's environment.
/// These are the overrides `RunTaskUseCase` injects, present for the whole task
/// lifecycle, so they are available on both RUNNING and STOPPED events.
fn extract_user_bot(detail: &EcsTaskStateChangeDetail) -> (Option<String>, Option<String>) {
    let mut user_id: Option<String> = None;
    let mut bot_id: Option<String> = None;

    // Scan EVERY container override, not just the first: a name-only sidecar
    // override (GuardDuty agent, Service Connect, etc.) can sort ahead of the
    // env-bearing passivbot one, and `.first()` would then miss USER_ID/BOT_ID.
    if let Some(container_overrides) = detail
        .overrides
        .as_ref()
        .and_then(|o| o.container_overrides.as_ref())
    {
        for co in container_overrides {
            if let Some(envs) = &co.environment {
                for e in envs {
                    let k = e.name.as_deref().unwrap_or("");
                    let v = e.value.as_deref().unwrap_or("").to_string();
                    match k {
                        "USER_ID" | "user_id" => user_id = Some(v),
                        "BOT_ID" | "bot_id" => bot_id = Some(v),
                        _ => {}
                    }
                }
            }
        }
    }

    tracing::info!("Extracted from overrides: user_id={:?}, bot_id={:?}", user_id, bot_id);

    (user_id, bot_id)
}

fn task_id_from_arn(task_arn: &str) -> &str {
    task_arn.rsplit('/').next().unwrap_or(task_arn)
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// This is the main body for the function.
pub(crate) async fn function_handler(
    event: LambdaEvent<EventBridgeEvent>,
    state: Arc<AppState>,
) -> Result<(), Error> {
    let payload = event.payload;

    // 1) only ECS Task State Change
    let source = payload.source.as_str();
    let detail_type = payload.detail_type.as_str();
    if source != "aws.ecs" || detail_type != "ECS Task State Change" {
        tracing::info!(
            "Ignore event: source={:?}, detail-type={:?}",
            payload.source,
            payload.detail_type
        );
        return Ok(());
    }

    // Use the event time (when the transition actually happened) as the observation
    // timestamp. This lets the RUNNING path drop stale/reordered events. Fall back
    // to wall-clock if EventBridge omits it.
    let observed_at = payload.time.map(|t| t.timestamp()).unwrap_or_else(now_epoch);

    // 2) parse detail
    let detail: EcsTaskStateChangeDetail = serde_json::from_value(payload.detail.clone())
        .map_err(|e| Error::from(format!("Failed to parse ECS detail: {e}")))?;

    let last_status = detail.last_status.as_deref().unwrap_or("");
    let task_arn = detail.task_arn.as_deref().unwrap_or("<unknown-task-arn>");
    let cluster_arn = detail.cluster_arn.as_deref().unwrap_or("<unknown-cluster-arn>");

    // user_id/bot_id identify the bot for every state we act on. Skip if absent.
    let (user_id, bot_id) = extract_user_bot(&detail);
    let (Some(user_id), Some(bot_id)) = (user_id.as_deref(), bot_id.as_deref()) else {
        tracing::warn!(
            "Missing USER_ID/BOT_ID in overrides; skip. taskArn={}, lastStatus={}",
            task_arn,
            last_status
        );
        return Ok(());
    };

    match last_status {
        // Task reached RUNNING -> record observed-running (the counterpart to the
        // STOPPED path below). This is what fills the runtime row for any task,
        // including ones started outside this Lambda.
        "RUNNING" => {
            let task_id = task_id_from_arn(task_arn);
            tracing::info!(
                "ECS task running: taskArn={}, clusterArn={}, user_id={}, bot_id={}",
                task_arn,
                cluster_arn,
                user_id,
                bot_id
            );

            let outcome = state
                .record_running
                .execute(user_id, bot_id, task_id, observed_at)
                .await
                .map_err(|e| Error::from(format!("Failed to record running task: {e:#}")))?;

            match outcome {
                RecordRunningOutcome::Recorded { version } => {
                    tracing::info!("Recorded observed-running: task_id={}, version={}", task_id, version);
                }
                RecordRunningOutcome::SkippedStale => {
                    tracing::warn!("Stale RUNNING event ignored. taskArn={}", task_arn);
                }
            }
            Ok(())
        }

        // Task STOPPED -> delegate the restart-or-skip decision to the reconcile
        // use case. It checks desired state (Bot.enabled), the memory-related rule,
        // records observed runtime, and (re)starts the task when appropriate.
        "STOPPED" => {
            tracing::info!(
                "ECS task stopped: taskArn={}, clusterArn={}, stopCode={:?}, stoppedReason={:?}",
                task_arn,
                cluster_arn,
                detail.stop_code,
                detail.stopped_reason
            );

            let container = match detail.containers.as_ref().and_then(|v| v.first()) {
                Some(c) => c,
                None => {
                    tracing::info!("No containers found in task detail; skip container-based handling. taskArn={}", task_arn);
                    return Ok(());
                }
            };

            let name = container.name.as_deref().unwrap_or("<unknown-container>");
            let exit = container.exit_code.unwrap_or(-1);
            let stop_code = detail.stop_code.as_deref().unwrap_or("<unknown-stop-code>");
            tracing::info!(
                "Container exit (single-container task): name={}, exitCode={}, reason={:?}",
                name,
                exit,
                stop_code
            );

            let cfg = &state.configs;
            let stop = StopInfo { exit_code: exit, stop_code: stop_code.to_string() };

            let outcome = state
                .reconcile
                .execute(
                    user_id,
                    bot_id,
                    &cfg.ecs.cluster_arn,
                    &cfg.ecs.td_passivbot_v741_arn,
                    &cfg.ecs.td_passivbot_v741_container_name,
                    stop,
                    observed_at,
                )
                .await
                .map_err(|e| Error::from(format!("Failed to reconcile stopped task: {e:#}")))?;

            match outcome {
                ReconcileOutcome::Restarted { task_id } => {
                    tracing::info!("Started replacement task_id={}", task_id);
                }
                ReconcileOutcome::SkippedNotEnabled => {
                    tracing::warn!("Bot desired state is OFF; not restarting. taskArn={}", task_arn);
                }
                ReconcileOutcome::SkippedNotMemoryRelated => {
                    tracing::warn!("Task did not exit due to a memory-related reason; skipping restart. taskArn={}", task_arn);
                }
                ReconcileOutcome::BotNotFound => {
                    tracing::warn!("Bot not found for user_id={:?}, bot_id={:?}; skipping restart. taskArn={}", user_id, bot_id, task_arn);
                }
            }
            Ok(())
        }

        other => {
            tracing::info!("Ignore task state change: lastStatus={}", other);
            Ok(())
        }
    }
}
