//! Hyperliquid's public `candleSnapshot` query: no credential, no fixed IP.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use pbtb_rust::config::hyperliquid::HyperliquidConfig;

use crate::days::DAY_MS;

/// The 1m candles of `coin` on UTC `day`, as Hyperliquid serves them
/// (`t`, `T`, `s`, `i`, `o`, `c`, `h`, `l`, `v`, `n`), oldest first. A minute
/// without trades may be absent; the lab fills it when it writes shards.
pub async fn day_candles(
    http: &reqwest::Client,
    cfg: &HyperliquidConfig,
    coin: &str,
    day: i64,
) -> Result<Vec<Value>> {
    let start = day * DAY_MS;
    let end = start + DAY_MS;
    let url = format!("{}/info", cfg.base_url.trim_end_matches('/'));
    let body = json!({
        "type": "candleSnapshot",
        "req": { "coin": coin, "interval": "1m", "startTime": start, "endTime": end - 1 },
    });
    let resp = http
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("hyperliquid candleSnapshot request failed")?;
    let status = resp.status();
    if !status.is_success() {
        bail!("hyperliquid candleSnapshot {coin} HTTP {status}");
    }
    let rows: Vec<Value> = resp
        .json()
        .await
        .with_context(|| format!("parse hyperliquid candleSnapshot {coin}"))?;
    Ok(within(rows, start, end))
}

/// The rows opening in `[start, end)`, oldest first.
fn within(rows: Vec<Value>, start: i64, end: i64) -> Vec<Value> {
    let mut rows: Vec<Value> = rows
        .into_iter()
        .filter(|r| {
            r.get("t")
                .and_then(Value::as_i64)
                .is_some_and(|t| (start..end).contains(&t))
        })
        .collect();
    rows.sort_by_key(|r| r.get("t").and_then(Value::as_i64));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_day_s_rows_are_kept_oldest_first() {
        let rows = vec![
            json!({"t": DAY_MS + 60_000}),
            json!({"t": DAY_MS - 60_000}),
            json!({"t": DAY_MS}),
            json!({"t": 2 * DAY_MS}),
            json!({"x": 1}),
        ];
        let kept = within(rows, DAY_MS, 2 * DAY_MS);
        assert_eq!(
            kept,
            vec![json!({"t": DAY_MS}), json!({"t": DAY_MS + 60_000})]
        );
    }
}
