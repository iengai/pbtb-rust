use serde::Deserialize;

/// Bybit V5 REST settings for the return-curve collector. Per-bot API
/// credentials are NOT here — they are read from the encrypted store per bot;
/// this only carries non-secret client settings.
#[derive(Debug, Deserialize)]
pub struct BybitConfig {
    /// REST base URL, e.g. `https://api.bybit.com` (or the testnet host).
    pub base_url: String,
    /// `X-BAPI-RECV-WINDOW` in milliseconds; defaults to 5000 when unset.
    #[serde(default)]
    pub recv_window_ms: Option<u64>,
    /// Settlement/quote coin to track (transaction-log `currency` filter).
    #[serde(default = "default_settle_coin")]
    pub settle_coin: String,
    /// How many days of history to back-fill each run. Bybit retains ~730 days;
    /// once our own history exceeds the window this bounds each run's work.
    #[serde(default = "default_backfill_days")]
    pub backfill_days: i64,
}

fn default_settle_coin() -> String {
    "USDT".to_string()
}

fn default_backfill_days() -> i64 {
    730
}
