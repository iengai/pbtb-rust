//! Exchange-neutral output types + the transaction-log → daily-return transform,
//! split so the return index can be recomputed incrementally from accumulated
//! state without re-fetching history.
//!
//! The published artifact carries only NORMALIZED performance (a time-weighted
//! return index and cumulative return %) keyed by an ANONYMIZED label — never an
//! absolute balance/equity, never the real bot id. Nothing here is Bybit-specific.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use pbtb_rust::domain::configswitch::ConfigSwitchEvent;

use crate::bybit::LedgerEntry;

const DAY_S: i64 = 86_400;
const DAY_MS: i64 = 86_400_000;

/// A stable, non-identifying public label for a bot: the first 6 hex of
/// SHA-256(bot_id). Deterministic, so it never renumbers, and reveals nothing
/// about the real id.
pub fn anon_label(bot_id: &str) -> String {
    format!(
        "bot-{}",
        hex::encode(&Sha256::digest(bot_id.as_bytes())[..3])
    )
}

/// One day's realized aggregate — the minimal state to (re)compute the return
/// index. Persisted privately so each run fetches only new data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayAgg {
    /// Days since epoch (UTC).
    pub day: i64,
    /// Realized PnL that day (net of fees/funding; excludes deposits).
    pub realized: f64,
    /// Wallet balance at day close.
    pub end_balance: f64,
}

/// Accumulated per-bot collector state, stored privately (never published).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BotState {
    /// Balance just before the earliest day's first entry — seeds the index.
    #[serde(default)]
    pub first_pre_balance: f64,
    /// Daily aggregates, ascending by `day`.
    #[serde(default)]
    pub days: Vec<DayAgg>,
}

impl BotState {
    /// Merge freshly-aggregated days into the state, overwriting any overlapping
    /// day (a re-fetched partial day is authoritative) and extending with new
    /// ones. Seeds `first_pre_balance` from the new data only when empty.
    pub fn merge(&mut self, new_days: Vec<DayAgg>, new_first_pre: Option<f64>) {
        if self.days.is_empty() {
            if let Some(fp) = new_first_pre {
                self.first_pre_balance = fp;
            }
        }
        let mut map: BTreeMap<i64, DayAgg> = self.days.drain(..).map(|d| (d.day, d)).collect();
        for d in new_days {
            map.insert(d.day, d);
        }
        self.days = map.into_values().collect();
    }
}

/// One point per UTC day. Normalized only.
#[derive(Debug, Clone, Serialize)]
pub struct DailyPoint {
    pub ts: i64,
    pub index: f64,
    pub return_pct: f64,
}

/// A marker the chart draws to show when the bot switched config.
#[derive(Debug, Clone, Serialize)]
pub struct SwitchMarker {
    pub ts: i64,
    pub template_name: String,
}

/// The per-bot artifact the static site fetches and draws. `bot` is the
/// anonymized label, not the real id.
#[derive(Debug, Clone, Serialize)]
pub struct BotReturnSeries {
    pub bot: String,
    pub exchange: String,
    pub generated_at: i64,
    pub current_return_pct: f64,
    pub points: Vec<DailyPoint>,
    pub config_switches: Vec<SwitchMarker>,
}

impl BotReturnSeries {
    pub fn new(
        label: &str,
        exchange: &str,
        points: Vec<DailyPoint>,
        switches: &[ConfigSwitchEvent],
        generated_at: i64,
    ) -> Self {
        let current_return_pct = points.last().map(|p| p.return_pct).unwrap_or(0.0);
        let config_switches = switches
            .iter()
            .map(|s| SwitchMarker {
                ts: s.applied_at,
                template_name: s.template_name.clone(),
            })
            .collect();
        Self {
            bot: label.to_string(),
            exchange: exchange.to_string(),
            generated_at,
            current_return_pct,
            points,
            config_switches,
        }
    }
}

fn is_capital_flow(kind: &str) -> bool {
    matches!(kind, "TRANSFER_IN" | "TRANSFER_OUT")
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

/// Aggregate a ledger slice into per-day realized/end-balance rows, plus the
/// pre-balance of the earliest day in the slice (used to seed the index on the
/// first run). Entries with a non-positive timestamp are dropped.
pub fn aggregate(ledger: &[LedgerEntry]) -> (Vec<DayAgg>, Option<f64>) {
    struct D {
        realized: f64,
        end_balance: f64,
        last_ts: i64,
        pre_balance: f64,
        first_ts: i64,
    }

    let mut days: BTreeMap<i64, D> = BTreeMap::new();
    for e in ledger {
        if e.ts_ms <= 0 {
            continue;
        }
        let day = e.ts_ms / DAY_MS;
        let d = days.entry(day).or_insert(D {
            realized: 0.0,
            end_balance: 0.0,
            last_ts: i64::MIN,
            pre_balance: 0.0,
            first_ts: i64::MAX,
        });
        if !is_capital_flow(&e.kind) {
            d.realized += e.change;
        }
        if e.ts_ms >= d.last_ts {
            d.last_ts = e.ts_ms;
            d.end_balance = e.cash_balance;
        }
        if e.ts_ms <= d.first_ts {
            d.first_ts = e.ts_ms;
            d.pre_balance = e.cash_balance - e.change;
        }
    }

    let first_pre = days.values().next().map(|d| d.pre_balance);
    let aggs = days
        .into_iter()
        .map(|(day, d)| DayAgg {
            day,
            realized: d.realized,
            end_balance: d.end_balance,
        })
        .collect();
    (aggs, first_pre)
}

/// Compute the time-weighted return index over ordered daily aggregates. Each
/// day's return is realized PnL over the balance at the start of the day (the
/// previous day's close, or `first_pre_balance` on day 0), compounded. A
/// non-positive start balance yields a flat day (no div-by-zero / blow-up).
pub fn compute_points(days: &[DayAgg], first_pre_balance: f64) -> Vec<DailyPoint> {
    let mut prev_end: Option<f64> = None;
    let mut idx = 100.0_f64;
    let mut out = Vec::with_capacity(days.len());
    for d in days {
        let start = prev_end.unwrap_or(first_pre_balance);
        let dr = if start > 0.0 { d.realized / start } else { 0.0 };
        idx *= 1.0 + dr;
        out.push(DailyPoint {
            ts: d.day * DAY_S,
            index: round4(idx),
            return_pct: round4(idx - 100.0),
        });
        prev_end = Some(d.end_balance);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(ts_ms: i64, kind: &str, change: f64, bal: f64) -> LedgerEntry {
        LedgerEntry {
            ts_ms,
            kind: kind.to_string(),
            change,
            cash_balance: bal,
        }
    }

    #[test]
    fn twr_index_ignores_deposits_and_compounds_daily() {
        let d0 = 0;
        let d1 = DAY_MS;
        let ledger = vec![
            entry(d0 + 1000, "TRADE", 10.0, 1010.0),
            entry(d0 + 2000, "SETTLEMENT", -5.0, 1005.0),
            entry(d1 + 1000, "TRANSFER_IN", 500.0, 1505.0),
            entry(d1 + 2000, "TRADE", 20.0, 1525.0),
        ];
        let (aggs, first_pre) = aggregate(&ledger);
        assert_eq!(first_pre, Some(1000.0));
        let pts = compute_points(&aggs, first_pre.unwrap());
        assert_eq!(pts.len(), 2);
        assert!((pts[0].return_pct - 0.5).abs() < 1e-6); // 5/1000
        assert!((pts[1].index - 102.5).abs() < 1e-6); // 100.5 * (1 + 20/1005)
    }

    #[test]
    fn incremental_merge_matches_one_shot() {
        // Full ledger over 3 days.
        let mk = |base: i64, r: f64, bal: f64| entry(base + 1000, "TRADE", r, bal);
        let full = vec![
            mk(0, 10.0, 1010.0),
            mk(DAY_MS, 20.0, 1030.0),
            mk(2 * DAY_MS, -15.0, 1015.0),
        ];
        let (full_aggs, full_pre) = aggregate(&full);
        let one_shot = compute_points(&full_aggs, full_pre.unwrap());

        // Incremental: first two days, then merge day 2 + 3 (re-fetching day 1).
        let (a1, p1) = aggregate(&full[..2]);
        let mut state = BotState::default();
        state.merge(a1, p1);
        let (a2, p2) = aggregate(&full[1..]); // overlaps day 1
        state.merge(a2, p2);
        let incremental = compute_points(&state.days, state.first_pre_balance);

        assert_eq!(incremental.len(), one_shot.len());
        for (a, b) in incremental.iter().zip(one_shot.iter()) {
            assert_eq!(a.ts, b.ts);
            assert!((a.index - b.index).abs() < 1e-9, "index mismatch");
        }
    }

    #[test]
    fn anon_label_is_stable_and_hides_id() {
        let a = anon_label("516903813");
        assert_eq!(a, anon_label("516903813"));
        assert!(a.starts_with("bot-"));
        assert_eq!(a.len(), 10); // "bot-" + 6 hex
        assert!(!a.contains("516903813"));
    }

    #[test]
    fn empty_and_nonpositive() {
        let (aggs, fp) = aggregate(&[]);
        assert!(aggs.is_empty());
        assert_eq!(fp, None);
        let (aggs, _) = aggregate(&[entry(0, "TRADE", 1.0, 1.0)]);
        assert!(aggs.is_empty());
    }
}
