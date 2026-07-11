use serde::Deserialize;

/// Where the return-curve collector writes its per-bot JSON artifacts. A
/// separate (private) bucket from the credential-bearing bot-configs bucket; the
/// Pages publish step pulls from here.
#[derive(Debug, Deserialize)]
pub struct ChartConfig {
    pub bucket_name: String,
    /// Key prefix under which per-bot `<bot_id>.json` objects are written.
    #[serde(default = "default_key_prefix")]
    pub key_prefix: String,
}

fn default_key_prefix() -> String {
    "charts".to_string()
}
