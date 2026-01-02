
mod config;
mod domain;
mod infra;
mod interface;
mod usecase;

use std::sync::Arc;
use anyhow::Context;
use teloxide::prelude::*;
use infra::{DynamoBotRepository, S3TemplateRepository, S3BotConfigRepository, S3ApiKeyRepository};
use usecase::*;
use domain::SystemClock;
use pbtb_rust::config::configs::{load_config, Configs};
use pbtb_rust::infra::client::{setup_dynamodb_with_configs,setup_s3_with_configs};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Starting Telegram bot...");

    // Initialize logger
    env_logger::init();

    // Read token from TELEGRAM_BOT_TOKEN environment variable
    let bot = Bot::from_env();
    let configs: Configs = load_config()
        .context("Failed to setup application during startup")?;

    // Setup DynamoDB
    let (dynamodb_client, table_name) = setup_dynamodb_with_configs(&configs).await;
    // Setup S3
    let (s3_client, bucket_name) = setup_s3_with_configs(&configs).await;

    // Create repositories
    let bot_repository = Arc::new(DynamoBotRepository::new(dynamodb_client, table_name));
    let template_repository = Arc::new(S3TemplateRepository::new(s3_client.clone(), bucket_name.clone()));
    let bot_config_repository = Arc::new(S3BotConfigRepository::new(s3_client.clone(), bucket_name.clone()));
    let api_keys_repository = Arc::new(S3ApiKeyRepository::new(s3_client, bucket_name));

    // Create clock
    let clock = Arc::new(SystemClock);

    // Create use cases - Bot management
    let list_bots_usecase = Arc::new(ListBotsUseCase::new(bot_repository.clone()));
    let add_bot_usecase = Arc::new(AddBotUseCase::new(
        bot_repository.clone(),
        api_keys_repository.clone(),
        clock.clone(),
    ));
    let delete_bot_usecase = Arc::new(DeleteBotUseCase::new(
        bot_repository.clone(),
        api_keys_repository.clone(),
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
    };

    interface::telegram::router::run(bot, deps).await
}