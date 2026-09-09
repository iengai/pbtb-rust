//! DynamoDB fixture for the integration suites: a client, a table of its own,
//! and whatever server is available to serve them.
//!
//! Two servers are possible and the order matters. The Dev Container — the
//! canonical build environment — has no Docker socket, so `testcontainers`
//! cannot start anything there; what it does have is the compose
//! `dynamodb-local` service on the same network, announced by
//! `APP__DYNAMODB__ENDPOINT_URL`. Preferring that endpoint is what makes these
//! tests actually run in the container instead of self-skipping into a green
//! result. Elsewhere (CI, a host shell) there is a Docker daemon and no
//! endpoint, so `testcontainers` supplies one.
//!
//! Each fixture creates its own uniquely named table, because the server is
//! long-lived and shared: a fixed name would leak rows between test runs and
//! between tests within a run. That is what lets one server serve a whole test
//! binary — a container per fixture asks a CI runner for one JVM per test, and
//! they start, then drop connections mid-request once enough of them are up.

use aws_sdk_dynamodb::Client;
use aws_sdk_dynamodb::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_dynamodb::types::{
    AttributeDefinition, BillingMode, KeySchemaElement, KeyType, ScalarAttributeType,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage};
use tokio::sync::OnceCell;

/// The endpoint the Dev Container publishes for its compose DynamoDB Local.
const ENDPOINT_ENV: &str = "APP__DYNAMODB__ENDPOINT_URL";

static TABLE_SEQ: AtomicU64 = AtomicU64::new(0);

pub struct Dynamo {
    pub client: Client,
    pub table: String,
}

/// The container this binary's fixtures share, started once on first use.
///
/// Held in a `static` so it outlives every test; nothing drops it, and the
/// testcontainers reaper removes it when the process exits.
static SERVER: OnceCell<Result<Server, String>> = OnceCell::const_new();

struct Server {
    _container: ContainerAsync<GenericImage>,
    endpoint: String,
}

/// A DynamoDB client with static dummy credentials, so no credential provider
/// chain is consulted offline.
pub fn client_for(endpoint: &str) -> Client {
    let creds = Credentials::new("test", "test", None, None, "pbtb-tests");
    let conf = aws_sdk_dynamodb::config::Builder::new()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .endpoint_url(endpoint)
        .credentials_provider(creds)
        .build();
    Client::from_conf(conf)
}

fn unique_table() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let seq = TABLE_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("pbtb-test-{nanos}-{seq}")
}

/// Create the single-table schema: pk (HASH, S) + sk (RANGE, S). Waits for
/// ACTIVE.
pub async fn create_table(client: &Client, table: &str) -> Result<(), String> {
    if let Err(e) = client
        .create_table()
        .table_name(table)
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

    for _ in 0..30 {
        let desc = client
            .describe_table()
            .table_name(table)
            .send()
            .await
            .map_err(|e| format!("describe_table failed: {e}"))?;
        if let Some(t) = desc.table()
            && let Some(status) = t.table_status()
            && status.as_str() == "ACTIVE"
        {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    Err("table did not become ACTIVE in time".to_string())
}

/// Turn a skip into a failure. Set on CI, where "no server" means the job is
/// misconfigured and the suites would otherwise report a green result having
/// asserted nothing.
fn skip(reason: String) -> Option<Dynamo> {
    if std::env::var("CI").is_ok() {
        panic!("DynamoDB Local is required on CI but was unavailable: {reason}");
    }
    println!("Skipping DynamoDB integration tests: {reason}");
    None
}

/// A fixture backed by whatever DynamoDB Local is reachable. `None` (with a
/// printed reason) when neither an endpoint nor Docker is available, so a
/// machine with neither keeps the suite green.
pub async fn start() -> Option<Dynamo> {
    let table = unique_table();

    if let Ok(endpoint) = std::env::var(ENDPOINT_ENV)
        && !endpoint.is_empty()
    {
        let client = client_for(&endpoint);
        match create_table(&client, &table).await {
            Ok(()) => return Some(Dynamo { client, table }),
            Err(e) => {
                println!("{ENDPOINT_ENV}={endpoint} is set but unusable ({e}); trying Docker.")
            }
        }
    }

    let endpoint = match server().await {
        Ok(server) => server.endpoint.clone(),
        Err(e) => return skip(e.clone()),
    };
    let client = client_for(&endpoint);

    // DynamoDB Local prints its startup banner (the wait-for message) before its
    // TCP listener is actually accepting connections, so the first request can
    // fail with a dispatch error. Retry table setup briefly before giving up.
    let mut last_err = String::new();
    for _ in 0..30 {
        match create_table(&client, &table).await {
            Ok(()) => return Some(Dynamo { client, table }),
            Err(e) => {
                last_err = e;
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }
        }
    }
    skip(format!("table setup failed ({last_err})"))
}

/// Start this binary's container, or hand back the one already running.
async fn server() -> &'static Result<Server, String> {
    SERVER
        .get_or_init(|| async {
            let image = GenericImage::new("amazon/dynamodb-local", "latest")
                .with_exposed_port(8000.tcp())
                .with_wait_for(WaitFor::message_on_stdout(
                    "Initializing DynamoDB Local with the following configuration",
                ));

            let container = image
                .start()
                .await
                .map_err(|e| format!("no endpoint and no container ({e})"))?;
            let port = container
                .get_host_port_ipv4(8000.tcp())
                .await
                .map_err(|e| format!("failed to map the container port ({e})"))?;

            Ok(Server {
                _container: container,
                endpoint: format!("http://127.0.0.1:{port}"),
            })
        })
        .await
}
