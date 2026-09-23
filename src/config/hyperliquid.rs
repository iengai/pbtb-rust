use serde::Deserialize;

/// Hyperliquid settings for the return-curve collector. The collector reads
/// only public `info` queries keyed by the account address, so it needs no
/// credential at all; every field has a default and the section may be
/// absent from the environment.
#[derive(Debug, Deserialize)]
pub struct HyperliquidConfig {
    /// API base URL, e.g. `https://api.hyperliquid.xyz` (or the testnet host).
    #[serde(default = "default_base_url")]
    pub base_url: String,
    /// How many days of history a bot's first run back-fills. Hyperliquid
    /// serves only an account's most recent fills (about ten thousand), so a
    /// busy account's history can end sooner; see the adapter.
    #[serde(default = "default_backfill_days")]
    pub backfill_days: i64,
}

impl Default for HyperliquidConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            backfill_days: default_backfill_days(),
        }
    }
}

fn default_base_url() -> String {
    "https://api.hyperliquid.xyz".to_string()
}

fn default_backfill_days() -> i64 {
    365
}
