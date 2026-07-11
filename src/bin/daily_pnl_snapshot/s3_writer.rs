//! S3 IO for the collector: the PUBLIC per-bot series (keyed by anonymized
//! label, under the published prefix) and the PRIVATE per-bot state (keyed by
//! the real bot id, under `_state/`, which the Pages publish never syncs).

use anyhow::{Context, Result};
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;

use pbtb_rust::config::chart::ChartConfig;

use crate::model::{BotReturnSeries, BotState};

fn state_key(bot_id: &str) -> String {
    // Deliberately NOT under cfg.key_prefix so the publish (which syncs only the
    // published prefix) never exposes state — it holds the real bot id.
    format!("_state/{bot_id}.json")
}

/// Read a bot's accumulated state; `Ok(None)` if none stored yet.
pub async fn read_state(
    client: &Client,
    cfg: &ChartConfig,
    bot_id: &str,
) -> Result<Option<BotState>> {
    let key = state_key(bot_id);
    match client
        .get_object()
        .bucket(&cfg.bucket_name)
        .key(&key)
        .send()
        .await
    {
        Ok(o) => {
            let bytes = o
                .body
                .collect()
                .await
                .context("read state body")?
                .into_bytes();
            let state: BotState = serde_json::from_slice(&bytes).context("parse state json")?;
            Ok(Some(state))
        }
        Err(e) => {
            if e.as_service_error().is_some_and(|se| se.is_no_such_key()) {
                Ok(None)
            } else {
                Err(anyhow::Error::new(e).context("get state object"))
            }
        }
    }
}

/// Persist a bot's accumulated state (private).
pub async fn write_state(
    client: &Client,
    cfg: &ChartConfig,
    bot_id: &str,
    state: &BotState,
) -> Result<()> {
    let body = serde_json::to_vec(state).context("serialize state")?;
    client
        .put_object()
        .bucket(&cfg.bucket_name)
        .key(state_key(bot_id))
        .body(ByteStream::from(body))
        .content_type("application/json")
        .send()
        .await
        .context("put state to S3")?;
    Ok(())
}

/// Write the public return series to `<prefix>/<label>.json`. A short cache
/// lifetime keeps the daily-updated curve fresh behind a CDN.
pub async fn put_series(
    client: &Client,
    cfg: &ChartConfig,
    label: &str,
    series: &BotReturnSeries,
) -> Result<()> {
    let key = format!("{}/{}.json", cfg.key_prefix.trim_matches('/'), label);
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
