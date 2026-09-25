use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use aws_lambda_events::event::eventbridge::EventBridgeEvent;
use lambda_runtime::{Error, LambdaEvent, tracing};

use pbtb_rust::domain::configtemplate::ConfigTemplateRepository;
use pbtb_rust::domain::exchange::Exchange;

use crate::AppState;
use crate::days::{approved_coins, reachable_days, ymd};
use crate::hyperliquid;

fn wall_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) async fn function_handler(
    event: LambdaEvent<EventBridgeEvent>,
    state: Arc<AppState>,
) -> Result<(), Error> {
    let payload = event.payload;

    // Only the daily schedule triggers a run; the deploy pipeline's smoke test
    // returns here, before any S3 or Hyperliquid call.
    if payload.detail_type != "Scheduled Event" {
        tracing::info!(
            "Ignore event: source={:?}, detail-type={:?}",
            payload.source,
            payload.detail_type
        );
        return Ok(());
    }

    // Wall clock rather than the schedule's fire time: an EventBridge retry
    // lands later, and the reach is measured from when the requests go out.
    let now_ms = wall_ms();
    let days = reachable_days(now_ms);

    let coins = hyperliquid_coins(&state)
        .await
        .map_err(|e| Error::from(format!("Failed to read the templates: {e:#}")))?;

    let (mut stored, mut failed) = (0u32, Vec::new());
    for coin in &coins {
        match collect_coin(&state, coin, days.clone()).await {
            Ok(n) => stored += n,
            Err(e) => {
                tracing::warn!(coin = %coin, "failed to collect candles: {e:#}");
                failed.push(coin.as_str());
            }
        }
    }

    tracing::info!(
        coins = coins.len(),
        stored,
        failed = failed.len(),
        "candle run done for {}..={}",
        ymd(*days.start()),
        ymd(*days.end())
    );
    if !failed.is_empty() {
        // The days stay in reach for a few more runs, and the next one fetches
        // whatever this one could not.
        return Err(Error::from(format!(
            "candle collection failed for {}",
            failed.join(", ")
        )));
    }
    Ok(())
}

/// Every coin a Hyperliquid template in `predefined/` approves.
async fn hyperliquid_coins(state: &AppState) -> Result<BTreeSet<String>> {
    let mut coins = BTreeSet::new();
    for name in state.templates.list().await? {
        let template = state.templates.get(&name).await?;
        match template.exchange() {
            Ok(Exchange::Hyperliquid) => coins.extend(approved_coins(&template.config_data)),
            Ok(_) => {}
            Err(e) => tracing::warn!(template = %name, "skipping template: {e}"),
        }
    }
    Ok(coins)
}

/// Store every reachable day of `coin` not stored yet; the number stored.
async fn collect_coin(
    state: &AppState,
    coin: &str,
    days: std::ops::RangeInclusive<i64>,
) -> Result<u32> {
    let first = *days.start();
    let mut stored = 0;
    for day in days {
        if state.store.has_day(coin, day).await? {
            continue;
        }
        // Reported once: the next run finds this day stored.
        if day == first
            && !state.store.has_day(coin, day - 1).await?
            && state.store.has_any(coin).await?
        {
            tracing::error!(
                tags.coin = %coin,
                "no 1m candles of {coin} before {}: that day is past Hyperliquid's reach, so the gap is permanent",
                ymd(day)
            );
        }
        let rows =
            hyperliquid::day_candles(&state.http, &state.configs.hyperliquid, coin, day).await;
        tokio::time::sleep(Duration::from_millis(
            state.configs.candles.request_interval_ms,
        ))
        .await;
        let rows = rows?;
        // A coin Hyperliquid does not list (delisted, or misspelt in a
        // template) answers a whole past day with no rows. The empty day is
        // stored all the same, so it reads as held and the gap alert above
        // does not fire again on every later run.
        if rows.is_empty() {
            tracing::warn!(coin = %coin, "no candles for {}", ymd(day));
        } else if rows.len() < 1_440 {
            tracing::info!(coin = %coin, rows = rows.len(), "minutes without trades on {}", ymd(day));
        }
        state.store.put_day(coin, day, &rows).await?;
        stored += 1;
    }
    Ok(stored)
}
