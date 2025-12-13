use std::sync::Arc;
use aws_lambda_events::event::eventbridge::EventBridgeEvent;

use lambda_runtime::{tracing, Error, LambdaEvent};
use serde::Deserialize;
use serde_json::Value;
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
    name: Option<String>,
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

    // 2) parse detail
    let detail_value: &Value = &payload.detail;
    let detail: EcsTaskStateChangeDetail = serde_json::from_value(detail_value.clone())
        .map_err(|e| Error::from(format!("Failed to parse ECS detail: {e}")))?;

    // 3) only STOPPED
    let last_status = detail.last_status.as_deref().unwrap_or("");
    if last_status != "STOPPED" {
        tracing::info!("Ignore task state change: lastStatus={:?}", detail.last_status);
        return Ok(());
    }

    let task_arn = detail.task_arn.as_deref().unwrap_or("<unknown-task-arn>");
    let cluster_arn = detail.cluster_arn.as_deref().unwrap_or("<unknown-cluster-arn>");
    tracing::info!(
        "ECS task stopped: taskArn={}, clusterArn={}, stopCode={:?}, stoppedReason={:?}",
        task_arn,
        cluster_arn,
        detail.stop_code,
        detail.stopped_reason
    );

    // 3.1) get user_id/bot_id from overrides.containerOverrides[0].environment
    let mut user_id: Option<String> = None;
    let mut bot_id: Option<String> = None;

    if let Some(overrides) = &detail.overrides {
        if let Some(container_overrides) = &overrides.container_overrides {
            if let Some(first_override) = container_overrides.first() {
                if let Some(envs) = &first_override.environment {
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

                tracing::info!(
                    "Extracted from overrides: container={:?}, user_id={:?}, bot_id={:?}",
                    first_override.name,
                    user_id,
                    bot_id
                );
            } else {
                tracing::info!("No containerOverrides found in overrides; cannot extract USER_ID/BOT_ID");
            }
        } else {
            tracing::info!("overrides.containerOverrides is missing; cannot extract USER_ID/BOT_ID");
        }
    } else {
        tracing::info!("overrides is missing; cannot extract USER_ID/BOT_ID");
    }

    // check user_id/bot_id
    if user_id.is_none() || bot_id.is_none() {
        tracing::warn!("Missing USER_ID/BOT_ID in overrides; skip processing. taskArn={}", task_arn);
        return Ok(());
    }

    // 4) check exit code
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

    // if not due to memory-related, skip
    if !is_memory_related_stop(
        exit,
        stop_code
    ) {
        tracing::warn!(
                "Task did not exit due to a memory-related reason; skipping restart. taskArn={}",
                task_arn
            );
        return Ok(());
    }

    let new_task_id = state
        .run_task
        .execute(
            user_id.as_deref().unwrap(),
            bot_id.as_deref().unwrap(),
            &state.configs.ecs.cluster_arn,
            &state.configs.ecs.td_passivbot_v741_arn,
            &state.configs.ecs.td_passivbot_v741_container_name,
        )
        .await
        .map_err(|e| Error::from(format!("Failed to run task: {e:#}")))?;

    tracing::warn!("Started replacement task_id={}", new_task_id);

    Ok(())
}

fn is_memory_related_stop(
    exit_code: i32,
    stopped_code: &str,
) -> bool {
    // OOM kill / SIGKILL check
    if exit_code != 137 {
        return false;
    }

    if stopped_code.contains("UserInitiated") {
        return false
    }
    true
}

