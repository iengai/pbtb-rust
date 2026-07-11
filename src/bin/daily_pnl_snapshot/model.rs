//! Exchange-neutral output types + the transaction-log → daily-return transform.
//!
//! The published artifact carries only NORMALIZED performance — a time-weighted
//! return index that starts at 100, plus cumulative return % — never absolute
//! balances or equity, so publishing it never leaks account size. Nothing here
//! is Bybit-specific: another exchange adapter produces the same `LedgerEntry`
//! slice and the same `BotReturnSeries`. Flat DTO layer, no domain modelling.

use std::collections::BTreeMap;

use serde::Serialize;

use pbtb_rust::domain::Bot;
use pbtb_rust::domain::configswitch::ConfigSwitchEvent;

use crate::bybit::LedgerEntry;

const DAY_S: i64 = 86_400;
const DAY_MS: i64 = 86_400_000;

/// One point per UTC day. Normalized only — no absolute amounts.
#[derive(Debug, Clone, Serialize)]
pub struct DailyPoint {
    /// UTC midnight of the day, Unix seconds.
    pub ts: i64,
    /// Time-weighted performance index: starts at 100 and compounds by each
    /// day's realized return. Deposits/withdrawals do not move it, so it tracks
    /// trading performance independent of capital changes.
    pub index: f64,
    /// Cumulative return since inception, percent (`index - 100`).
    pub return_pct: f64,
}

/// A marker the chart draws to show when the bot switched config.
#[derive(Debug, Clone, Serialize)]
pub struct SwitchMarker {
    pub ts: i64,
    pub template_name: String,
}

/// The per-bot artifact the static site fetches and draws.
#[derive(Debug, Clone, Serialize)]
pub struct BotReturnSeries {
    pub bot_id: String,
    pub exchange: String,
    /// When this artifact was generated, Unix seconds.
    pub generated_at: i64,
    /// Cumulative return since inception, percent (the last point's `return_pct`).
    pub current_return_pct: f64,
    pub points: Vec<DailyPoint>,
    pub config_switches: Vec<SwitchMarker>,
}

impl BotReturnSeries {
    pub fn new(
        bot: &Bot,
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
            bot_id: bot.id.clone(),
            exchange: bot.exchange.as_str().to_string(),
            generated_at,
            current_return_pct,
            points,
            config_switches,
        }
    }
}

/// Deposits/withdrawals move capital without being profit, so they are excluded
/// from realized PnL. Everything else (trades, settlement/funding, delivery,
/// bonus, interest) nets into realized PnL.
fn is_capital_flow(kind: &str) -> bool {
    matches!(kind, "TRANSFER_IN" | "TRANSFER_OUT")
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

/// Aggregate raw ledger entries into a daily time-weighted return index. Each
/// day's return is that day's realized PnL over the balance at the start of the
/// day (the previous day's close, or the pre-first-entry balance on day 0); the
/// index compounds these. A non-positive start balance yields a flat day, so a
/// just-funded account never divides by zero or explodes.
pub fn daily_points(ledger: &[LedgerEntry]) -> Vec<DailyPoint> {
    struct Day {
        realized: f64,
        end_balance: f64,
        last_ts: i64,
        /// Balance just before this day's first entry (its `cashBalance - change`).
        pre_balance: f64,
        first_ts: i64,
    }

    let mut days: BTreeMap<i64, Day> = BTreeMap::new();
    for e in ledger {
        if e.ts_ms <= 0 {
            continue;
        }
        let day = e.ts_ms / DAY_MS;
        let d = days.entry(day).or_insert(Day {
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

    let mut prev_end: Option<f64> = None;
    let mut idx = 100.0_f64;
    let mut out = Vec::with_capacity(days.len());
    for (day, d) in days {
        let start_balance = prev_end.unwrap_or(d.pre_balance);
        let daily_return = if start_balance > 0.0 {
            d.realized / start_balance
        } else {
            0.0
        };
        idx *= 1.0 + daily_return;
        out.push(DailyPoint {
            ts: day * DAY_S,
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
            entry(d0 + 1000, "TRADE", 10.0, 1010.0), // pre-balance 1000
            entry(d0 + 2000, "SETTLEMENT", -5.0, 1005.0), // realized day0 = 5
            entry(d1 + 1000, "TRANSFER_IN", 500.0, 1505.0), // deposit — not return
            entry(d1 + 2000, "TRADE", 20.0, 1525.0), // realized day1 = 20
        ];
        let pts = daily_points(&ledger);
        assert_eq!(pts.len(), 2);

        // Day 0: 5 / 1000 = 0.5% -> index 100.5.
        assert!((pts[0].index - 100.5).abs() < 1e-6);
        assert!((pts[0].return_pct - 0.5).abs() < 1e-6);

        // Day 1: start balance = day0 close 1005; 100.5 * 20/1005 = 2.0 -> 102.5.
        // The 500 deposit does not affect the return.
        assert!((pts[1].index - 102.5).abs() < 1e-6);
        assert!((pts[1].return_pct - 2.5).abs() < 1e-6);
    }

    #[test]
    fn zero_start_balance_is_flat() {
        // The first-ever entry starts from a 0 pre-balance -> flat, no div-by-zero.
        let pts = daily_points(&[entry(1000, "TRADE", 5.0, 5.0)]);
        assert_eq!(pts.len(), 1);
        assert!((pts[0].index - 100.0).abs() < 1e-9);
        assert!((pts[0].return_pct).abs() < 1e-9);
    }

    #[test]
    fn drops_nonpositive_timestamps_and_handles_empty() {
        assert!(daily_points(&[]).is_empty());
        assert!(daily_points(&[entry(0, "TRADE", 1.0, 1.0)]).is_empty());
    }
}
