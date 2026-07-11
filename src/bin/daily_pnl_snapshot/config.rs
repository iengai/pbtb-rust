use pbtb_rust::config::bybit::BybitConfig;
use pbtb_rust::config::chart::ChartConfig;
use pbtb_rust::config::dynamodb::DynamoDBConfig;
use pbtb_rust::config::s3::S3Config;
use serde::Deserialize;

/// Config for the daily return-curve collector Lambda, composed from the shared
/// per-service config sections plus the Bybit/chart settings. Populated from
/// `APP__*` env vars by `pbtb_rust::config::configs::load_config`.
#[derive(Debug, Deserialize)]
pub struct DailyPnlSnapshotConfig {
    pub dynamodb: DynamoDBConfig,
    pub s3: S3Config,
    pub bybit: BybitConfig,
    pub chart: ChartConfig,
}
