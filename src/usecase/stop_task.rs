use anyhow::{Context, Result};
use async_trait::async_trait;

/// Whether an ECS task is still alive, as reported authoritatively by ECS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskLiveness {
    Alive,
    Gone,
}

/// Port for controlling a running ECS task — stopping it, and checking whether
/// it is still alive. Lets the use cases depend on an abstraction (testable with
/// a mock) rather than the concrete ECS client.
#[async_trait]
pub trait TaskController: Send + Sync {
    async fn stop(&self, cluster_arn: &str, task_id: &str, reason: &str) -> Result<()>;
    /// Authoritative liveness for a specific task id (ECS DescribeTasks). Used to
    /// confirm a task is truly gone before a time-based stale-lock reclaim, so a
    /// live task whose RUNNING event was lost is never double-launched.
    async fn liveness(&self, cluster_arn: &str, task_id: &str) -> Result<TaskLiveness>;
}

pub struct EcsTaskController {
    ecs_client: aws_sdk_ecs::Client,
}

impl EcsTaskController {
    pub fn new(client: aws_sdk_ecs::Client) -> Self {
        Self { ecs_client: client }
    }

    pub async fn stop_task(&self, cluster_arn: &str, task_id: &str, reason: &str) -> Result<()> {
        // ECS stamps a StopTask-initiated stop with stopCode `UserInitiated`, so
        // the reconcile Lambda treats the resulting STOPPED event as non-restart.
        self.ecs_client
            .stop_task()
            .cluster(cluster_arn)
            .task(task_id)
            .reason(reason)
            .send()
            .await
            .context("ecs stop_task failed")?;

        tracing::info!("ecs stop_task issued: task_id={}", task_id);
        Ok(())
    }

    pub async fn describe_liveness(
        &self,
        cluster_arn: &str,
        task_id: &str,
    ) -> Result<TaskLiveness> {
        let resp = self
            .ecs_client
            .describe_tasks()
            .cluster(cluster_arn)
            .tasks(task_id)
            .send()
            .await
            .context("ecs describe_tasks failed")?;

        // A task absent from ECS (aged out of the stopped-task window, or never
        // existed) is gone. A found task is gone only once ECS reports STOPPED;
        // every pre-terminal state (incl. STOPPING) counts as alive so a winding-
        // down task is never raced by a replacement.
        let Some(task) = resp.tasks().first() else {
            return Ok(TaskLiveness::Gone);
        };
        let status = task.last_status().unwrap_or("");
        Ok(if status.eq_ignore_ascii_case("STOPPED") {
            TaskLiveness::Gone
        } else {
            TaskLiveness::Alive
        })
    }
}

#[async_trait]
impl TaskController for EcsTaskController {
    async fn stop(&self, cluster_arn: &str, task_id: &str, reason: &str) -> Result<()> {
        self.stop_task(cluster_arn, task_id, reason).await
    }
    async fn liveness(&self, cluster_arn: &str, task_id: &str) -> Result<TaskLiveness> {
        self.describe_liveness(cluster_arn, task_id).await
    }
}
