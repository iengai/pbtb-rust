use pbtb_rust::config::hyperliquid::HyperliquidConfig;
use pbtb_rust::config::s3::S3Config;
use serde::Deserialize;

/// Config for the Hyperliquid 1m candle collector Lambda. Populated from
/// `APP__*` env vars by `pbtb_rust::config::configs::load_config`.
#[derive(Debug, Deserialize)]
pub struct HlCandleCollectorConfig {
    /// The bot-configs bucket, read for its `predefined/` templates only.
    pub s3: S3Config,
    /// Only `base_url` is read.
    #[serde(default)]
    pub hyperliquid: HyperliquidConfig,
    pub candles: CandlesConfig,
}

#[derive(Debug, Deserialize)]
pub struct CandlesConfig {
    /// The private bucket the day objects are written to.
    pub bucket_name: String,
    /// Key prefix above `<COIN>/<YYYY-MM-DD>.json`.
    #[serde(default = "default_key_prefix")]
    pub key_prefix: String,
    /// Pause after every candle request. A day of 1m candles weighs about 44
    /// against Hyperliquid's 1200 per minute per IP, so a catch-up run of
    /// several days per coin needs roughly 2.2 s between requests.
    #[serde(default = "default_request_interval_ms")]
    pub request_interval_ms: u64,
}

fn default_key_prefix() -> String {
    "hyperliquid/1m".to_string()
}

fn default_request_interval_ms() -> u64 {
    2500
}
