//! The monthly raw archive: every Bybit bot's history as the exchange returned
//! it, one object per dataset and UTC month, kept in our own bucket because
//! Bybit serves only the last two years. The return curve keeps daily
//! aggregates alone; this is what lets them be recomputed another way later.
//!
//! A run lists what each bot already has and fetches every complete month
//! missing from it, oldest first, back to the horizon Bybit still serves. So
//! the first run back-fills, a routine run adds the month just closed, and a
//! missed run is made up by the next. A month is written whole or not at all,
//! and an object, once written, is never rewritten. A run that nears the
//! Lambda deadline stops between months; the next invocation carries on.
//!
//! Keys: `{user_id}/{bot_id}/bybit/{dataset}/{YYYY-MM}.jsonl`, one raw row per
//! line. The month the horizon falls in is partial, and says so in its
//! `from-ms` metadata.

use std::collections::HashSet;

use anyhow::{Context, Result};
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use lambda_runtime::tracing;
use serde_json::value::RawValue;

use pbtb_rust::domain::Bot;
use pbtb_rust::domain::exchange::Exchange;

use crate::AppState;
use crate::bybit;

/// The `detail.job` of the monthly rule's event.
pub const JOB: &str = "raw-archive";

const DAY_MS: i64 = 86_400_000;

/// Time left before the Lambda deadline below which no new month is started.
/// The busiest month seen is a few hundred requests, well inside it.
const DEADLINE_MARGIN_MS: i64 = 180_000;

/// One archived history: its key segment, endpoint and filters.
struct Dataset {
    name: &'static str,
    path: &'static str,
    query: fn(&str) -> String,
}

/// The transaction log is every balance change of the unified account, all
/// categories and coins; the executions are the linear fills, with the order
/// ids and prices the log does not carry.
const DATASETS: [Dataset; 2] = [
    Dataset {
        name: "transaction-log",
        path: "/v5/account/transaction-log",
        query: |_| "accountType=UNIFIED&limit=50".to_string(),
    },
    Dataset {
        name: "executions",
        path: "/v5/execution/list",
        query: |settle| format!("category=linear&settleCoin={settle}&limit=100"),
    },
];

/// Whether an event is the monthly archive's rather than the daily curve's.
pub fn is_archive_event(detail: &serde_json::Value) -> bool {
    detail.get("job").and_then(|j| j.as_str()) == Some(JOB)
}

/// A UTC calendar month.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Month {
    pub year: i64,
    pub month: u32,
}

impl Month {
    fn from_ms(ms: i64) -> Self {
        let (year, month, _) = civil_from_days(ms.div_euclid(DAY_MS));
        Month { year, month }
    }

    fn start_ms(self) -> i64 {
        days_from_civil(self.year, self.month, 1) * DAY_MS
    }

    fn next(self) -> Self {
        if self.month == 12 {
            Month {
                year: self.year + 1,
                month: 1,
            }
        } else {
            Month {
                year: self.year,
                month: self.month + 1,
            }
        }
    }

    fn label(self) -> String {
        format!("{:04}-{:02}", self.year, self.month)
    }
}

/// Every month that has closed by `now_ms` and still has data past
/// `horizon_ms`, oldest first, each with the window to fetch: the whole month,
/// except the first, which starts at the horizon.
pub fn complete_months(horizon_ms: i64, now_ms: i64) -> Vec<(Month, i64, i64)> {
    let mut out = Vec::new();
    let mut m = Month::from_ms(horizon_ms);
    loop {
        let end = m.next().start_ms();
        if end > now_ms {
            break;
        }
        out.push((m, m.start_ms().max(horizon_ms), end));
        m = m.next();
    }
    out
}

fn bot_prefix(bot: &Bot) -> String {
    format!("{}/{}/bybit/", bot.user_id, bot.id)
}

fn object_key(bot: &Bot, dataset: &str, month: Month) -> String {
    format!("{}{dataset}/{}.jsonl", bot_prefix(bot), month.label())
}

/// Rows as JSON Lines, each exactly as Bybit sent it.
fn to_jsonl(rows: &[Box<RawValue>]) -> Vec<u8> {
    let mut body = Vec::new();
    for row in rows {
        body.extend_from_slice(row.get().as_bytes());
        body.push(b'\n');
    }
    body
}

async fn existing_keys(client: &Client, bucket: &str, prefix: &str) -> Result<HashSet<String>> {
    let mut keys = HashSet::new();
    let mut pages = client
        .list_objects_v2()
        .bucket(bucket)
        .prefix(prefix)
        .into_paginator()
        .send();
    while let Some(page) = pages.next().await {
        let page = page.context("list archived months")?;
        keys.extend(
            page.contents()
                .iter()
                .filter_map(|o| o.key().map(str::to_string)),
        );
    }
    Ok(keys)
}

#[derive(Default)]
struct Tally {
    written: u32,
    rows: usize,
    failed: u32,
    /// Bot-months left for the next run because the deadline came first.
    pending: u32,
}

/// Archive every Bybit bot's missing months. `deadline_ms` is the Lambda's.
pub async fn run(state: &AppState, now_ms: i64, deadline_ms: i64) -> Result<()> {
    let Some(cfg) = &state.configs.archive else {
        tracing::warn!("no archive bucket configured; raw archive skipped");
        return Ok(());
    };
    let bots = state.bots.find_all().await.context("list bots")?;
    let horizon_ms = now_ms - state.configs.bybit.backfill_days * DAY_MS;
    let months = complete_months(horizon_ms, now_ms);

    let mut tally = Tally::default();
    for bot in bots.iter().filter(|b| b.exchange == Exchange::Bybit) {
        if let Err(e) = archive_bot(
            state,
            &cfg.bucket_name,
            bot,
            &months,
            deadline_ms,
            &mut tally,
        )
        .await
        {
            tally.failed += 1;
            tracing::warn!(bot_id = %bot.id, "raw archive failed: {e:#}");
        }
    }

    tracing::info!(
        written = tally.written,
        rows = tally.rows,
        failed = tally.failed,
        pending = tally.pending,
        "raw archive finished"
    );
    Ok(())
}

async fn archive_bot(
    state: &AppState,
    bucket: &str,
    bot: &Bot,
    months: &[(Month, i64, i64)],
    deadline_ms: i64,
    tally: &mut Tally,
) -> Result<()> {
    let Some(creds) = state
        .api_keys
        .get(&bot.user_id, &bot.id)
        .await
        .context("read api keys")?
    else {
        tracing::warn!(bot_id = %bot.id, "no api keys stored; raw archive skipped");
        return Ok(());
    };
    let have = existing_keys(&state.s3, bucket, &bot_prefix(bot)).await?;
    let settle = &state.configs.bybit.settle_coin;

    for &(month, from_ms, to_ms) in months {
        for ds in &DATASETS {
            let key = object_key(bot, ds.name, month);
            if have.contains(&key) {
                continue;
            }
            if crate::event_handler::wall_ms() > deadline_ms - DEADLINE_MARGIN_MS {
                tally.pending += 1;
                continue;
            }
            let rows = bybit::fetch_raw_rows(
                &state.http,
                &state.configs.bybit,
                &creds.key,
                &creds.secret,
                ds.path,
                &(ds.query)(settle),
                from_ms,
                to_ms,
            )
            .await
            .with_context(|| format!("fetch {} {}", ds.name, month.label()))?;
            state
                .s3
                .put_object()
                .bucket(bucket)
                .key(&key)
                .body(ByteStream::from(to_jsonl(&rows)))
                .content_type("application/x-ndjson")
                .metadata("from-ms", from_ms.to_string())
                .metadata("to-ms", to_ms.to_string())
                .metadata("rows", rows.len().to_string())
                .send()
                .await
                .with_context(|| format!("write {key}"))?;
            tally.written += 1;
            tally.rows += rows.len();
        }
    }
    Ok(())
}

// Howard Hinnant's civil-calendar conversions (proleptic Gregorian, UTC days
// since 1970-01-01).

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ms(y: i64, m: u32, d: u32) -> i64 {
        days_from_civil(y, m, d) * DAY_MS
    }

    #[test]
    fn civil_round_trips_across_leap_years_and_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        for z in [-1, 59, 60, 11_016, 20_513, 20_878, 21_000] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
        assert_eq!(civil_from_days(days_from_civil(2028, 2, 29)), (2028, 2, 29));
    }

    #[test]
    fn complete_months_stop_before_the_current_one() {
        let now = ms(2026, 10, 2) + 3 * 3_600_000;
        let horizon = ms(2026, 7, 15);
        let months = complete_months(horizon, now);
        let labels: Vec<_> = months.iter().map(|(m, _, _)| m.label()).collect();
        assert_eq!(labels, ["2026-07", "2026-08", "2026-09"]);
        // The horizon's month is partial; the rest are whole.
        assert_eq!(months[0].1, horizon);
        assert_eq!(months[0].2, ms(2026, 8, 1));
        assert_eq!(
            (months[2].1, months[2].2),
            (ms(2026, 9, 1), ms(2026, 10, 1))
        );
    }

    #[test]
    fn a_month_closing_exactly_now_is_complete() {
        let now = ms(2027, 1, 1);
        let months = complete_months(ms(2026, 12, 1), now);
        assert_eq!(months.len(), 1);
        assert_eq!(
            months[0].0,
            Month {
                year: 2026,
                month: 12
            }
        );
    }

    #[test]
    fn the_default_horizon_spans_two_years_of_months() {
        let now = ms(2026, 10, 2);
        let months = complete_months(now - 720 * DAY_MS, now);
        assert_eq!(months.first().unwrap().0.label(), "2024-10");
        assert_eq!(months.last().unwrap().0.label(), "2026-09");
        assert_eq!(months.len(), 24);
    }

    #[test]
    fn only_the_archive_job_is_recognised() {
        assert!(is_archive_event(&json!({"job": "raw-archive"})));
        assert!(!is_archive_event(&json!({})));
        assert!(!is_archive_event(&json!({"job": "other"})));
    }

    #[test]
    fn rows_are_kept_verbatim_one_per_line() {
        let page = r#"{"list":[{"id":"1","change":"-0.10", "cashBalance":"12.5"},{"id":"2"}]}"#;
        #[derive(serde::Deserialize)]
        struct Page {
            list: Vec<Box<RawValue>>,
        }
        let rows = serde_json::from_str::<Page>(page).unwrap().list;
        let body = String::from_utf8(to_jsonl(&rows)).unwrap();
        assert_eq!(
            body,
            "{\"id\":\"1\",\"change\":\"-0.10\", \"cashBalance\":\"12.5\"}\n{\"id\":\"2\"}\n"
        );
        assert!(to_jsonl(&[]).is_empty());
    }
}
