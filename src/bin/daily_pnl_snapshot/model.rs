//! Exchange-neutral output types + the transaction-log → daily-return transform,
//! split so the return index can be recomputed incrementally from accumulated
//! state without re-fetching history.
//!
//! The private output carries two readings of the same ledger: a time-weighted
//! return index (cumulative return %, deposit-neutral) and the realized PnL in
//! the settlement coin, per day and accumulated. It never carries a balance or
//! equity — how much money the owner has is not the collector's to tell. It
//! is keyed by the bot's immutable id, so a rename never orphans it; the
//! readable name rides inside as mutable data. The public output, for a bot
//! the operator put on the showcase page, carries the index alone plus one
//! rounded capital figure per config switch and reset. Nothing here is
//! Bybit-specific.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use pbtb_rust::domain::Bot;
use pbtb_rust::domain::configswitch::ConfigSwitchEvent;

use crate::bybit::LedgerEntry;

const DAY_S: i64 = 86_400;
const DAY_MS: i64 = 86_400_000;

/// A deposit at least this many times the balance it lands on is not a top-up:
/// whatever survived is a rounding error beside the new capital. Calibrated on
/// the live accounts — the largest genuine top-up is ~14x the balance it joins,
/// the smallest post-wipeout re-funding ~42x.
const REVIVE_RATIO: f64 = 20.0;

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
        if self.days.is_empty()
            && let Some(fp) = new_first_pre
        {
            self.first_pre_balance = fp;
        }
        let mut map: BTreeMap<i64, DayAgg> = self.days.drain(..).map(|d| (d.day, d)).collect();
        for d in new_days {
            map.insert(d.day, d);
        }
        self.days = map.into_values().collect();
    }
}

/// One point per UTC day: where the return index stands, and the money the
/// day made. The two do not agree by construction — a capital reset restarts
/// the index but the realized figures keep counting, since the coins earned
/// on either side of a re-funding are equally real.
#[derive(Debug, Clone, Serialize)]
pub struct DailyPoint {
    pub ts: i64,
    pub index: f64,
    pub return_pct: f64,
    /// The day's realized PnL in the settlement coin (net of fees and
    /// funding, deposits excluded).
    pub realized_usdt: f64,
    /// Realized PnL summed from the first recorded day through this one.
    pub cum_realized_usdt: f64,
}

/// The return index over a bot's history, plus the days on which it restarted.
#[derive(Debug, Clone, Default)]
pub struct ReturnSeries {
    pub points: Vec<DailyPoint>,
    /// Timestamps where the index restarts at 100. The capital before such a
    /// day bears no relation to the capital after it.
    pub capital_resets: Vec<i64>,
}

/// A marker the chart draws to show when the bot switched config.
#[derive(Debug, Clone, Serialize)]
pub struct SwitchMarker {
    pub ts: i64,
    pub template_name: String,
}

/// The per-bot artifact the console draws, served to the bot's owner by the
/// API. `id` is the bot's immutable id (also the S3 filename stem); `name` is
/// the current readable display name — mutable, refreshed every run, never
/// used as a key.
#[derive(Debug, Clone, Serialize)]
pub struct BotReturnSeries {
    pub id: String,
    pub name: String,
    pub exchange: String,
    pub generated_at: i64,
    pub current_return_pct: f64,
    /// Realized PnL over every recorded day, in the settlement coin.
    pub total_realized_usdt: f64,
    pub points: Vec<DailyPoint>,
    pub config_switches: Vec<SwitchMarker>,
    /// Days the index restarted; the chart never re-bases across one.
    pub capital_resets: Vec<i64>,
}

impl BotReturnSeries {
    pub fn new(
        id: &str,
        name: &str,
        exchange: &str,
        series: ReturnSeries,
        switches: &[ConfigSwitchEvent],
        generated_at: i64,
    ) -> Self {
        let ReturnSeries {
            points,
            capital_resets,
        } = series;
        let current_return_pct = points.last().map(|p| p.return_pct).unwrap_or(0.0);
        let total_realized_usdt = points.last().map(|p| p.cum_realized_usdt).unwrap_or(0.0);
        let config_switches = switches
            .iter()
            .map(|s| SwitchMarker {
                ts: s.applied_at,
                template_name: s.template_name.clone(),
            })
            .collect();
        Self {
            id: id.to_string(),
            name: name.to_string(),
            exchange: exchange.to_string(),
            generated_at,
            current_return_pct,
            total_realized_usdt,
            points,
            config_switches,
            capital_resets,
        }
    }
}

/// Whether a ledger entry moves money into or out of the account rather than
/// earning or losing it: transfers and deposits, exchange gifts, coin
/// conversions, and the institutional-loan legs. Everything else — trades,
/// settlement, funding, fees and their refunds, liquidation, ADL, margin
/// interest — is the cost or the reward of trading, and counts as realized.
fn is_capital_flow(kind: &str) -> bool {
    matches!(
        kind,
        "TRANSFER_IN"
            | "TRANSFER_OUT"
            | "DEPOSIT"
            | "WITHDRAW"
            | "AIRDROP"
            | "BONUS"
            | "RECEIVE"
            | "CURRENCY_BUY"
            | "CURRENCY_SELL"
    ) || kind.starts_with("TRANSFER_")
        || kind.starts_with("SPOT_REPAYMENT_")
        || kind.ends_with("_INS_LOAN")
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
///
/// A multiplicative chain cannot climb back out of zero, so an account that is
/// wiped out and then re-funded would read −100% for the rest of its life. A
/// re-funding therefore opens a new capital era: the index restarts at 100 and
/// the day is recorded in `capital_resets`. The stake before such a day and the
/// stake after it are different money; nothing may be compounded across one.
pub fn compute_points(days: &[DayAgg], first_pre_balance: f64) -> ReturnSeries {
    let mut prev_end: Option<f64> = None;
    let mut idx = 100.0_f64;
    let mut cum = 0.0_f64;
    let mut points = Vec::with_capacity(days.len());
    let mut capital_resets = Vec::new();

    for d in days {
        let start = prev_end.unwrap_or(first_pre_balance);
        let ts = d.day * DAY_S;
        prev_end = Some(d.end_balance);
        cum += d.realized;
        let realized_usdt = round4(d.realized);
        let cum_realized_usdt = round4(cum);

        // `aggregate` keeps capital flows out of `realized`, so whatever else
        // moved the balance that day is money arriving or leaving.
        let flow = d.end_balance - start - d.realized;
        if flow > 0.0 && d.end_balance > 0.0 && (start <= 0.0 || flow >= REVIVE_RATIO * start) {
            // The day's own return is dropped: it was earned on the dust that
            // the deposit replaced, and there is no base to measure it
            // against. The coins it made are still counted.
            idx = 100.0;
            capital_resets.push(ts);
            points.push(DailyPoint {
                ts,
                index: 100.0,
                return_pct: 0.0,
                realized_usdt,
                cum_realized_usdt,
            });
            continue;
        }

        let dr = if start > 0.0 { d.realized / start } else { 0.0 };
        // A day that loses the whole starting balance ends at zero; letting the
        // factor go negative would flip the sign of every day after it.
        idx = if 1.0 + dr > 0.0 {
            idx * (1.0 + dr)
        } else {
            0.0
        };
        points.push(DailyPoint {
            ts,
            index: round4(idx),
            return_pct: round4(idx - 100.0),
            realized_usdt,
            cum_realized_usdt,
        });
    }

    ReturnSeries {
        points,
        capital_resets,
    }
}

/// One day of a public curve: where the index stands, and nothing about money.
#[derive(Debug, Clone, Serialize)]
pub struct PublicPoint {
    pub ts: i64,
    pub index: f64,
    pub return_pct: f64,
}

/// A config switch on a public curve, with the capital the bot ran the new
/// config at: the wallet balance at the close of the last day before the
/// switch, rounded to a magnitude.
#[derive(Debug, Clone, Serialize)]
pub struct PublicSwitch {
    pub ts: i64,
    pub template_name: String,
    pub cap_usdt: f64,
}

/// A capital reset on a public curve, with the capital the new era started
/// at: the close of the reset day itself, since the day before held the dust
/// the re-funding replaced.
#[derive(Debug, Clone, Serialize)]
pub struct PublicReset {
    pub ts: i64,
    pub cap_usdt: f64,
}

/// The showcase artifact for one bot the operator made public. A type of its
/// own rather than the private series with fields removed: it has no place
/// for realized PnL or a balance, so a money field added to the private
/// series later cannot leak through it. `cap_usdt` is the one balance-derived
/// figure, rounded to a magnitude. `id` is the opaque public id, not the bot
/// id: bot ids are per-tenant keys two tenants may share.
#[derive(Debug, Clone, Serialize)]
pub struct PublicBotSeries {
    pub id: String,
    pub name: String,
    pub exchange: String,
    pub public_url: String,
    pub generated_at: i64,
    pub current_return_pct: f64,
    pub points: Vec<PublicPoint>,
    pub config_switches: Vec<PublicSwitch>,
    pub capital_resets: Vec<PublicReset>,
}

/// The showcase listing: every public bot with what a list row needs, and a
/// 30-day sparkline. Written on every run, empty when nothing is public, so
/// the page can tell "nothing public" from "never synced".
#[derive(Debug, Clone, Serialize)]
pub struct PublicIndex {
    pub generated_at: i64,
    pub bots: Vec<PublicIndexBot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicIndexBot {
    pub id: String,
    pub name: String,
    pub exchange: String,
    pub public_url: String,
    pub current_return_pct: f64,
    pub spark: Vec<f64>,
}

const SPARK_DAYS: usize = 30;

/// `sha256("{user_id}#{bot_id}")`, twelve hex characters: stable for the life
/// of the bot, distinct across tenants, and safe as an S3 key and a URL
/// segment whatever the bot was named.
pub fn public_id(user_id: &str, bot_id: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(&Sha256::digest(format!("{user_id}#{bot_id}"))[..6])
}

/// The link a bot is published under, when it is: the operator's account gave
/// it one. A link on any other account's row is inert.
pub fn showcase_link(bot: &Bot, operator: bool) -> Option<&str> {
    match &bot.public_url {
        Some(url) if operator => Some(url),
        _ => None,
    }
}

/// Round a balance to the nearest of 1, 2 and 5 times a power of ten (100,
/// 200, 500, 1k, 2k, 5k, …); a tie goes down. Coarse on purpose: it is the
/// one balance-derived figure the public sees. Nothing positive rounds to 0.
pub fn round_cap(balance: f64) -> f64 {
    if !balance.is_finite() || balance <= 0.0 {
        return 0.0;
    }
    let base = 10f64.powf(balance.log10().floor());
    let mut best = base;
    for candidate in [2.0 * base, 5.0 * base, 10.0 * base] {
        if (candidate - balance).abs() < (best - balance).abs() {
            best = candidate;
        }
    }
    best
}

/// The close of the last day before the day `ts` falls in, or of the first
/// recorded day when the ledger starts later (a switch recorded before the
/// backfill window). 0 without any day.
fn close_before(days: &[DayAgg], ts: i64) -> f64 {
    let day = ts.div_euclid(DAY_S);
    days.iter()
        .rev()
        .find(|d| d.day < day)
        .or_else(|| days.first())
        .map(|d| d.end_balance)
        .unwrap_or(0.0)
}

/// The close of the day `ts` falls in; 0 when the ledger has no such day.
fn close_on(days: &[DayAgg], ts: i64) -> f64 {
    let day = ts.div_euclid(DAY_S);
    days.iter()
        .find(|d| d.day == day)
        .map(|d| d.end_balance)
        .unwrap_or(0.0)
}

impl PublicBotSeries {
    pub fn new(
        bot: &Bot,
        public_url: &str,
        series: &ReturnSeries,
        switches: &[ConfigSwitchEvent],
        days: &[DayAgg],
        generated_at: i64,
    ) -> Self {
        let points: Vec<PublicPoint> = series
            .points
            .iter()
            .map(|p| PublicPoint {
                ts: p.ts,
                index: p.index,
                return_pct: p.return_pct,
            })
            .collect();
        let current_return_pct = points.last().map(|p| p.return_pct).unwrap_or(0.0);
        let config_switches = switches
            .iter()
            .map(|s| PublicSwitch {
                ts: s.applied_at,
                template_name: s.template_name.clone(),
                cap_usdt: round_cap(close_before(days, s.applied_at)),
            })
            .collect();
        let capital_resets = series
            .capital_resets
            .iter()
            .map(|&ts| PublicReset {
                ts,
                cap_usdt: round_cap(close_on(days, ts)),
            })
            .collect();
        Self {
            id: public_id(&bot.user_id, &bot.id),
            name: bot.name.clone(),
            exchange: bot.exchange.as_str().to_string(),
            public_url: public_url.to_string(),
            generated_at,
            current_return_pct,
            points,
            config_switches,
            capital_resets,
        }
    }
}

impl PublicIndex {
    pub fn new(series: &[PublicBotSeries], generated_at: i64) -> Self {
        let bots = series
            .iter()
            .map(|s| PublicIndexBot {
                id: s.id.clone(),
                name: s.name.clone(),
                exchange: s.exchange.clone(),
                public_url: s.public_url.clone(),
                current_return_pct: s.current_return_pct,
                spark: s
                    .points
                    .iter()
                    .rev()
                    .take(SPARK_DAYS)
                    .map(|p| p.return_pct)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect(),
            })
            .collect();
        Self { generated_at, bots }
    }
}

#[cfg(test)]
mod public_tests {
    use super::*;

    const LINK: &str = "https://www.bybit.com/copyTrade/x";

    fn day(day: i64, realized: f64, end_balance: f64) -> DayAgg {
        DayAgg {
            day,
            realized,
            end_balance,
        }
    }

    fn a_public_bot() -> Bot {
        let mut bot = Bot::create("u-1".into(), "shown".into(), "ak".into(), "sk".into(), 1);
        bot.set_public_url(Some(LINK.into()), 1).unwrap();
        bot
    }

    fn switch(ts: i64, template: &str) -> ConfigSwitchEvent {
        ConfigSwitchEvent::template("u-1".into(), "shown".into(), template.into(), None, ts)
    }

    fn keys(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    out.push(k.clone());
                    keys(v, out);
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|v| keys(v, out)),
            _ => {}
        }
    }

    #[test]
    fn public_bot_series_has_no_money_key() {
        let days = [
            day(0, 10.0, 1010.0),
            day(1, -5.0, 1005.0),
            day(2, 20.0, 1025.0),
        ];
        let series = compute_points(&days, 1000.0);
        let public = PublicBotSeries::new(
            &a_public_bot(),
            LINK,
            &series,
            &[switch(DAY_S + 100, "tpl-a")],
            &days,
            3 * DAY_S,
        );
        let mut found = Vec::new();
        keys(&serde_json::to_value(&public).unwrap(), &mut found);
        assert!(!found.is_empty());
        for key in &found {
            assert!(!key.contains("balance"), "{key}");
            assert!(!key.contains("usdt") || key == "cap_usdt", "{key}");
        }
        let index = PublicIndex::new(&[public], 3 * DAY_S);
        found.clear();
        keys(&serde_json::to_value(&index).unwrap(), &mut found);
        assert!(
            found
                .iter()
                .all(|k| !k.contains("balance") && !k.contains("usdt"))
        );
    }

    #[test]
    fn public_cap_rounds_on_the_1_2_5_series() {
        for (balance, cap) in [
            (1234.0, 1000.0),
            (1600.0, 2000.0),
            (4200.0, 5000.0),
            (1500.0, 1000.0),
            (95.0, 100.0),
            (7.0, 5.0),
            (0.0, 0.0),
            (-3.0, 0.0),
        ] {
            assert_eq!(round_cap(balance), cap, "{balance}");
        }
    }

    #[test]
    fn public_cap_is_the_close_before_a_switch_and_the_close_of_a_reset_day() {
        // Day 2 is a re-funding: 12 of dust, then 5000 arrives.
        let days = [
            day(0, 10.0, 1010.0),
            day(1, -998.0, 12.0),
            day(2, 1.0, 5013.0),
            day(3, 30.0, 5043.0),
        ];
        let series = compute_points(&days, 1000.0);
        assert_eq!(series.capital_resets, vec![2 * DAY_S]);
        let public = PublicBotSeries::new(
            &a_public_bot(),
            LINK,
            &series,
            &[switch(DAY_S + 3600, "tpl-a"), switch(-DAY_S, "tpl-0")],
            &days,
            4 * DAY_S,
        );
        assert_eq!(public.config_switches[0].cap_usdt, 1000.0, "close of day 0");
        assert_eq!(
            public.config_switches[1].cap_usdt, 1000.0,
            "a switch before the ledger reads the first close"
        );
        assert_eq!(
            public.capital_resets[0].cap_usdt, 5000.0,
            "the reset day's own close"
        );
        assert_eq!(public.points.len(), 4);
        assert_eq!(public.current_return_pct, series.points[3].return_pct);
    }

    #[test]
    fn public_id_is_twelve_hex_of_user_and_bot() {
        let id = public_id("u-1", "shown");
        assert_eq!(id.len(), 12);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(id, public_id("u-1", "shown"), "stable");
        assert_ne!(id, public_id("u-2", "shown"), "per tenant");
        assert_eq!(a_public_bot().user_id, "u-1");
        assert_eq!(
            PublicBotSeries::new(&a_public_bot(), LINK, &ReturnSeries::default(), &[], &[], 0).id,
            id
        );
    }

    #[test]
    fn public_material_is_none_without_a_link() {
        let private = Bot::create("u-1".into(), "quiet".into(), "ak".into(), "sk".into(), 1);
        assert_eq!(showcase_link(&private, true), None);
        assert_eq!(showcase_link(&a_public_bot(), true), Some(LINK));
    }

    #[test]
    fn public_material_is_none_when_the_account_is_not_an_operator() {
        assert_eq!(showcase_link(&a_public_bot(), false), None);
    }

    #[test]
    fn the_index_sparkline_is_the_last_thirty_days_oldest_first() {
        let days: Vec<DayAgg> = (0..40).map(|d| day(d, 1.0, 1000.0 + d as f64)).collect();
        let series = compute_points(&days, 1000.0);
        let public = PublicBotSeries::new(&a_public_bot(), LINK, &series, &[], &days, 0);
        let index = PublicIndex::new(&[public], 0);
        let spark = &index.bots[0].spark;
        assert_eq!(spark.len(), 30);
        assert_eq!(spark[29], series.points[39].return_pct);
        assert_eq!(spark[0], series.points[10].return_pct);
        assert_eq!(index.bots[0].id, public_id("u-1", "shown"));
    }
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
        let pts = compute_points(&aggs, first_pre.unwrap()).points;
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
        let one_shot = compute_points(&full_aggs, full_pre.unwrap()).points;

        // Incremental: first two days, then merge day 2 + 3 (re-fetching day 1).
        let (a1, p1) = aggregate(&full[..2]);
        let mut state = BotState::default();
        state.merge(a1, p1);
        let (a2, p2) = aggregate(&full[1..]); // overlaps day 1
        state.merge(a2, p2);
        let incremental = compute_points(&state.days, state.first_pre_balance).points;

        assert_eq!(incremental.len(), one_shot.len());
        for (a, b) in incremental.iter().zip(one_shot.iter()) {
            assert_eq!(a.ts, b.ts);
            assert!((a.index - b.index).abs() < 1e-9, "index mismatch");
        }
    }

    fn agg(day: i64, realized: f64, end_balance: f64) -> DayAgg {
        DayAgg {
            day,
            realized,
            end_balance,
        }
    }

    #[test]
    fn refunding_a_wiped_account_starts_a_new_era() {
        // 1000 traded to dust, then re-funded with 100 and up 10% on that stake.
        let days = vec![
            agg(0, 0.0, 1000.0),
            agg(1, -999.99, 0.01),
            agg(2, 0.0, 100.01),
            agg(3, 10.0, 110.01),
        ];
        let s = compute_points(&days, 1000.0);
        assert_eq!(s.capital_resets, vec![2 * DAY_S]);
        assert_eq!(s.points[2].index, 100.0);
        assert!((s.points[3].return_pct - 9.999).abs() < 0.01);
    }

    #[test]
    fn realized_money_keeps_counting_across_a_reset() {
        // The index forgets the wiped stake; the coins do not: −999.99 lost,
        // then +2 earned on the dust the same day the re-funding landed, then
        // +10 on the new stake.
        let days = vec![
            agg(0, 0.0, 1000.0),
            agg(1, -999.99, 0.01),
            agg(2, 2.0, 102.01),
            agg(3, 10.0, 112.01),
        ];
        let s = compute_points(&days, 1000.0);
        assert_eq!(s.capital_resets, vec![2 * DAY_S]);
        assert_eq!(s.points[2].return_pct, 0.0);
        assert!((s.points[2].realized_usdt - 2.0).abs() < 1e-9);
        assert!((s.points[3].cum_realized_usdt - (-999.99 + 2.0 + 10.0)).abs() < 1e-9);
        let series = BotReturnSeries::new("b", "b", "bybit", s, &[], 0);
        assert!((series.total_realized_usdt - (-987.99)).abs() < 1e-9);
    }

    #[test]
    fn money_arriving_or_leaving_is_not_realized() {
        let ledger = vec![
            entry(1000, "TRADE", 10.0, 1010.0),
            entry(2000, "DEPOSIT", 500.0, 1510.0),
            entry(3000, "BONUS", 5.0, 1515.0),
            entry(4000, "TRANSFER_IN_INS_LOAN", 100.0, 1615.0),
            entry(5000, "SPOT_REPAYMENT_SELL", -20.0, 1595.0),
            entry(6000, "WITHDRAW", -300.0, 1295.0),
            entry(7000, "FEE_REFUND", 0.5, 1295.5),
            entry(8000, "LIQUIDATION", -7.0, 1288.5),
        ];
        let (aggs, _) = aggregate(&ledger);
        assert_eq!(aggs.len(), 1);
        assert!((aggs[0].realized - 3.5).abs() < 1e-9);
        assert!((aggs[0].end_balance - 1288.5).abs() < 1e-9);
    }

    #[test]
    fn a_top_up_onto_a_live_balance_keeps_the_era() {
        // +9000 onto a working 1000 is a deposit, not a revival: TWR already
        // excludes it, and the day's 1% return must survive intact.
        let days = vec![agg(0, 0.0, 1000.0), agg(1, 10.0, 10_010.0)];
        let s = compute_points(&days, 1000.0);
        assert!(s.capital_resets.is_empty());
        assert!((s.points[1].return_pct - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_loss_past_the_starting_balance_stops_at_zero() {
        let days = vec![
            agg(0, 0.0, 100.0),
            agg(1, -150.0, -50.0),
            agg(2, 0.0, -50.0),
        ];
        let s = compute_points(&days, 100.0);
        assert_eq!(s.points[1].index, 0.0);
        assert_eq!(s.points[2].index, 0.0);
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
