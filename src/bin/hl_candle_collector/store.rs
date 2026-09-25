//! The candle bucket: one object per coin and UTC day, written once.

use anyhow::{Context, Result};
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use serde_json::Value;

use crate::config::CandlesConfig;
use crate::days::ymd;

pub fn day_key(prefix: &str, coin: &str, day: i64) -> String {
    format!("{}/{coin}/{}.json", prefix.trim_end_matches('/'), ymd(day))
}

pub struct CandleStore {
    client: Client,
    bucket: String,
    prefix: String,
}

impl CandleStore {
    pub fn new(client: Client, cfg: &CandlesConfig) -> Self {
        Self {
            client,
            bucket: cfg.bucket_name.clone(),
            prefix: cfg.key_prefix.clone(),
        }
    }

    /// Whether the day is stored. Needs `s3:ListBucket`, or an absent key
    /// reads as 403 rather than 404.
    pub async fn has_day(&self, coin: &str, day: i64) -> Result<bool> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(day_key(&self.prefix, coin, day))
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) if e.as_service_error().is_some_and(|se| se.is_not_found()) => Ok(false),
            Err(e) => Err(anyhow::Error::new(e).context("head candle object")),
        }
    }

    /// Whether any day of `coin` is stored.
    pub async fn has_any(&self, coin: &str) -> Result<bool> {
        let out = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(format!("{}/{coin}/", self.prefix.trim_end_matches('/')))
            .max_keys(1)
            .send()
            .await
            .context("list candle objects")?;
        Ok(out.key_count().unwrap_or(0) > 0)
    }

    pub async fn put_day(&self, coin: &str, day: i64, rows: &[Value]) -> Result<()> {
        let body = serde_json::to_vec(rows).context("serialize candles")?;
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(day_key(&self.prefix, coin, day))
            .body(ByteStream::from(body))
            .content_type("application/json")
            .send()
            .await
            .context("put candle object")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_is_keyed_by_coin_and_date() {
        assert_eq!(
            day_key("hyperliquid/1m/", "BTC", 20_720),
            "hyperliquid/1m/BTC/2026-09-24.json"
        );
    }
}
