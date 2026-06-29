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
use pbtb_rust::domain::runtime::{
    BotRuntime, BotRuntimeRepository, RuntimePhase, StartClaim, StartLockRepository,
};
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
        .endpoint_url(format!("http://127.0.0.1:{port}"))
        .credentials_provider(creds)
        .build();
    Client::from_conf(conf)
}

/// Create the single-table schema: pk (HASH, S) + sk (RANGE, S), PAY_PER_REQUEST.
/// Waits until the table reports ACTIVE.
async fn create_table(client: &Client) -> Result<(), String> {
    if let Err(e) = client
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
    {
        // A prior attempt in the retry loop may have already created the table;
        // treat that as success and fall through to the ACTIVE poll. Any other
        // error (incl. the dispatch failure while the listener is still coming
        // up) propagates so the caller can retry.
        let msg = format!("{e:?}");
        if !msg.contains("ResourceInUseException") {
            return Err(format!("create_table failed: {e}"));
        }
    }

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

    // DynamoDB Local prints its startup banner (the wait-for message) before its
    // TCP listener is actually accepting connections, so the first request can
    // fail with a dispatch error. Retry table setup briefly before giving up.
    let mut last_err = String::new();
    for _ in 0..30 {
        match create_table(&client).await {
            Ok(()) => return Some((container, client)),
            Err(e) => {
                last_err = e;
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }
        }
    }
    println!("Skipping DynamoDB integration tests: table setup failed ({last_err}).");
    None
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
        .expect("find should not error")
        .expect("bot should be found");
    assert_eq!(found.id, "alpha-bot");
    assert_eq!(found.user_id, "user-1");
    assert_eq!(found.name, "alpha-bot");
    assert_eq!(found.api_key, "ak-123");
    assert_eq!(found.secret_key, "sk-456");
    assert!(!found.enabled, "Bot::create starts disabled");
    assert_eq!(found.created_at, 1_700_000_000);

    // --- find for non-existent id returns None ---
    let missing = BotRepository::find(&repo, "user-1", "does-not-exist")
        .await
        .expect("find should not error");
    assert!(missing.is_none());

    // --- mutate (enable) then save then find reflects the update ---
    let mut updated = found.clone();
    updated.enable(1_700_000_100);
    repo.save(&updated)
        .await
        .expect("update save should succeed");

    let refound = BotRepository::find(&repo, "user-1", "alpha-bot")
        .await
        .expect("find should not error")
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

    let all = repo
        .find_by_user_id("user-1")
        .await
        .expect("find_by_user_id should not error");
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
        &BotRuntime::stopped(
            "user-1".to_string(),
            "alpha-bot".to_string(),
            1,
            1_700_000_300,
        ),
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
    let after_delete = BotRepository::find(&repo, "user-1", "alpha-bot")
        .await
        .expect("find should not error");
    assert!(after_delete.is_none(), "bot should be gone after delete");
}

/// The money-critical exclusive-start lock: exercises the atomic CAS, the
/// starting->running transition via the observed-write path, AlreadyRunning /
/// AlreadyStarting reporting, release, and stale-lock recovery against real
/// DynamoDB Local. Skips gracefully without Docker.
#[tokio::test]
async fn start_lock_cas_and_lifecycle() {
    let Some((_container, client)) = start_dynamodb().await else {
        return; // Docker unavailable: skip gracefully.
    };
    let repo = DynamoBotRepository::new(client, TABLE_NAME.to_string());
    let (u, b) = ("user-1", "lock-bot");

    // Absent row -> claim succeeds; row is `starting` with no task id yet.
    assert_eq!(
        repo.try_acquire_start(u, b, 1000, 600).await.unwrap(),
        StartClaim::Acquired
    );
    let rt = BotRuntimeRepository::find(&repo, u, b)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rt.phase, RuntimePhase::Starting);
    assert_eq!(rt.task_id, None);

    // A fresh `starting` lock blocks a second claim — never two launches.
    assert_eq!(
        repo.try_acquire_start(u, b, 1001, 600).await.unwrap(),
        StartClaim::AlreadyStarting
    );

    // Attaching the task id keeps the lock `starting` and the claim timestamp.
    repo.attach_started_task(u, b, "task-1").await.unwrap();
    let rt = BotRuntimeRepository::find(&repo, u, b)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rt.phase, RuntimePhase::Starting);
    assert_eq!(rt.task_id.as_deref(), Some("task-1"));

    // The Lambda's RUNNING write (later observed_at) transitions starting->running.
    BotRuntimeRepository::record(
        &repo,
        &BotRuntime::running(u.into(), b.into(), "task-1".into(), 1, 1010),
    )
    .await
    .unwrap();
    assert_eq!(
        BotRuntimeRepository::find(&repo, u, b)
            .await
            .unwrap()
            .unwrap()
            .phase,
        RuntimePhase::Running
    );

    // While running, a claim reports AlreadyRunning (no relaunch).
    assert_eq!(
        repo.try_acquire_start(u, b, 1020, 600).await.unwrap(),
        StartClaim::AlreadyRunning
    );

    // After a stop, a new claim succeeds.
    BotRuntimeRepository::record(&repo, &BotRuntime::stopped(u.into(), b.into(), 1, 1030))
        .await
        .unwrap();
    assert_eq!(
        repo.try_acquire_start(u, b, 1040, 600).await.unwrap(),
        StartClaim::Acquired
    );

    // release_start rolls a held lock back to stopped.
    repo.release_start(u, b, 1050).await.unwrap();
    let rt = BotRuntimeRepository::find(&repo, u, b)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rt.phase, RuntimePhase::Stopped);
    assert_eq!(rt.task_id, None);

    // Stale-lock recovery: a held lock is not stealable while fresh, but is
    // reclaimable once older than stale_after.
    assert_eq!(
        repo.try_acquire_start(u, b, 2000, 600).await.unwrap(),
        StartClaim::Acquired
    );
    assert_eq!(
        repo.try_acquire_start(u, b, 2100, 600).await.unwrap(),
        StartClaim::AlreadyStarting
    );
    assert_eq!(
        repo.try_acquire_start(u, b, 2700, 600).await.unwrap(),
        StartClaim::Acquired
    );
}

/// The restart lock keyed on the stopped task id: a stopped task can be replaced
/// at most once, so a duplicate or late STOPPED event never double-launches.
#[tokio::test]
async fn start_lock_restart_is_idempotent_per_stopped_task() {
    let Some((_container, client)) = start_dynamodb().await else {
        return; // Docker unavailable: skip gracefully.
    };
    let repo = DynamoBotRepository::new(client, TABLE_NAME.to_string());
    let (u, b) = ("user-1", "restart-bot");

    // A task is observed running at version 5.
    BotRuntimeRepository::record(
        &repo,
        &BotRuntime::running(u.into(), b.into(), "task-1".into(), 5, 1000),
    )
    .await
    .unwrap();

    // First STOPPED(task-1): claim succeeds, row -> starting, version bumped, id cleared.
    assert_eq!(
        repo.try_acquire_restart(u, b, "task-1", 1100)
            .await
            .unwrap(),
        StartClaim::Acquired
    );
    let rt = BotRuntimeRepository::find(&repo, u, b)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rt.phase, RuntimePhase::Starting);
    assert_eq!(
        rt.task_id, None,
        "task id cleared while the replacement launches"
    );
    assert_eq!(rt.version, 6, "restart counter bumped");

    // Duplicate STOPPED(task-1): the id is already cleared, so the claim is refused.
    // The restart path only distinguishes Acquired from "nothing to restart".
    assert_ne!(
        repo.try_acquire_restart(u, b, "task-1", 1101)
            .await
            .unwrap(),
        StartClaim::Acquired
    );

    // The replacement comes up as task-2.
    BotRuntimeRepository::record(
        &repo,
        &BotRuntime::running(u.into(), b.into(), "task-2".into(), 6, 1200),
    )
    .await
    .unwrap();

    // A late STOPPED(task-1) after task-2 is running is rejected — task-1 is no longer current.
    assert_ne!(
        repo.try_acquire_restart(u, b, "task-1", 1300)
            .await
            .unwrap(),
        StartClaim::Acquired
    );
    let rt = BotRuntimeRepository::find(&repo, u, b)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        rt.phase,
        RuntimePhase::Running,
        "the live task-2 is untouched by the stale event"
    );
    assert_eq!(rt.task_id.as_deref(), Some("task-2"));

    // STOPPED(task-2) (the current task) is honoured.
    assert_eq!(
        repo.try_acquire_restart(u, b, "task-2", 1400)
            .await
            .unwrap(),
        StartClaim::Acquired
    );
}

/// The observed `Stopping` write is identity-guarded: it marks the task it
/// actually stopped, but must NEVER clobber a newer generation (a restart lock a
/// concurrent reconcile just claimed, which clears the id and bumps the version).
/// A stale StopBot snapshot clobbering that lock would orphan the relaunched task
/// and break the runtime-row mirror the no-double-run invariant depends on.
#[tokio::test]
async fn stopping_write_is_identity_guarded() {
    let Some((_container, client)) = start_dynamodb().await else {
        return; // Docker unavailable: skip gracefully.
    };
    let repo = DynamoBotRepository::new(client, TABLE_NAME.to_string());
    let (u, b) = ("user-1", "stop-bot");

    // task-1 is running.
    BotRuntimeRepository::record(
        &repo,
        &BotRuntime::running(u.into(), b.into(), "task-1".into(), 1, 1000),
    )
    .await
    .unwrap();

    // POSITIVE: a Stopping write for the CURRENT task is honoured.
    BotRuntimeRepository::record(
        &repo,
        &BotRuntime::stopping(u.into(), b.into(), "task-1".into(), 1, 1100),
    )
    .await
    .unwrap();
    let rt = BotRuntimeRepository::find(&repo, u, b)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rt.phase, RuntimePhase::Stopping);
    assert_eq!(rt.task_id.as_deref(), Some("task-1"));

    // Reset to running, then a concurrent restart claim supersedes task-1: the row
    // becomes `starting` with the id cleared and the version bumped.
    BotRuntimeRepository::record(
        &repo,
        &BotRuntime::running(u.into(), b.into(), "task-1".into(), 1, 1200),
    )
    .await
    .unwrap();
    assert_eq!(
        repo.try_acquire_restart(u, b, "task-1", 1300)
            .await
            .unwrap(),
        StartClaim::Acquired
    );
    let claimed = BotRuntimeRepository::find(&repo, u, b)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.phase, RuntimePhase::Starting);
    assert_eq!(claimed.task_id, None, "restart cleared the id");
    let claimed_version = claimed.version;

    // NEGATIVE (the regression guard): a stale StopBot Stopping write for the OLD
    // task-1 must NOT clobber the fresh restart lock.
    BotRuntimeRepository::record(
        &repo,
        &BotRuntime::stopping(u.into(), b.into(), "task-1".into(), 1, 9999),
    )
    .await
    .unwrap();
    let after = BotRuntimeRepository::find(&repo, u, b)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        after.phase,
        RuntimePhase::Starting,
        "stale stopping must not clobber the restart lock"
    );
    assert_eq!(after.task_id, None, "the relaunch id is not orphaned");
    assert_eq!(after.version, claimed_version, "version untouched");
}

/// A terminal STOPPED always settles a `stopping` row, even one stamped slightly
/// in the future by a clock-ahead telebot — otherwise a dropped settlement would
/// leave the row stuck `stopping` forever (there is no sweeper). The same clause
/// must NOT let a stale STOPPED clobber a genuinely newer `running` row.
#[tokio::test]
async fn terminal_stopped_settles_future_stamped_stopping() {
    let Some((_container, client)) = start_dynamodb().await else {
        return; // Docker unavailable: skip gracefully.
    };
    let repo = DynamoBotRepository::new(client, TABLE_NAME.to_string());
    let (u, b) = ("user-1", "settle-bot");

    // Running task-1 at ua=4000, then a Stopping stamped in the FUTURE (ua=5000).
    BotRuntimeRepository::record(
        &repo,
        &BotRuntime::running(u.into(), b.into(), "task-1".into(), 1, 4000),
    )
    .await
    .unwrap();
    BotRuntimeRepository::record(
        &repo,
        &BotRuntime::stopping(u.into(), b.into(), "task-1".into(), 1, 5000),
    )
    .await
    .unwrap();
    assert_eq!(
        BotRuntimeRepository::find(&repo, u, b)
            .await
            .unwrap()
            .unwrap()
            .phase,
        RuntimePhase::Stopping
    );

    // A terminal STOPPED with an EARLIER observed_at still settles it.
    BotRuntimeRepository::record(&repo, &BotRuntime::stopped(u.into(), b.into(), 1, 3000))
        .await
        .unwrap();
    let settled = BotRuntimeRepository::find(&repo, u, b)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        settled.phase,
        RuntimePhase::Stopped,
        "STOPPED settles a future-stamped stopping row"
    );
    assert_eq!(settled.task_id, None);

    // Guard: a stale STOPPED must NOT clobber a genuinely newer RUNNING row.
    BotRuntimeRepository::record(
        &repo,
        &BotRuntime::running(u.into(), b.into(), "task-2".into(), 2, 6000),
    )
    .await
    .unwrap();
    BotRuntimeRepository::record(&repo, &BotRuntime::stopped(u.into(), b.into(), 2, 3500))
        .await
        .unwrap();
    let still = BotRuntimeRepository::find(&repo, u, b)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        still.phase,
        RuntimePhase::Running,
        "a stale stopped must not clobber a newer running row"
    );
    assert_eq!(still.task_id.as_deref(), Some("task-2"));
}
