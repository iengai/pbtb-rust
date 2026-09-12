use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Context;
use aws_lambda_events::event::eventbridge::EventBridgeEvent;
use lambda_runtime::{Error, LambdaEvent, tracing};

use pbtb_rust::domain::Bot;
use pbtb_rust::domain::configswitch::ConfigSwitchRepository;
use pbtb_rust::domain::exchange::Exchange;
use pbtb_rust::domain::user::UserRepository;

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
    let mut operators = Operators::default();
    let mut public = Vec::new();
    // The public files to leave in place: every bot the scan lists with a
    // link, built this run or not. A fault on a still-public bot (a throttle,
    // an S3 hiccup) keeps yesterday's file; a bot that lost its link, its
    // role or its row is not in this set and loses its file.
    let mut keep = HashSet::new();
    for bot in &bots {
        match process_bot(&state, bot, now_ms, &mut operators, &mut public).await {
            Ok(true) => ok += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                failed += 1;
                if bot.public_url.is_some() {
                    keep.insert(public_object(&bot.user_id, &bot.id));
                }
                tracing::warn!(
                    bot_id = %bot.id,
                    exchange = %bot.exchange.as_str(),
                    "failed to build return series: {e:#}"
                );
            }
        }
    }

    // The public side comes after every private write, so a fault here leaves
    // the owners' curves refreshed and fails the run loudly: a missing grant on
    // the public prefix must never read as "nothing to publish".
    let published = publish(&state, &public, keep, now_ms / 1000)
        .await
        .map_err(|e| Error::from(format!("Failed to publish the showcase artifacts: {e:#}")))?;

    tracing::info!(
        bots = bots.len(),
        ok,
        skipped,
        failed,
        public = public.len(),
        removed = published,
        "return-curve collection finished"
    );
    Ok(())
}

/// The public object a bot is published at, relative to the public prefix.
fn public_object(user_id: &str, bot_id: &str) -> String {
    format!("bots/{}.json", model::public_id(user_id, bot_id))
}

/// Which accounts are the operator's, looked up once per account and run.
#[derive(Default)]
struct Operators(HashMap<String, bool>);

impl Operators {
    async fn is_operator(&mut self, state: &AppState, user_id: &str) -> anyhow::Result<bool> {
        if let Some(known) = self.0.get(user_id) {
            return Ok(*known);
        }
        let is = UserRepository::find_user(&*state.bots, user_id)
            .await
            .context("read account row")?
            .is_some_and(|u| u.role.is_operator());
        self.0.insert(user_id.to_string(), is);
        Ok(is)
    }
}

/// Build and store one bot's return series. `Ok(true)` when a series was
/// written, `Ok(false)` when the bot was skipped (unsupported exchange or no
/// stored keys), `Err` on a real fetch/write fault.
///
/// A bot the operator has given a public link also yields its showcase
/// artifact, built from the stored state whether or not today's fetch
/// succeeded: a Bybit fault leaves yesterday's public curve in place rather
/// than a hole on the page. The operator lookup comes after the private
/// writes, so a fault in it never costs the owner the day's series.
async fn process_bot(
    state: &AppState,
    bot: &Bot,
    now_ms: i64,
    operators: &mut Operators,
    public: &mut Vec<model::PublicBotSeries>,
) -> anyhow::Result<bool> {
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
    let mut bot_state = s3_writer::read_state(&state.s3, chart_cfg, &bot.user_id, &bot.id)
        .await
        .context("read state")?
        .unwrap_or_default();
    let from_ms = match bot_state.days.last() {
        Some(last) => last.day * DAY_MS,
        None => now_ms - bybit_cfg.backfill_days * DAY_MS,
    };

    let fetched = bybit::fetch_transaction_log(
        &state.http,
        bybit_cfg,
        &creds.key,
        &creds.secret,
        from_ms,
        now_ms,
    )
    .await
    .context("fetch transaction log");
    let fetch_fault = match fetched {
        Ok(ledger) => {
            let (new_days, new_pre) = model::aggregate(&ledger);
            bot_state.merge(new_days, new_pre);
            None
        }
        Err(e) => Some(e),
    };

    let returns = model::compute_points(&bot_state.days, bot_state.first_pre_balance);
    let switches = ConfigSwitchRepository::list_for_bot(&*state.bots, &bot.user_id, &bot.id)
        .await
        .context("read config switches")?;

    if let Some(e) = fetch_fault {
        // Yesterday's public curve stands in for today's.
        push_public(
            state,
            bot,
            operators,
            &returns,
            &switches,
            &bot_state.days,
            now_ms,
            public,
        )
        .await?;
        return Err(e);
    }

    // The artifact is keyed by the bot's immutable id, so a rename never
    // orphans the file; the readable name rides inside as mutable data.
    let series = model::BotReturnSeries::new(
        &bot.id,
        &bot.name,
        bot.exchange.as_str(),
        returns.clone(),
        &switches,
        now_ms / 1000,
    );

    s3_writer::write_state(&state.s3, chart_cfg, &bot.user_id, &bot.id, &bot_state)
        .await
        .context("write state")?;
    s3_writer::put_series(&state.s3, chart_cfg, &bot.user_id, &bot.id, &series)
        .await
        .context("write series")?;

    push_public(
        state,
        bot,
        operators,
        &returns,
        &switches,
        &bot_state.days,
        now_ms,
        public,
    )
    .await?;
    Ok(true)
}

/// Add the bot's showcase artifact when it has one: a link, on the
/// operator's account. The account is read only for a bot with a link.
#[allow(clippy::too_many_arguments)]
async fn push_public(
    state: &AppState,
    bot: &Bot,
    operators: &mut Operators,
    returns: &model::ReturnSeries,
    switches: &[pbtb_rust::domain::configswitch::ConfigSwitchEvent],
    days: &[model::DayAgg],
    now_ms: i64,
    public: &mut Vec<model::PublicBotSeries>,
) -> anyhow::Result<()> {
    let operator = if bot.public_url.is_some() {
        operators.is_operator(state, &bot.user_id).await?
    } else {
        false
    };
    if let Some(url) = model::showcase_link(bot, operator) {
        public.push(model::PublicBotSeries::new(
            bot,
            url,
            returns,
            switches,
            days,
            now_ms / 1000,
        ));
    }
    Ok(())
}

/// Write the showcase artifacts under the public prefix and remove the bot
/// files not in `keep`, the objects of the bots the scan listed with a link.
/// Returns how many were removed.
///
/// The index is written on every run, empty when nothing is public, so the
/// page can tell "nothing public" from "never synced".
async fn publish(
    state: &AppState,
    public: &[model::PublicBotSeries],
    mut keep: HashSet<String>,
    generated_at: i64,
) -> anyhow::Result<usize> {
    let chart_cfg = &state.configs.chart;
    let index = model::PublicIndex::new(public, generated_at);
    s3_writer::put_public(&state.s3, chart_cfg, "index.json", &index)
        .await
        .context("write public index")?;

    for series in public {
        let key = format!("bots/{}.json", series.id);
        s3_writer::put_public(&state.s3, chart_cfg, &key, series)
            .await
            .with_context(|| format!("write public series {key}"))?;
        keep.insert(key);
    }

    let mut removed = 0;
    for key in s3_writer::list_public(&state.s3, chart_cfg, "bots/")
        .await
        .context("list public series")?
    {
        if !keep.contains(&key) {
            s3_writer::delete_public(&state.s3, chart_cfg, &key)
                .await
                .with_context(|| format!("remove public series {key}"))?;
            removed += 1;
        }
    }
    Ok(removed)
}
