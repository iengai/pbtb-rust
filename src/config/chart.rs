use serde::Deserialize;

/// Where the return-curve collector writes its per-bot JSON artifacts, and
/// where the API reads them back for their owner. A separate, private bucket
/// from the credential-bearing bot-configs bucket; nothing in it is published.
#[derive(Debug, Deserialize)]
pub struct ChartConfig {
    pub bucket_name: String,
    /// Key prefix under which per-bot `{user_id}/{bot_id}.json` objects are
    /// written.
    #[serde(default = "default_key_prefix")]
    pub key_prefix: String,
}

fn default_key_prefix() -> String {
    "charts".to_string()
}
