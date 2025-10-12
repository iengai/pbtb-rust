mod config;
mod domain;
mod infra;
mod interface;
mod usecase;

use std::sync::Arc;
use teloxide::prelude::*;
use infra::client::setup_dynamodb;
use infra::botrepository::DynamoBotRepository;
use usecase::ListBotsUseCase;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Starting Telegram bot...");

    // Initialize logger
    env_logger::init();

    // Read token from TELEGRAM_BOT_TOKEN environment variable
    let bot = Bot::from_env();

    // Setup DynamoDB
    let (client, table_name) = setup_dynamodb()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to setup DynamoDB: {}", e))?;

    // Create repository
    let bot_repository = Arc::new(DynamoBotRepository::new(client, table_name));

    // Create use cases
    let list_bots_usecase = Arc::new(ListBotsUseCase::new(bot_repository.clone()));

    // Construct dependencies
    let deps = interface::telegram::Deps {
        list_bots_usecase,
    };

    interface::telegram::router::run(bot, deps).await
}