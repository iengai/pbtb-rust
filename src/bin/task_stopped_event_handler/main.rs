use std::sync::Arc;

use lambda_runtime::{run, service_fn, tracing, Error, LambdaEvent};
use aws_lambda_events::event::eventbridge::EventBridgeEvent;

use pbtb_rust::config::configs::{load_config};
use pbtb_rust::infra::client::create_ecs_client;
use pbtb_rust::usecase::RunTaskUseCase;
use crate::config::TaskStoppedConfig;

mod event_handler;
mod config;

#[derive(Clone)]
pub struct AppState {
    configs: Arc<TaskStoppedConfig>,
    run_task: Arc<RunTaskUseCase>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing::init_default_subscriber();

    // 冷启动初始化：只执行一次
    let configs: TaskStoppedConfig = load_config()
        .map_err(|e| Error::from(format!("Failed to load configs: {e:#}")))?;
    let ecs_client = create_ecs_client(&configs.ecs).await;

    let state = AppState {
        configs: Arc::new(configs),
        run_task: Arc::new(RunTaskUseCase::new(ecs_client)),
    };

    // 每次 invocation 复用同一个 state（热启动不会重新 init）
    run(service_fn({
        let state = Arc::new(state);
        move |event: LambdaEvent<EventBridgeEvent>| {
            let state = state.clone();
            async move { event_handler::function_handler(event, state).await }
        }
    }))
        .await
}