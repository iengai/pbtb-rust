//! Minimal signed Bybit V5 REST client for the return-curve collector.
//!
//! Only the two read endpoints the collector needs are implemented. Requests are
//! signed with `HMAC-SHA256(secret, timestamp + api_key + recv_window + query)`
//! per Bybit V5. The key/secret are never logged: errors carry only the HTTP
//! status or Bybit's own `retMsg`, never request headers (which hold the key).

use anyhow::{Context, Result, anyhow, bail};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

use pbtb_rust::config::bybit::BybitConfig;

type HmacSha256 = Hmac<Sha256>;

const DAY_MS: i64 = 86_400_000;
const WINDOW_MS: i64 = 7 * DAY_MS; // Bybit caps each history query at 7 days.

/// One normalized transaction-log entry. `change = cashFlow + funding - fee`
/// (the net cash effect); `cash_balance` is the wallet balance after it.
pub struct LedgerEntry {
    pub ts_ms: i64,
    pub kind: String,
    pub change: f64,
    pub cash_balance: f64,
}

/// Bybit occasionally sends `null` for an otherwise-string field (e.g. an empty
/// `nextPageCursor`); map both `null` and an absent field to an empty string.
fn de_null_string<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

#[derive(Deserialize)]
struct Envelope<T> {
    #[serde(rename = "retCode")]
    ret_code: i64,
    #[serde(rename = "retMsg", default, deserialize_with = "de_null_string")]
    ret_msg: String,
    result: Option<T>,
}

fn sign(secret: &str, payload: &str) -> Result<String> {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).map_err(|e| anyhow!("hmac init: {e}"))?;
    mac.update(payload.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

fn now_ms() -> Result<i64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64)
}

/// Issue a signed GET and unwrap the Bybit envelope. `query` is the exact query
/// string (without leading `?`) that is both signed and sent, so any pagination
/// cursor must already be in the form Bybit returned it.
async fn signed_get<T: for<'de> Deserialize<'de>>(
    http: &reqwest::Client,
    cfg: &BybitConfig,
    api_key: &str,
    secret: &str,
    path: &str,
    query: &str,
) -> Result<T> {
    let ts = now_ms()?.to_string();
    let recv = cfg.recv_window_ms.unwrap_or(5000).to_string();
    let signature = sign(secret, &format!("{ts}{api_key}{recv}{query}"))?;
    let url = format!("{}{}?{}", cfg.base_url.trim_end_matches('/'), path, query);

    let resp = http
        .get(&url)
        .header("X-BAPI-API-KEY", api_key)
        .header("X-BAPI-TIMESTAMP", &ts)
        .header("X-BAPI-RECV-WINDOW", &recv)
        .header("X-BAPI-SIGN", signature)
        .send()
        .await
        .context("bybit request failed")?;

    let status = resp.status();
    let body = resp.text().await.context("read bybit body")?;
    if !status.is_success() {
        // Status only — the body may echo account data; the key is in a header.
        bail!("bybit HTTP {status}");
    }
    let env: Envelope<T> = serde_json::from_str(&body).context("parse bybit envelope")?;
    if env.ret_code != 0 {
        bail!("bybit retCode {}: {}", env.ret_code, env.ret_msg);
    }
    env.result
        .ok_or_else(|| anyhow!("bybit response missing result"))
}

#[derive(Deserialize)]
struct TxnResult {
    #[serde(default)]
    list: Vec<TxnRow>,
    #[serde(
        rename = "nextPageCursor",
        default,
        deserialize_with = "de_null_string"
    )]
    next_page_cursor: String,
}

#[derive(Deserialize)]
struct TxnRow {
    #[serde(
        rename = "transactionTime",
        default,
        deserialize_with = "de_null_string"
    )]
    transaction_time: String,
    #[serde(rename = "type", default, deserialize_with = "de_null_string")]
    r#type: String,
    #[serde(default, deserialize_with = "de_null_string")]
    change: String,
    #[serde(rename = "cashBalance", default, deserialize_with = "de_null_string")]
    cash_balance: String,
    #[serde(default, deserialize_with = "de_null_string")]
    currency: String,
}

/// Collect settlement-coin transaction-log entries in `[from_ms, to_ms]`, in
/// 7-day windows (Bybit's per-request cap) with cursor pagination. The caller
/// chooses the window — a small incremental slice on routine runs, a wider one
/// only on a bot's first run — so a long-running deployment relies on its own
/// accumulated state rather than re-fetching everything each day.
pub async fn fetch_transaction_log(
    http: &reqwest::Client,
    cfg: &BybitConfig,
    api_key: &str,
    secret: &str,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<LedgerEntry>> {
    let mut entries = Vec::new();
    let mut win_end = to_ms;

    while win_end > from_ms {
        let win_start = (win_end - WINDOW_MS).max(from_ms);
        let mut cursor = String::new();
        loop {
            let mut query = format!(
                "accountType=UNIFIED&category=linear&currency={}&limit=50&startTime={}&endTime={}",
                cfg.settle_coin, win_start, win_end
            );
            if !cursor.is_empty() {
                // Bybit returns the cursor already URL-encoded, so it is signed
                // and sent verbatim (re-encoding would break the signature).
                query.push_str(&format!("&cursor={cursor}"));
            }

            let res: TxnResult = signed_get(
                http,
                cfg,
                api_key,
                secret,
                "/v5/account/transaction-log",
                &query,
            )
            .await?;

            for row in &res.list {
                if !row.currency.is_empty() && row.currency != cfg.settle_coin {
                    continue;
                }
                entries.push(LedgerEntry {
                    ts_ms: row.transaction_time.parse::<i64>().unwrap_or(0),
                    kind: row.r#type.clone(),
                    change: row.change.parse::<f64>().unwrap_or(0.0),
                    cash_balance: row.cash_balance.parse::<f64>().unwrap_or(0.0),
                });
            }

            if res.next_page_cursor.is_empty() {
                break;
            }
            cursor = res.next_page_cursor;
        }
        win_end = win_start;
    }

    Ok(entries)
}
