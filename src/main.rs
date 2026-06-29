use anyhow::Context;
use pbtb_rust::config::configs::{Configs, load_config};
use pbtb_rust::domain::{self, SystemClock};
use pbtb_rust::infra::client::{
    setup_dynamodb_with_configs, setup_ecs_with_configs, setup_s3_with_configs,
};
use pbtb_rust::infra::{
    DynamoBotRepository, S3ApiKeyRepository, S3BotConfigRepository, S3TemplateRepository,
};
use pbtb_rust::interface;
use pbtb_rust::usecase::*;
use std::sync::Arc;
use teloxide::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Starting Telegram bot...");

    // Initialize logger
    env_logger::init();

    // Read token from TELEGRAM_BOT_TOKEN environment variable
    let bot = Bot::from_env();
    let configs: Configs = load_config().context("Failed to setup application during startup")?;

    // Setup DynamoDB
    let (dynamodb_client, table_name) = setup_dynamodb_with_configs(&configs).await;
    // Setup S3
    let (s3_client, bucket_name) = setup_s3_with_configs(&configs).await;
    // Setup ECS (for telebot Run/Stop -> RunTask/StopTask actuation)
    let (ecs_client, cluster_arn, td_arn) = setup_ecs_with_configs(&configs).await;
    let container_name = configs.ecs.td_passivbot_container_name.clone();

    // Create repositories
    let bot_repository = Arc::new(DynamoBotRepository::new(dynamodb_client, table_name));
    let template_repository = Arc::new(S3TemplateRepository::new(
        s3_client.clone(),
        bucket_name.clone(),
    ));
    let bot_config_repository = Arc::new(S3BotConfigRepository::new(
        s3_client.clone(),
        bucket_name.clone(),
    ));
    let api_keys_repository = Arc::new(S3ApiKeyRepository::new(s3_client, bucket_name));
    // Use cases depend on the domain ApiKeyRepository port, not the concrete infra impl.
    let api_keys_repo: Arc<dyn domain::ApiKeyRepository> = api_keys_repository.clone();

    // Create clock
    let clock = Arc::new(SystemClock);

    // Create use cases - Bot management
    let list_bots_usecase = Arc::new(ListBotsUseCase::new(bot_repository.clone()));
    let add_bot_usecase = Arc::new(AddBotUseCase::new(
        bot_repository.clone(),
        api_keys_repo.clone(),
        clock.clone(),
    ));
    let delete_bot_usecase = Arc::new(DeleteBotUseCase::new(
        bot_repository.clone(),
        api_keys_repo.clone(),
    ));

    // Create use cases - Template management
    let list_templates_usecase = Arc::new(ListTemplatesUseCase::new(template_repository.clone()));

    // Create use cases - Bot config management
    let apply_template_usecase = Arc::new(ApplyTemplateUseCase::new(
        template_repository.clone(),
        bot_config_repository.clone(),
        clock.clone(),
    ));
    let get_bot_config_usecase = Arc::new(GetBotConfigUseCase::new(bot_config_repository.clone()));
    let update_bot_config_usecase = Arc::new(UpdateBotConfigUseCase::new(
        bot_config_repository.clone(),
        clock.clone(),
    ));
    let update_risk_level_usecase = Arc::new(UpdateRiskLevelUseCase::new(
        bot_config_repository.clone(),
        clock.clone(),
    ));
    let set_strategy_side_usecase = Arc::new(SetStrategySideUseCase::new(
        bot_config_repository.clone(),
        clock.clone(),
    ));

    // Create use cases - Runtime / desired-state management
    // DynamoBotRepository implements BotRepository, BotRuntimeRepository and
    // StartLockRepository, so coerce the same Arc into each trait object.
    let bots_dyn: Arc<dyn domain::BotRepository> = bot_repository.clone();
    let runtimes_dyn: Arc<dyn domain::BotRuntimeRepository> = bot_repository.clone();
    let start_locks: Arc<dyn domain::StartLockRepository> = bot_repository.clone();
    let get_bot_runtime_usecase = Arc::new(GetBotRuntimeUseCase::new(runtimes_dyn.clone()));

    // Create use cases - ECS actuation (Run/Stop buttons -> RunTask/StopTask)
    let task_runner: Arc<dyn TaskRunner> = Arc::new(RunTaskUseCase::new(ecs_client.clone()));
    let task_controller: Arc<dyn TaskController> = Arc::new(EcsTaskController::new(ecs_client));
    let start_bot_usecase = Arc::new(StartBotUseCase::new(
        bots_dyn.clone(),
        runtimes_dyn.clone(),
        start_locks,
        task_runner,
        task_controller.clone(),
        clock.clone(),
        cluster_arn.clone(),
        td_arn,
        container_name,
    ));
    let stop_bot_usecase = Arc::new(StopBotUseCase::new(
        bots_dyn,
        runtimes_dyn,
        task_controller,
        clock.clone(),
        cluster_arn,
    ));

    // Construct dependencies
    let deps = interface::telegram::Deps {
        // Bot management
        list_bots_usecase,
        add_bot_usecase,
        delete_bot_usecase,
        // Template management
        list_templates_usecase,
        // Bot config management
        apply_template_usecase,
        get_bot_config_usecase,
        update_bot_config_usecase,
        update_risk_level_usecase,
        set_strategy_side_usecase,
        // Runtime / desired-state management
        get_bot_runtime_usecase,
        // ECS actuation
        start_bot_usecase,
        stop_bot_usecase,
    };

    interface::telegram::router::run(bot, deps).await
}
