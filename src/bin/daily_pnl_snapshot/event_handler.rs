use std::sync::Arc;

use anyhow::Context;
use aws_lambda_events::event::eventbridge::EventBridgeEvent;
use lambda_runtime::{Error, LambdaEvent, tracing};

use pbtb_rust::domain::Bot;
use pbtb_rust::domain::configswitch::ConfigSwitchRepository;
use pbtb_rust::domain::exchange::Exchange;

use crate::AppState;
use crate::{bybit, model, s3_writer};

const DAY_MS: i64 = 86_400_000;

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

    // Only the daily schedule triggers a collection run. Any other invocation —
    // notably the deploy pipeline's benign smoke test — returns before any Bybit
    // or S3 call, mirroring the task-state handler's guard-clause safety net.
    if payload.detail_type != "Scheduled Event" {
        tracing::info!(
            "Ignore event: source={:?}, detail-type={:?}",
            payload.source,
            payload.detail_type
        );
        return Ok(());
    }

    // The as-of instant for this run: the schedule's fire time, falling back to
    // wall clock. Drives the back-fill window end.
    let now_ms = payload
        .time
        .map(|t| t.timestamp_millis())
        .unwrap_or_else(wall_ms);

    let bots = state
        .bots
        .find_all()
        .await
        .map_err(|e| Error::from(format!("Failed to list bots: {e:#}")))?;

    // Each bot is independent: a per-bot failure is logged (never with the
    // key/secret) and the run continues, so one broken account cannot starve the
    // others. A fetch fault is never turned into an empty/partial JSON write.
    let (mut ok, mut failed, mut skipped) = (0u32, 0u32, 0u32);
    for bot in &bots {
        match process_bot(&state, bot, now_ms).await {
            Ok(true) => ok += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                failed += 1;
                tracing::warn!(
                    bot_id = %bot.id,
                    exchange = %bot.exchange.as_str(),
                    "failed to build return series: {e:#}"
                );
            }
        }
    }

    tracing::info!(
        bots = bots.len(),
        ok,
        skipped,
        failed,
        "return-curve collection finished"
    );
    Ok(())
}

/// Build and store one bot's return series. `Ok(true)` when a series was
/// written, `Ok(false)` when the bot was skipped (unsupported exchange or no
/// stored keys), `Err` on a real fetch/write fault.
async fn process_bot(state: &AppState, bot: &Bot, now_ms: i64) -> anyhow::Result<bool> {
    // Bybit-only today; other exchanges plug in later as new adapters that
    // produce the same neutral `BotReturnSeries`.
    if bot.exchange != Exchange::Bybit {
        return Ok(false);
    }

    let Some(creds) = state
        .api_keys
        .get(&bot.user_id, &bot.id)
        .await
        .context("read api keys")?
    else {
        tracing::warn!(bot_id = %bot.id, "no api keys stored; skipping");
        return Ok(false);
    };

    let bybit_cfg = &state.configs.bybit;
    let chart_cfg = &state.configs.chart;

    // Resume from stored state: on a routine run re-fetch only from the last
    // stored day (re-doing that day catches late settlements, then extends);
    // on a bot's first run fetch the initial backfill window.
    let mut bot_state = s3_writer::read_state(&state.s3, chart_cfg, &bot.id)
        .await
        .context("read state")?
        .unwrap_or_default();
    let from_ms = match bot_state.days.last() {
        Some(last) => last.day * DAY_MS,
        None => now_ms - bybit_cfg.backfill_days * DAY_MS,
    };

    let ledger = bybit::fetch_transaction_log(
        &state.http,
        bybit_cfg,
        &creds.key,
        &creds.secret,
        from_ms,
        now_ms,
    )
    .await
    .context("fetch transaction log")?;
    let (new_days, new_pre) = model::aggregate(&ledger);
    bot_state.merge(new_days, new_pre);

    let points = model::compute_points(&bot_state.days, bot_state.first_pre_balance);
    let switches = ConfigSwitchRepository::list_for_bot(&*state.bots, &bot.user_id, &bot.id)
        .await
        .context("read config switches")?;

    // Public artifact is keyed by the anonymized label; the real id only ever
    // appears in the private state object.
    let label = model::anon_label(&bot.id);
    let series = model::BotReturnSeries::new(
        &label,
        bot.exchange.as_str(),
        points,
        &switches,
        now_ms / 1000,
    );

    s3_writer::write_state(&state.s3, chart_cfg, &bot.id, &bot_state)
        .await
        .context("write state")?;
    s3_writer::put_series(&state.s3, chart_cfg, &label, &series)
        .await
        .context("write series")?;

    Ok(true)
}
