use std::sync::Arc;

use lambda_runtime::{run, service_fn, tracing, Error, LambdaEvent};
use aws_lambda_events::event::eventbridge::EventBridgeEvent;

use pbtb_rust::config::configs::{load_config};
use pbtb_rust::domain::bot::BotRepository;
use pbtb_rust::domain::clock::SystemClock;
use pbtb_rust::domain::runtime::BotRuntimeRepository;
use pbtb_rust::infra::client::{create_ecs_client, create_dynamodb_client};
use pbtb_rust::infra::DynamoBotRepository;
use pbtb_rust::usecase::{ReconcileStoppedTaskUseCase, RecordRunningTaskUseCase, RunTaskUseCase, TaskRunner};
use crate::config::TaskStateChangeConfig;

mod event_handler;
mod config;

#[derive(Clone)]
pub struct AppState {
    configs: Arc<TaskStateChangeConfig>,
    reconcile: Arc<ReconcileStoppedTaskUseCase>,
    record_running: Arc<RecordRunningTaskUseCase>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing::init_default_subscriber();

    // 冷启动初始化：只执行一次
    let configs: TaskStateChangeConfig = load_config()
        .map_err(|e| Error::from(format!("Failed to load configs: {e:#}")))?;

    // ECS client for (re)starting tasks.
    let ecs_client = create_ecs_client(&configs.ecs).await;

    // DynamoDB client + repo for reading desired state (Bot.enabled) and
    // recording observed runtime.
    let dynamodb_client = create_dynamodb_client(&configs.dynamodb).await;
    let table_name = configs.dynamodb.table_name.clone();
    let repo = Arc::new(DynamoBotRepository::new(dynamodb_client, table_name));

    // The same repo satisfies both domain traits.
    let bots: Arc<dyn BotRepository> = repo.clone();
    let runtimes: Arc<dyn BotRuntimeRepository> = repo.clone();
    let runtimes_for_record: Arc<dyn BotRuntimeRepository> = repo.clone();

    let clock = Arc::new(SystemClock);
    let run_task: Arc<dyn TaskRunner> = Arc::new(RunTaskUseCase::new(ecs_client));
    let reconcile = Arc::new(ReconcileStoppedTaskUseCase::new(
        bots,
        runtimes,
        run_task,
        clock,
    ));
    // Observed-running recorder for the RUNNING branch (counterpart to reconcile).
    let record_running = Arc::new(RecordRunningTaskUseCase::new(runtimes_for_record));

    let state = AppState {
        configs: Arc::new(configs),
        reconcile,
        record_running,
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
