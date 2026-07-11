//! Exchange-neutral output types + the transaction-log → daily-series transform.
//!
//! Nothing here is Bybit-specific: a future exchange adapter produces the same
//! `LedgerEntry` slice and the same `BotReturnSeries` comes out, so the chart and
//! the JSON schema never change. This is a flat DTO layer — no domain modelling.

use std::collections::BTreeMap;

use serde::Serialize;

use pbtb_rust::domain::Bot;
use pbtb_rust::domain::configswitch::ConfigSwitchEvent;

use crate::bybit::LedgerEntry;

const DAY_S: i64 = 86_400;

/// One point per UTC day. Amounts are in the account's settlement coin.
#[derive(Debug, Clone, Serialize)]
pub struct DailyPoint {
    /// UTC midnight of the day, in Unix seconds.
    pub ts: i64,
    /// Wallet cash balance at the end of the day (last entry's `cashBalance`).
    pub balance: f64,
    /// Realized PnL booked during the day (net of fees/funding; excludes
    /// deposits and withdrawals).
    pub realized_pnl: f64,
    /// Cumulative realized PnL up to and including the day — the profit curve.
    pub cumulative_pnl: f64,
    /// Cumulative net deposits (deposits minus withdrawals) up to the day, so a
    /// balance change from moving capital can be told apart from actual profit.
    pub cumulative_net_deposit: f64,
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
    /// When this artifact was generated, in Unix seconds.
    pub generated_at: i64,
    /// Current total equity incl. unrealized PnL (point-in-time). The daily
    /// series is realized-only, so this is the live headline figure.
    pub current_equity: f64,
    pub points: Vec<DailyPoint>,
    pub config_switches: Vec<SwitchMarker>,
}

impl BotReturnSeries {
    pub fn new(
        bot: &Bot,
        current_equity: f64,
        points: Vec<DailyPoint>,
        switches: &[ConfigSwitchEvent],
        generated_at: i64,
    ) -> Self {
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
            current_equity,
            points,
            config_switches,
        }
    }
}

/// Deposits/withdrawals move capital without being profit, so they are tracked
/// separately from realized PnL. Everything else (trades, settlement/funding,
/// delivery, bonus, interest) nets into realized PnL.
fn is_capital_flow(kind: &str) -> bool {
    matches!(kind, "TRANSFER_IN" | "TRANSFER_OUT")
}

/// Aggregate raw ledger entries into a chronological daily series with running
/// cumulative totals. Entries with a non-positive timestamp are dropped.
pub fn daily_points(ledger: &[LedgerEntry]) -> Vec<DailyPoint> {
    struct Day {
        realized: f64,
        net_deposit: f64,
        last_ts_ms: i64,
        last_balance: f64,
    }

    let mut days: BTreeMap<i64, Day> = BTreeMap::new();
    for e in ledger {
        if e.ts_ms <= 0 {
            continue;
        }
        let day = e.ts_ms / DAY_MS_I64;
        let d = days.entry(day).or_insert(Day {
            realized: 0.0,
            net_deposit: 0.0,
            last_ts_ms: 0,
            last_balance: 0.0,
        });
        if is_capital_flow(&e.kind) {
            d.net_deposit += e.change;
        } else {
            d.realized += e.change;
        }
        if e.ts_ms >= d.last_ts_ms {
            d.last_ts_ms = e.ts_ms;
            d.last_balance = e.cash_balance;
        }
    }

    let mut cum_pnl = 0.0;
    let mut cum_dep = 0.0;
    let mut out = Vec::with_capacity(days.len());
    for (day, d) in days {
        cum_pnl += d.realized;
        cum_dep += d.net_deposit;
        out.push(DailyPoint {
            ts: day * DAY_S,
            balance: d.last_balance,
            realized_pnl: d.realized,
            cumulative_pnl: cum_pnl,
            cumulative_net_deposit: cum_dep,
        });
    }
    out
}

const DAY_MS_I64: i64 = 86_400_000;

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
    fn buckets_by_day_and_runs_cumulative_totals() {
        // Day 0: two trades (+10, -3) and a deposit (+100). Day 1: a trade (+5).
        let d0 = 0;
        let d1 = DAY_MS_I64;
        let ledger = vec![
            entry(d0 + 1_000, "TRADE", 10.0, 110.0),
            entry(d0 + 2_000, "TRANSFER_IN", 100.0, 100.0),
            entry(d0 + 3_000, "SETTLEMENT", -3.0, 107.0),
            entry(d1 + 1_000, "TRADE", 5.0, 112.0),
        ];
        let pts = daily_points(&ledger);
        assert_eq!(pts.len(), 2);

        // Day 0: realized = 10 - 3 = 7; net deposit = 100; balance = last (107).
        assert_eq!(pts[0].ts, 0);
        assert!((pts[0].realized_pnl - 7.0).abs() < 1e-9);
        assert!((pts[0].cumulative_pnl - 7.0).abs() < 1e-9);
        assert!((pts[0].cumulative_net_deposit - 100.0).abs() < 1e-9);
        assert!((pts[0].balance - 107.0).abs() < 1e-9);

        // Day 1: realized = 5; cumulative = 12; deposits carry forward at 100.
        assert_eq!(pts[1].ts, DAY_S);
        assert!((pts[1].cumulative_pnl - 12.0).abs() < 1e-9);
        assert!((pts[1].cumulative_net_deposit - 100.0).abs() < 1e-9);
        assert!((pts[1].balance - 112.0).abs() < 1e-9);
    }

    #[test]
    fn drops_nonpositive_timestamps_and_handles_empty() {
        assert!(daily_points(&[]).is_empty());
        assert!(daily_points(&[entry(0, "TRADE", 1.0, 1.0)]).is_empty());
    }
}
