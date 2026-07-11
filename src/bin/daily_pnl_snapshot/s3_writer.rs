use anyhow::{Context, Result};
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;

use pbtb_rust::config::chart::ChartConfig;

use crate::model::BotReturnSeries;

/// Write a bot's return series to `s3://<chart bucket>/<prefix>/<bot_id>.json`.
/// A short cache lifetime keeps the daily-updated curve reasonably fresh behind
/// a CDN without a per-request origin hit.
pub async fn put(
    client: &Client,
    cfg: &ChartConfig,
    bot_id: &str,
    series: &BotReturnSeries,
) -> Result<()> {
    let key = format!("{}/{}.json", cfg.key_prefix.trim_matches('/'), bot_id);
    let body = serde_json::to_vec(series).context("serialize return series")?;

    client
        .put_object()
        .bucket(&cfg.bucket_name)
        .key(&key)
        .body(ByteStream::from(body))
        .content_type("application/json")
        .cache_control("public, max-age=300")
        .send()
        .await
        .context("put chart json to S3")?;

    Ok(())
}
