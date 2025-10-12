use std::sync::Once;
use uuid::Uuid;
use pbtb_rust::domain::bot::{Bot, BotRepository};
use pbtb_rust::infra::botrepository::DynamoBotRepository;
use pbtb_rust::infra::client::setup_dynamodb;

// Initialize test environment once
static INIT: Once = Once::new();

async fn setup() -> Option<(DynamoBotRepository, String)> {
    // Initialize test environment
    INIT.call_once(|| {
        // This will be called only once, even if setup() is called multiple times
        println!("Initializing test environment...");
    });

    // Set up DynamoDB client and table
    match setup_dynamodb().await {
        Ok((client, table_name)) => {
            // Create repository
            let repository = DynamoBotRepository::new(client, table_name.clone());
            Some((repository, table_name))
        },
        Err(e) => {
            println!("Failed to set up DynamoDB: {}. Skipping test.", e);
            None
        }
    }
}

fn create_test_bot() -> Bot {
    Bot::new(
        Uuid::new_v4().to_string(),
        "test-user".to_string(),
        "Test Bot".to_string(),
        "test-api-key".to_string(),
        "test-secret-key".to_string(),
        true,
    )
}

#[tokio::test]
async fn test_save_and_find_by_id() {
    // Set up test environment
    let Some((repository, _)) = setup().await else {
        println!("Skipping test_save_and_find_by_id because DynamoDB setup failed");
        return;
    };

    // Create a test bot
    let bot = create_test_bot();
    let bot_id = bot.id.clone();

    // Save the bot
    repository.save(&bot).await;

    // Find the bot by ID
    let found_bot = repository.find_by_id(&bot_id).await;

    // Assert that the bot was found and has the correct data
    assert!(found_bot.is_some());
    let found_bot = found_bot.unwrap();
    assert_eq!(found_bot.id, bot.id);
    assert_eq!(found_bot.user_id, bot.user_id);
    assert_eq!(found_bot.name, bot.name);
    assert_eq!(found_bot.api_key, bot.api_key);
    assert_eq!(found_bot.secret_key, bot.secret_key);
    assert_eq!(found_bot.enabled, bot.enabled);
}

#[tokio::test]
async fn test_find_by_id_not_found() {
    // Set up test environment
    let Some((repository, _)) = setup().await else {
        println!("Skipping test_find_by_id_not_found because DynamoDB setup failed");
        return;
    };

    // Generate a random ID that shouldn't exist
    let non_existent_id = Uuid::new_v4().to_string();

    // Try to find a bot with the non-existent ID
    let found_bot = repository.find_by_id(&non_existent_id).await;

    // Assert that no bot was found
    assert!(found_bot.is_none());
}

#[tokio::test]
async fn test_save_updates_existing_bot() {
    // Set up test environment
    let Some((repository, _)) = setup().await else {
        println!("Skipping test_save_updates_existing_bot because DynamoDB setup failed");
        return;
    };

    // Create a test bot
    let mut bot = create_test_bot();
    let bot_id = bot.id.clone();

    // Save the bot
    repository.save(&bot).await;

    // Update the bot
    bot.name = "Updated Bot Name".to_string();
    bot.enabled = false;

    // Save the updated bot
    repository.save(&bot).await;

    // Find the bot by ID
    let found_bot = repository.find_by_id(&bot_id).await;

    // Assert that the bot was found and has the updated data
    assert!(found_bot.is_some());
    let found_bot = found_bot.unwrap();
    assert_eq!(found_bot.name, "Updated Bot Name");
    assert_eq!(found_bot.enabled, false);
}
