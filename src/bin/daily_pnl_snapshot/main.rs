use std::sync::Arc;

use aws_lambda_events::event::eventbridge::EventBridgeEvent;
use lambda_runtime::{Error, LambdaEvent, run, service_fn, tracing};

use crate::config::DailyPnlSnapshotConfig;
use pbtb_rust::config::configs::load_config;
use pbtb_rust::infra::client::{create_dynamodb_client, create_s3_client};
use pbtb_rust::infra::{DynamoBotRepository, S3ApiKeyRepository};

mod bybit;
mod config;
mod event_handler;
mod model;
mod s3_writer;

/// Cold-start state, reused across warm invocations.
#[derive(Clone)]
pub struct AppState {
    configs: Arc<DailyPnlSnapshotConfig>,
    /// Concrete repo: `find_all` (bot enumeration) and `list_for_bot` (the
    /// config-switch timeline) are both read off it.
    bots: Arc<DynamoBotRepository>,
    /// Reads each bot's Bybit key/secret from the encrypted S3 store.
    api_keys: Arc<S3ApiKeyRepository>,
    /// Writes the per-bot chart JSON (to the chart bucket, not the S3 config
    /// above — the client is shared, the bucket name is not).
    s3: aws_sdk_s3::Client,
    http: reqwest::Client,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing::init_default_subscriber();

    // Cold-start init: run once.
    let configs: DailyPnlSnapshotConfig =
        load_config().map_err(|e| Error::from(format!("Failed to load configs: {e:#}")))?;

    let dynamodb_client = create_dynamodb_client(&configs.dynamodb).await;
    let bots = Arc::new(DynamoBotRepository::new(
        dynamodb_client,
        configs.dynamodb.table_name.clone(),
    ));

    let s3_client = create_s3_client(&configs.s3).await;
    let api_keys = Arc::new(S3ApiKeyRepository::new(
        s3_client.clone(),
        configs.s3.bucket_name.clone(),
    ));

    let http = reqwest::Client::builder()
        .build()
        .map_err(|e| Error::from(format!("Failed to build HTTP client: {e}")))?;

    let state = Arc::new(AppState {
        configs: Arc::new(configs),
        bots,
        api_keys,
        s3: s3_client,
        http,
    });

    run(service_fn(move |event: LambdaEvent<EventBridgeEvent>| {
        let state = state.clone();
        async move { event_handler::function_handler(event, state).await }
    }))
    .await
}
