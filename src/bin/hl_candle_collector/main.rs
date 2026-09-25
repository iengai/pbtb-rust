//! Daily collector of Hyperliquid 1m candles for the lab's backtests.
//! Hyperliquid serves only the latest 5000 candles of an interval, about 3.5
//! days of minutes, so the history exists only where this keeps it.

use std::sync::Arc;

use aws_lambda_events::event::eventbridge::EventBridgeEvent;
use lambda_runtime::{Error, LambdaEvent, run, service_fn};

use crate::config::HlCandleCollectorConfig;
use crate::store::CandleStore;
use pbtb_rust::config::configs::load_config;
use pbtb_rust::infra::S3TemplateRepository;
use pbtb_rust::infra::client::create_s3_client;
use pbtb_rust::observability::Telemetry;

mod config;
mod days;
mod event_handler;
mod hyperliquid;
mod store;

/// Cold-start state, reused across warm invocations.
pub struct AppState {
    configs: HlCandleCollectorConfig,
    /// The `predefined/` templates, for the coins to collect.
    templates: S3TemplateRepository,
    store: CandleStore,
    http: reqwest::Client,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let telemetry = Arc::new(Telemetry::init("hl-candle-collector"));

    let configs: HlCandleCollectorConfig =
        load_config().map_err(|e| Error::from(format!("Failed to load configs: {e:#}")))?;

    let s3_client = create_s3_client(&configs.s3).await;
    let templates = S3TemplateRepository::new(s3_client.clone(), configs.s3.bucket_name.clone());
    let store = CandleStore::new(s3_client, &configs.candles);

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| Error::from(format!("Failed to build HTTP client: {e}")))?;

    let state = Arc::new(AppState {
        configs,
        templates,
        store,
        http,
    });

    run(service_fn(move |event: LambdaEvent<EventBridgeEvent>| {
        let state = state.clone();
        let telemetry = telemetry.clone();
        async move {
            let result = event_handler::function_handler(event, state).await;
            telemetry.flush();
            result
        }
    }))
    .await
}
