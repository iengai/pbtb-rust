//! Hyperliquid adapter for the return-curve collector.
//!
//! Everything read here is a public `POST /info` query keyed by the account
//! address: nothing is signed, so the collector never reads the bot's API
//! wallet key. Hyperliquid keeps no transaction log with a running balance the
//! way Bybit does, so the ledger is rebuilt: the settlement-coin movements up
//! to an anchor instant are fetched (each fill's closed PnL net of its fee,
//! funding, and the non-funding ledger's deposits, withdrawals and transfers),
//! the account's cash at that instant is read from its state, and the balance
//! after each movement is walked back from it.
//!
//! What this cannot see, and so assumes the account does not do: trade spot
//! (a spot fill moves USDC without a closed PnL), or hold positions on a HIP-3
//! dex in a non-unified account (that margin sits outside the balances read
//! here). A movement it misses shifts every balance before it by the same
//! amount. A bot's account is meant to be the bot's alone.

use std::collections::HashSet;
use std::hash::Hash;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use pbtb_rust::config::hyperliquid::HyperliquidConfig;

use crate::model::LedgerEntry;

const DAY_MS: i64 = 86_400_000;
const QUOTE: &str = "USDC";
/// Hyperliquid serves an account's most recent 10 000 fills and none older.
const FILL_HISTORY_CAP: usize = 10_000;
/// A pagination that has not caught up by here is a fault, not a long history.
const MAX_PAGES: usize = 200;

/// A settlement-coin movement before its balance is known.
#[derive(Debug, Clone, PartialEq)]
struct Move {
    ts_ms: i64,
    change: f64,
    flow: bool,
}

#[derive(Deserialize)]
struct Fill {
    coin: String,
    time: i64,
    #[serde(rename = "closedPnl")]
    closed_pnl: String,
    fee: String,
    #[serde(rename = "feeToken", default)]
    fee_token: Option<String>,
    tid: u64,
}

/// A `userFunding` or `userNonFundingLedgerUpdates` row.
#[derive(Deserialize)]
struct Update {
    time: i64,
    #[serde(default)]
    hash: String,
    delta: Value,
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// A fee paid in the settlement coin; a fee charged in another token does
/// not move it.
fn quote_fee(fee: &Value, token: Option<&str>) -> f64 {
    match token {
        None | Some("") | Some(QUOTE) => num(fee).unwrap_or(0.0),
        Some(_) => 0.0,
    }
}

/// The account's cash at the anchor: equity less the open positions'
/// unrealized PnL, which is what the ledger moves. In a unified account the
/// spot USDC total is the whole equity, perp margin and PnL included; in a
/// standard one the perp account value is, and idle spot USDC sits beside it.
/// Counting both there makes a spot-perp transfer the internal move it is.
fn anchor_cash(abstraction: &str, perp: &Value, spot: &Value) -> Result<f64> {
    let upnl: f64 = perp["assetPositions"]
        .as_array()
        .ok_or_else(|| anyhow!("clearinghouseState without assetPositions"))?
        .iter()
        .map(|p| {
            num(&p["position"]["unrealizedPnl"])
                .ok_or_else(|| anyhow!("a position without unrealizedPnl"))
        })
        .sum::<Result<f64>>()?;
    let spot_quote = spot["balances"]
        .as_array()
        .ok_or_else(|| anyhow!("spotClearinghouseState without balances"))?
        .iter()
        .find(|b| b["coin"] == QUOTE)
        .map(|b| num(&b["total"]).ok_or_else(|| anyhow!("a {QUOTE} balance without total")))
        .transpose()?
        .unwrap_or(0.0);
    if matches!(abstraction, "unifiedAccount" | "portfolioMargin") {
        Ok(spot_quote - upnl)
    } else {
        let value = num(&perp["marginSummary"]["accountValue"])
            .ok_or_else(|| anyhow!("clearinghouseState without accountValue"))?;
        Ok(value - upnl + spot_quote)
    }
}

/// A perp fill's effect on the settlement coin. `None` for a spot fill,
/// whose coin is a pair (`PURR/USDC`) or an index (`@107`).
fn fill_move(f: &Fill) -> Option<Move> {
    if f.coin.starts_with('@') || f.coin.contains('/') {
        return None;
    }
    let pnl: f64 = f.closed_pnl.parse().unwrap_or(0.0);
    let fee = quote_fee(&Value::String(f.fee.clone()), f.fee_token.as_deref());
    Some(Move {
        ts_ms: f.time,
        change: pnl - fee,
        flow: false,
    })
}

/// A non-funding ledger row's effect on the account's settlement coin, seen
/// from `me` (a lowercase address). Every movement here is money arriving or
/// leaving, never earned. `Ok(None)` for a row that moves none: a transfer
/// between the account's own spot and perp wallets, a token other than the
/// settlement coin, a liquidation (its loss is in the fills). `Err` carries
/// the kind of a row this does not know, for the caller to log.
fn ledger_change(delta: &Value, me: &str) -> Result<Option<f64>, String> {
    let kind = delta["type"].as_str().unwrap_or_default();
    let party = |field: &str| {
        delta[field]
            .as_str()
            .map(|a| a.eq_ignore_ascii_case(me))
            .unwrap_or(false)
    };
    let between = |amount: f64, fee: f64| match (party("user"), party("destination")) {
        (true, true) => -fee,
        (true, false) => -(amount + fee),
        (false, true) => amount,
        (false, false) => 0.0,
    };
    let change = match kind {
        "deposit" => num(&delta["usdc"]).unwrap_or(0.0),
        "withdraw" => -(num(&delta["usdc"]).unwrap_or(0.0) + num(&delta["fee"]).unwrap_or(0.0)),
        "internalTransfer" | "subAccountTransfer" => between(
            num(&delta["usdc"]).unwrap_or(0.0),
            num(&delta["fee"]).unwrap_or(0.0),
        ),
        "send" | "spotTransfer" => {
            if delta["token"] != QUOTE {
                return Ok(None);
            }
            let amount = num(&delta["usdcValue"])
                .or_else(|| num(&delta["amount"]))
                .unwrap_or(0.0);
            between(amount, quote_fee(&delta["fee"], delta["feeToken"].as_str()))
        }
        "vaultDeposit" | "vaultCreate" => -num(&delta["usdc"]).unwrap_or(0.0),
        "vaultWithdraw" => num(&delta["netWithdrawnUsd"]).unwrap_or(0.0),
        "rewardsClaim" => num(&delta["amount"]).unwrap_or(0.0),
        "accountClassTransfer" | "liquidation" => return Ok(None),
        other => return Err(other.to_string()),
    };
    Ok((change != 0.0).then_some(change))
}

/// The movements with the balance after each, walked back from the cash at
/// the anchor, which follows the last of them.
fn rebuild(mut moves: Vec<Move>, cash_at_anchor: f64) -> Vec<LedgerEntry> {
    moves.sort_by_key(|m| m.ts_ms);
    let mut balance = cash_at_anchor;
    let mut entries: Vec<LedgerEntry> = moves
        .iter()
        .rev()
        .map(|m| {
            let entry = LedgerEntry {
                ts_ms: m.ts_ms,
                change: m.change,
                cash_balance: balance,
                flow: m.flow,
            };
            balance -= m.change;
            entry
        })
        .collect();
    entries.reverse();
    entries
}

/// Where a history whose fills hit the service's cap can be trusted from: the
/// first whole day after the earliest fill served. Before it the fills are
/// missing while funding and transfers are not, and a day built from half its
/// movements would be wrong rather than absent.
fn trusted_from(fills: &[Fill]) -> Option<i64> {
    (fills.len() >= FILL_HISTORY_CAP)
        .then(|| fills.iter().map(|f| f.time).min())
        .flatten()
        .map(|earliest| (earliest.div_euclid(DAY_MS) + 1) * DAY_MS)
}

async fn info<T: DeserializeOwned>(
    http: &reqwest::Client,
    cfg: &HyperliquidConfig,
    body: Value,
) -> Result<T> {
    let url = format!("{}/info", cfg.base_url.trim_end_matches('/'));
    let resp = http
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("hyperliquid request failed")?;
    let status = resp.status();
    if !status.is_success() {
        bail!("hyperliquid {} HTTP {status}", body["type"]);
    }
    resp.json::<T>()
        .await
        .with_context(|| format!("parse hyperliquid {}", body["type"]))
}

/// Every row of a time-ranged query in `[from_ms, to_ms]`. A response holds
/// the earliest rows of the range up to a page size, so the next page starts
/// at the latest time seen; rows at that instant come back again and are told
/// apart by `key_of`. The walk ends on a page with nothing new.
async fn paged<T, K>(
    http: &reqwest::Client,
    cfg: &HyperliquidConfig,
    kind: &str,
    address: &str,
    (from_ms, to_ms): (i64, i64),
    time_of: impl Fn(&T) -> i64,
    key_of: impl Fn(&T) -> K,
) -> Result<Vec<T>>
where
    T: DeserializeOwned,
    K: Eq + Hash,
{
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = from_ms;
    for _ in 0..MAX_PAGES {
        let page: Vec<T> = info(
            http,
            cfg,
            json!({ "type": kind, "user": address, "startTime": cursor, "endTime": to_ms }),
        )
        .await?;
        let mut fresh = false;
        for row in page {
            cursor = cursor.max(time_of(&row));
            if seen.insert(key_of(&row)) {
                rows.push(row);
                fresh = true;
            }
        }
        if !fresh {
            return Ok(rows);
        }
    }
    bail!("hyperliquid {kind}: still paging after {MAX_PAGES} pages")
}

/// The account's settlement-coin ledger from `from_ms` to now, each entry with
/// the balance after it. `address` is the account the bot trades for.
pub async fn fetch_ledger(
    http: &reqwest::Client,
    cfg: &HyperliquidConfig,
    address: &str,
    from_ms: i64,
) -> Result<Vec<LedgerEntry>> {
    let me = address.to_ascii_lowercase();
    let user = |kind: &str| json!({ "type": kind, "user": me });

    let abstraction: Value = info(http, cfg, user("userAbstraction")).await?;
    let spot: Value = info(http, cfg, user("spotClearinghouseState")).await?;
    let perp: Value = info(http, cfg, user("clearinghouseState")).await?;
    let anchor_ms = perp["time"]
        .as_i64()
        .ok_or_else(|| anyhow!("clearinghouseState without time"))?;
    let cash = anchor_cash(abstraction.as_str().unwrap_or_default(), &perp, &spot)?;
    let range = (from_ms, anchor_ms);

    let fills: Vec<Fill> = paged(
        http,
        cfg,
        "userFillsByTime",
        &me,
        range,
        |f: &Fill| f.time,
        |f| f.tid,
    )
    .await?;
    let funding: Vec<Update> = paged(
        http,
        cfg,
        "userFunding",
        &me,
        range,
        |u: &Update| u.time,
        |u| {
            (
                u.time,
                u.delta["coin"].as_str().unwrap_or_default().to_string(),
            )
        },
    )
    .await?;
    let ledger: Vec<Update> = paged(
        http,
        cfg,
        "userNonFundingLedgerUpdates",
        &me,
        range,
        |u: &Update| u.time,
        |u| (u.time, u.hash.clone(), u.delta["type"].to_string()),
    )
    .await?;

    let mut moves: Vec<Move> = fills.iter().filter_map(fill_move).collect();
    moves.extend(funding.iter().filter_map(|u| {
        num(&u.delta["usdc"]).map(|change| Move {
            ts_ms: u.time,
            change,
            flow: false,
        })
    }));
    let mut unknown = HashSet::new();
    for u in &ledger {
        match ledger_change(&u.delta, &me) {
            Ok(Some(change)) => moves.push(Move {
                ts_ms: u.time,
                change,
                flow: true,
            }),
            Ok(None) => {}
            Err(kind) => {
                unknown.insert(kind);
            }
        }
    }
    if !unknown.is_empty() {
        tracing::warn!(kinds = ?unknown, "hyperliquid ledger rows of an unknown kind counted as zero");
    }

    let mut entries = rebuild(moves, cash);
    if let Some(start) = trusted_from(&fills) {
        tracing::warn!(
            from_ms = start,
            "hyperliquid serves no older fills; the history starts at the first whole day it covers"
        );
        entries.retain(|e| e.ts_ms >= start);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER: &str = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn perp(value: &str, upnls: &[&str]) -> Value {
        json!({
            "marginSummary": { "accountValue": value },
            "assetPositions": upnls
                .iter()
                .map(|u| json!({ "position": { "unrealizedPnl": u } }))
                .collect::<Vec<_>>(),
            "time": 1,
        })
    }

    fn spot(usdc: &str) -> Value {
        json!({ "balances": [
            { "coin": "HYPE", "total": "9.0" },
            { "coin": "USDC", "total": usdc },
        ] })
    }

    #[test]
    fn a_unified_account_s_cash_is_its_spot_usdc_less_open_pnl() {
        let cash = anchor_cash(
            "unifiedAccount",
            &perp("2641.17", &["2.61"]),
            &spot("2647.40"),
        )
        .unwrap();
        assert!((cash - 2644.79).abs() < 1e-9, "{cash}");
    }

    #[test]
    fn a_standard_account_s_cash_is_perp_value_less_open_pnl_plus_idle_spot() {
        let cash = anchor_cash("default", &perp("1000", &["-20", "5"]), &spot("50")).unwrap();
        assert!((cash - 1065.0).abs() < 1e-9, "{cash}");
        let no_spot = json!({ "balances": [] });
        let cash = anchor_cash("disabled", &perp("1000", &[]), &no_spot).unwrap();
        assert!((cash - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn a_state_missing_a_figure_is_an_error_not_a_zero() {
        let broken = json!({ "assetPositions": [], "time": 1 });
        assert!(anchor_cash("default", &broken, &spot("1")).is_err());
        let no_upnl = json!({ "assetPositions": [{ "position": {} }], "time": 1 });
        assert!(anchor_cash("unifiedAccount", &no_upnl, &spot("1")).is_err());
    }

    fn fill(coin: &str, pnl: &str, fee: &str, token: Option<&str>) -> Fill {
        Fill {
            coin: coin.into(),
            time: 5,
            closed_pnl: pnl.into(),
            fee: fee.into(),
            fee_token: token.map(String::from),
            tid: 1,
        }
    }

    #[test]
    fn a_perp_fill_moves_its_closed_pnl_net_of_fee_and_a_spot_fill_is_skipped() {
        let m = fill_move(&fill("XRP", "13.4557", "3.378863", Some("USDC"))).unwrap();
        assert!((m.change - 10.076837).abs() < 1e-9);
        assert!(!m.flow);
        // A maker rebate is a negative fee.
        let m = fill_move(&fill("xyz:MU", "0.0", "-0.001948", Some("USDC"))).unwrap();
        assert!((m.change - 0.001948).abs() < 1e-12);
        assert!(fill_move(&fill("@107", "0", "0.1", Some("USDC"))).is_none());
        assert!(fill_move(&fill("PURR/USDC", "0", "0.1", Some("USDC"))).is_none());
        let m = fill_move(&fill("BTC", "1.0", "0.5", Some("HYPE"))).unwrap();
        assert_eq!(m.change, 1.0, "a fee in another token does not move USDC");
    }

    fn change(delta: Value) -> Option<f64> {
        ledger_change(&delta, ME).unwrap()
    }

    #[test]
    fn ledger_rows_move_money_in_and_out() {
        assert_eq!(
            change(json!({"type": "deposit", "usdc": "100"})),
            Some(100.0)
        );
        assert_eq!(
            change(json!({"type": "withdraw", "usdc": "40", "fee": "1"})),
            Some(-41.0)
        );
        assert_eq!(
            change(
                json!({"type": "internalTransfer", "usdc": "5", "user": OTHER, "destination": ME, "fee": "0"})
            ),
            Some(5.0)
        );
        assert_eq!(
            change(
                json!({"type": "subAccountTransfer", "usdc": "7", "user": ME, "destination": OTHER})
            ),
            Some(-7.0)
        );
        assert_eq!(
            change(
                json!({"type": "send", "user": ME, "destination": OTHER, "token": "USDC",
                          "amount": "3000.0", "usdcValue": "3000.0", "fee": "1.0", "feeToken": "USDC"})
            ),
            Some(-3001.0)
        );
        assert_eq!(
            change(json!({"type": "vaultDeposit", "vault": OTHER, "usdc": "1600"})),
            Some(-1600.0)
        );
        assert_eq!(
            change(
                json!({"type": "vaultWithdraw", "vault": OTHER, "user": ME, "netWithdrawnUsd": "54.5"})
            ),
            Some(54.5)
        );
    }

    #[test]
    fn moves_inside_the_account_and_other_tokens_move_nothing() {
        // Spot to perp within the account, as a self-send and as the older kind.
        let self_send = json!({"type": "send", "user": ME, "destination": ME, "sourceDex": "spot",
                               "destinationDex": "", "token": "USDC", "amount": "1.0",
                               "usdcValue": "1.0", "fee": "0.0", "feeToken": ""});
        assert_eq!(change(self_send), None);
        assert_eq!(
            change(json!({"type": "accountClassTransfer", "usdc": "9", "toPerp": true})),
            None
        );
        assert_eq!(
            change(
                json!({"type": "spotTransfer", "token": "HYPE", "amount": "3", "user": OTHER, "destination": ME})
            ),
            None
        );
        assert_eq!(
            change(json!({"type": "liquidation", "accountValue": "1"})),
            None
        );
        assert_eq!(
            ledger_change(&json!({"type": "somethingNew"}), ME),
            Err("somethingNew".to_string())
        );
    }

    #[test]
    fn balances_are_walked_back_from_the_anchor() {
        let moves = vec![
            Move {
                ts_ms: 30,
                change: -5.0,
                flow: false,
            },
            Move {
                ts_ms: 10,
                change: 100.0,
                flow: true,
            },
            Move {
                ts_ms: 20,
                change: 10.0,
                flow: false,
            },
        ];
        let entries = rebuild(moves, 1105.0);
        let got: Vec<_> = entries.iter().map(|e| (e.ts_ms, e.cash_balance)).collect();
        assert_eq!(got, vec![(10, 1100.0), (20, 1110.0), (30, 1105.0)]);
        assert!(entries[0].flow && !entries[1].flow);
    }

    #[test]
    fn a_capped_fill_history_is_trusted_from_the_next_whole_day() {
        let mut fills: Vec<Fill> = (0..FILL_HISTORY_CAP as u64)
            .map(|i| Fill {
                coin: "XRP".into(),
                time: 3 * DAY_MS + 5 + i as i64,
                closed_pnl: "0".into(),
                fee: "0".into(),
                fee_token: None,
                tid: i,
            })
            .collect();
        assert_eq!(trusted_from(&fills), Some(4 * DAY_MS));
        fills.pop();
        assert_eq!(
            trusted_from(&fills),
            None,
            "below the cap the history is whole"
        );
    }
}
