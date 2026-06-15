//! Integration tests for `DynamoBotRepository` against a real DynamoDB Local
//! instance spun up via testcontainers.
//!
//! The whole suite is gated on Docker being available: if the container fails
//! to start (no Docker daemon, CI without docker-in-docker, etc.) the test
//! prints a skip message and returns successfully so `cargo test` stays green
//! in environments without Docker.
//!
//! testcontainers 0.24 API used (see report):
//!   - `GenericImage::new("amazon/dynamodb-local", "latest")`
//!   - `.with_exposed_port(8000.tcp())` (requires `IntoContainerPort` in scope)
//!   - `.with_wait_for(WaitFor::message_on_stdout("..."))`
//!   - `.start().await` (requires `AsyncRunner` in scope)
//!   - `container.get_host_port_ipv4(8000.tcp()).await`

use aws_sdk_dynamodb::Client;
use aws_sdk_dynamodb::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_dynamodb::types::{
    AttributeDefinition, BillingMode, KeySchemaElement, KeyType, ScalarAttributeType,
};

use pbtb_rust::domain::bot::{Bot, BotRepository};
use pbtb_rust::domain::runtime::{BotRuntime, BotRuntimeRepository, RuntimePhase};
use pbtb_rust::infra::botrepository::DynamoBotRepository;

use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage};

const TABLE_NAME: &str = "pbtb-test-table";

/// Build a DynamoDB client pointed at a local endpoint with static dummy
/// credentials (mirrors `src/infra/client.rs` but supplies fixed credentials
/// so no AWS credential provider chain is required offline).
fn local_client(port: u16) -> Client {
    let creds = Credentials::new("test", "test", None, None, "pbtb-tests");
    let conf = aws_sdk_dynamodb::config::Builder::new()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .endpoint_url(format!("http://127.0.0.1:{}", port))
        .credentials_provider(creds)
        .build();
    Client::from_conf(conf)
}

/// Create the single-table schema: pk (HASH, S) + sk (RANGE, S), PAY_PER_REQUEST.
/// Waits until the table reports ACTIVE.
async fn create_table(client: &Client) -> Result<(), String> {
    client
        .create_table()
        .table_name(TABLE_NAME)
        .billing_mode(BillingMode::PayPerRequest)
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("pk")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .map_err(|e| e.to_string())?,
        )
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("sk")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .map_err(|e| e.to_string())?,
        )
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("pk")
                .key_type(KeyType::Hash)
                .build()
                .map_err(|e| e.to_string())?,
        )
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("sk")
                .key_type(KeyType::Range)
                .build()
                .map_err(|e| e.to_string())?,
        )
        .send()
        .await
        .map_err(|e| format!("create_table failed: {e}"))?;

    // Poll until ACTIVE.
    for _ in 0..30 {
        let desc = client
            .describe_table()
            .table_name(TABLE_NAME)
            .send()
            .await
            .map_err(|e| format!("describe_table failed: {e}"))?;
        if let Some(table) = desc.table() {
            if let Some(status) = table.table_status() {
                if status.as_str() == "ACTIVE" {
                    return Ok(());
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    Err("table did not become ACTIVE in time".to_string())
}

/// Spin up DynamoDB Local. Returns `None` (with a printed skip message) if the
/// container cannot be started — keeping the suite green without Docker.
async fn start_dynamodb() -> Option<(ContainerAsync<GenericImage>, Client)> {
    let image = GenericImage::new("amazon/dynamodb-local", "latest")
        .with_exposed_port(8000.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "Initializing DynamoDB Local with the following configuration",
        ));

    let container = match image.start().await {
        Ok(c) => c,
        Err(e) => {
            println!("Skipping DynamoDB integration tests: failed to start container ({e}).");
            return None;
        }
    };

    let port = match container.get_host_port_ipv4(8000.tcp()).await {
        Ok(p) => p,
        Err(e) => {
            println!("Skipping DynamoDB integration tests: failed to map port ({e}).");
            return None;
        }
    };

    let client = local_client(port);

    if let Err(e) = create_table(&client).await {
        println!("Skipping DynamoDB integration tests: table setup failed ({e}).");
        return None;
    }

    Some((container, client))
}

#[tokio::test]
async fn dynamo_bot_repository_roundtrip() {
    let Some((_container, client)) = start_dynamodb().await else {
        return; // Docker unavailable: skip gracefully.
    };

    let repo = DynamoBotRepository::new(client, TABLE_NAME.to_string());

    // --- save then find returns the bot with matching fields ---
    let bot = Bot::create(
        "user-1".to_string(),
        "alpha-bot".to_string(),
        "ak-123".to_string(),
        "sk-456".to_string(),
        1_700_000_000,
    );
    repo.save(&bot).await.expect("save should succeed");

    let found = BotRepository::find(&repo, "user-1", "alpha-bot")
        .await
        .expect("bot should be found");
    assert_eq!(found.id, "alpha-bot");
    assert_eq!(found.user_id, "user-1");
    assert_eq!(found.name, "alpha-bot");
    assert_eq!(found.api_key, "ak-123");
    assert_eq!(found.secret_key, "sk-456");
    assert!(!found.enabled, "Bot::create starts disabled");
    assert_eq!(found.created_at, 1_700_000_000);

    // --- find for non-existent id returns None ---
    let missing = BotRepository::find(&repo, "user-1", "does-not-exist").await;
    assert!(missing.is_none());

    // --- mutate (enable) then save then find reflects the update ---
    let mut updated = found.clone();
    updated.enable(1_700_000_100);
    repo.save(&updated).await.expect("update save should succeed");

    let refound = BotRepository::find(&repo, "user-1", "alpha-bot")
        .await
        .expect("updated bot should be found");
    assert!(refound.enabled, "enable() should persist");
    assert_eq!(refound.updated_at, 1_700_000_100);

    // --- find_by_user_id returns all bots and excludes runtime rows ---
    let bot2 = Bot::create(
        "user-1".to_string(),
        "beta-bot".to_string(),
        "ak-2".to_string(),
        "sk-2".to_string(),
        1_700_000_000,
    );
    repo.save(&bot2).await.expect("save bot2 should succeed");

    // Record a runtime row for alpha-bot (sk = ecs_task_metadata#alpha-bot).
    BotRuntimeRepository::record(
        &repo,
        &BotRuntime::running(
            "user-1".to_string(),
            "alpha-bot".to_string(),
            "task-abc".to_string(),
            1,
            1_700_000_200,
        ),
    )
    .await
    .expect("record runtime should succeed");

    let all = repo.find_by_user_id("user-1").await;
    assert_eq!(
        all.len(),
        2,
        "should return both bots and NOT the ecs_task_metadata# runtime row"
    );
    let mut ids: Vec<String> = all.iter().map(|b| b.id.clone()).collect();
    ids.sort();
    assert_eq!(ids, vec!["alpha-bot".to_string(), "beta-bot".to_string()]);

    // --- BotRuntime: record(running) then find returns Running w/ task_id+version ---
    let rt = BotRuntimeRepository::find(&repo, "user-1", "alpha-bot")
        .await
        .expect("runtime find should not error")
        .expect("runtime should exist");
    assert_eq!(rt.phase, RuntimePhase::Running);
    assert_eq!(rt.task_id.as_deref(), Some("task-abc"));
    assert_eq!(rt.version, 1);

    // --- record(stopped) then find returns Stopped with task_id None ---
    BotRuntimeRepository::record(
        &repo,
        &BotRuntime::stopped("user-1".to_string(), "alpha-bot".to_string(), 1, 1_700_000_300),
    )
    .await
    .expect("record stopped should succeed");

    let rt_stopped = BotRuntimeRepository::find(&repo, "user-1", "alpha-bot")
        .await
        .expect("runtime find should not error")
        .expect("runtime should exist");
    assert_eq!(rt_stopped.phase, RuntimePhase::Stopped);
    assert_eq!(rt_stopped.task_id, None);

    // --- delete removes the bot ---
    repo.delete("user-1", "alpha-bot")
        .await
        .expect("delete should succeed");
    let after_delete = BotRepository::find(&repo, "user-1", "alpha-bot").await;
    assert!(after_delete.is_none(), "bot should be gone after delete");
}
