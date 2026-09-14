use serde::Deserialize;

/// Where the return-curve collector writes its per-bot JSON artifacts, and
/// where the API reads them back for their owner. A separate, private bucket
/// from the credential-bearing bot-configs bucket. Only the public prefix
/// leaves it: the showcase CDN serves it, and the pages-publish workflow
/// copies it to the site.
#[derive(Debug, Deserialize)]
pub struct ChartConfig {
    pub bucket_name: String,
    /// Key prefix under which per-bot `{user_id}/{bot_id}.json` objects are
    /// written.
    #[serde(default = "default_key_prefix")]
    pub key_prefix: String,
    /// Key prefix of the showcase artifacts (`index.json`, `bots/{id}.json`),
    /// the one part of the bucket that is published.
    #[serde(default = "default_public_prefix")]
    pub public_prefix: String,
    /// Key prefix of every operator bot's showcase artifact, shown or not,
    /// which the showcase switch publishes from. Never published itself.
    #[serde(default = "default_showcase_prefix")]
    pub showcase_prefix: String,
}

fn default_key_prefix() -> String {
    "charts".to_string()
}

fn default_public_prefix() -> String {
    "public".to_string()
}

fn default_showcase_prefix() -> String {
    "showcase".to_string()
}
