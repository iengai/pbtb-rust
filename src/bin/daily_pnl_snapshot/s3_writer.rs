//! S3 IO for the collector. The private halves are keyed by tenant and bot:
//! the series the API serves to its owner (under the configured prefix, at
//! the key `infra::returncurverepository::series_key` names) and the
//! accumulated ledger state under `_state/`, which only the collector reads.
//! The public prefix holds the showcase artifacts, keyed by opaque public id,
//! which the pages-publish workflow copies to the site.

use anyhow::{Context, Result};
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use serde::Serialize;

use pbtb_rust::config::chart::ChartConfig;
use pbtb_rust::infra::returncurverepository::series_key;

use crate::model::{BotReturnSeries, BotState};

fn state_key(user_id: &str, bot_id: &str) -> String {
    format!("_state/{user_id}/{bot_id}.json")
}

/// Where state lived while the bucket held one tenant: keyed by bot id alone.
/// Read as a fallback so an existing bot resumes from its history instead of
/// re-fetching the whole backfill window; the next write lands under the
/// tenant key and the old object is never read again.
fn legacy_state_key(bot_id: &str) -> String {
    format!("_state/{bot_id}.json")
}

async fn get_state(client: &Client, bucket: &str, key: &str) -> Result<Option<BotState>> {
    match client.get_object().bucket(bucket).key(key).send().await {
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

/// Read a bot's accumulated state; `Ok(None)` if none stored yet.
pub async fn read_state(
    client: &Client,
    cfg: &ChartConfig,
    user_id: &str,
    bot_id: &str,
) -> Result<Option<BotState>> {
    if let Some(state) = get_state(client, &cfg.bucket_name, &state_key(user_id, bot_id)).await? {
        return Ok(Some(state));
    }
    get_state(client, &cfg.bucket_name, &legacy_state_key(bot_id)).await
}

/// Persist a bot's accumulated state.
pub async fn write_state(
    client: &Client,
    cfg: &ChartConfig,
    user_id: &str,
    bot_id: &str,
    state: &BotState,
) -> Result<()> {
    let body = serde_json::to_vec(state).context("serialize state")?;
    client
        .put_object()
        .bucket(&cfg.bucket_name)
        .key(state_key(user_id, bot_id))
        .body(ByteStream::from(body))
        .content_type("application/json")
        .send()
        .await
        .context("put state to S3")?;
    Ok(())
}

/// Write the return series where the API will read it for this bot's owner.
pub async fn put_series(
    client: &Client,
    cfg: &ChartConfig,
    user_id: &str,
    bot_id: &str,
    series: &BotReturnSeries,
) -> Result<()> {
    let key = series_key(&cfg.key_prefix, user_id, bot_id);
    let body = serde_json::to_vec(series).context("serialize return series")?;
    client
        .put_object()
        .bucket(&cfg.bucket_name)
        .key(&key)
        .body(ByteStream::from(body))
        .content_type("application/json")
        .send()
        .await
        .context("put chart json to S3")?;
    Ok(())
}

fn public_key(cfg: &ChartConfig, name: &str) -> String {
    format!("{}/{name}", cfg.public_prefix.trim_matches('/'))
}

/// Write one showcase artifact under the public prefix.
pub async fn put_public<T: Serialize>(
    client: &Client,
    cfg: &ChartConfig,
    name: &str,
    value: &T,
) -> Result<()> {
    let body = serde_json::to_vec(value).context("serialize public artifact")?;
    client
        .put_object()
        .bucket(&cfg.bucket_name)
        .key(public_key(cfg, name))
        .body(ByteStream::from(body))
        .content_type("application/json")
        .send()
        .await
        .context("put public artifact to S3")?;
    Ok(())
}

/// The names under `<public prefix>/<sub>`, relative to the prefix, every page
/// of the listing.
pub async fn list_public(client: &Client, cfg: &ChartConfig, sub: &str) -> Result<Vec<String>> {
    let prefix = public_key(cfg, sub);
    let strip = format!("{}/", cfg.public_prefix.trim_matches('/'));
    let mut names = Vec::new();
    let mut pages = client
        .list_objects_v2()
        .bucket(&cfg.bucket_name)
        .prefix(&prefix)
        .into_paginator()
        .send();
    while let Some(page) = pages.next().await {
        let page = page.context("list public prefix")?;
        for object in page.contents() {
            if let Some(key) = object.key()
                && let Some(name) = key.strip_prefix(&strip)
            {
                names.push(name.to_string());
            }
        }
    }
    Ok(names)
}

/// Remove one showcase artifact.
pub async fn delete_public(client: &Client, cfg: &ChartConfig, name: &str) -> Result<()> {
    client
        .delete_object()
        .bucket(&cfg.bucket_name)
        .key(public_key(cfg, name))
        .send()
        .await
        .context("delete public artifact")?;
    Ok(())
}
