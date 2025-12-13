use anyhow::{anyhow, Context, Result};
use aws_sdk_ecs::types::{ContainerOverride, KeyValuePair, PropagateTags, TaskOverride};

pub struct RunTaskUseCase {
    ecs_client: aws_sdk_ecs::Client,
}

impl RunTaskUseCase {
    pub fn new(client: aws_sdk_ecs::Client) -> Self {
        Self { ecs_client: client }
    }

    pub async fn execute(
        &self,
        user_id: &str,
        bot_id: &str,
        cluster_arn: &str,
        td_arn: &str,
    ) -> Result<String> {
        let container_name = td_arn
            .rsplit('/')
            .next()
            .context("failed to derive container_name from task definition arn")?;

        let overrides = TaskOverride::builder()
            .container_overrides(
                ContainerOverride::builder()
                    .name(container_name)
                    .environment(KeyValuePair::builder().name("USER_ID").value(user_id).build())
                    .environment(KeyValuePair::builder().name("BOT_ID").value(bot_id).build())
                    .build(),
            )
            .build();

        let resp = self
            .ecs_client
            .run_task()
            .cluster(cluster_arn)
            .task_definition(td_arn)
            .overrides(overrides)
            .enable_ecs_managed_tags(true)
            .propagate_tags(PropagateTags::TaskDefinition)
            .send()
            .await
            .context("ecs run_task failed")?;

        let started = resp.tasks().len();
        let failed = resp.failures().len();
        tracing::info!("ecs run_task done: started_tasks={}, failures={}", started, failed);

        if failed > 0 {
            // failures
            return Err(anyhow!("ecs run_task returned failures"));
        }

        let task_arn = resp
            .tasks()
            .first()
            .and_then(|t| t.task_arn())
            .ok_or_else(|| anyhow!("ecs run_task did not return a taskArn"))?;

        let task_id = task_arn
            .rsplit('/')
            .next()
            .ok_or_else(|| anyhow!("failed to parse task id from taskArn"))?
            .to_string();

        Ok(task_id)
    }
}