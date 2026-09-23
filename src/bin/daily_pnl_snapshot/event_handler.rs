use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Context;
use aws_lambda_events::event::eventbridge::EventBridgeEvent;
use lambda_runtime::{Error, LambdaEvent, tracing};

use pbtb_rust::domain::Bot;
use pbtb_rust::domain::configswitch::ConfigSwitchRepository;
use pbtb_rust::domain::exchange::Exchange;
use pbtb_rust::domain::showcase::{PublicBotSeries, is_published, public_id};
use pbtb_rust::domain::user::UserRepository;

use crate::AppState;
use crate::{bybit, hyperliquid, model, s3_writer};

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
    // notably the deploy pipeline's benign smoke test — returns before any
    // exchange or S3 call, mirroring the task-state handler's guard-clause safety net.
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
    // The showcase artifact of every operator bot built this run, shown or not.
    let mut built = Vec::new();
    for bot in &bots {
        match process_bot(&state, bot, now_ms, &mut operators, &mut built).await {
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

    // The public side comes after every private write, so a fault here leaves
    // the owners' curves refreshed and fails the run loudly: a missing grant on
    // the public prefix must never read as "nothing to publish".
    let (written, removed) = publish(&state, &built, &mut operators)
        .await
        .map_err(|e| Error::from(format!("Failed to publish the showcase artifacts: {e:#}")))?;

    tracing::info!(
        bots = bots.len(),
        ok,
        skipped,
        failed,
        showcase = built.len(),
        public = written,
        removed,
        "return-curve collection finished"
    );
    Ok(())
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
/// written, `Ok(false)` when the bot was skipped (no stored keys), `Err` on a
/// real fetch/write fault.
///
/// A bot on the operator's account also yields its showcase artifact, shown or
/// not, built from the stored state whether or not today's fetch succeeded: an
/// exchange fault leaves yesterday's curve in place rather than a hole on the
/// page. The operator lookup comes after the private writes, so a fault in it
/// never costs the owner the day's series.
async fn process_bot(
    state: &AppState,
    bot: &Bot,
    now_ms: i64,
    operators: &mut Operators,
    built: &mut Vec<PublicBotSeries>,
) -> anyhow::Result<bool> {
    let Some(creds) = state
        .api_keys
        .get(&bot.user_id, &bot.id)
        .await
        .context("read api keys")?
    else {
        tracing::warn!(bot_id = %bot.id, "no api keys stored; skipping");
        return Ok(false);
    };

    let chart_cfg = &state.configs.chart;
    let backfill_days = match bot.exchange {
        Exchange::Bybit => state.configs.bybit.backfill_days,
        Exchange::Hyperliquid => state.configs.hyperliquid.backfill_days,
    };

    // Resume from stored state: on a routine run re-fetch only from the last
    // stored day (re-doing that day catches late settlements, then extends);
    // on a bot's first run fetch the initial backfill window.
    let mut bot_state = s3_writer::read_state(&state.s3, chart_cfg, &bot.user_id, &bot.id)
        .await
        .context("read state")?
        .unwrap_or_default();
    let from_ms = match bot_state.days.last() {
        Some(last) => last.day * DAY_MS,
        None => now_ms - backfill_days * DAY_MS,
    };

    // Each adapter produces the same neutral ledger.
    let fetched = match bot.exchange {
        Exchange::Bybit => bybit::fetch_transaction_log(
            &state.http,
            &state.configs.bybit,
            &creds.key,
            &creds.secret,
            from_ms,
            now_ms,
        )
        .await
        .context("fetch transaction log"),
        Exchange::Hyperliquid => {
            hyperliquid::fetch_ledger(&state.http, &state.configs.hyperliquid, &creds.key, from_ms)
                .await
                .context("fetch ledger")
        }
    };
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
        // Yesterday's curve stands in for today's.
        collect_showcase(
            state,
            bot,
            operators,
            &returns,
            &switches,
            &bot_state.days,
            now_ms,
            built,
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

    collect_showcase(
        state,
        bot,
        operators,
        &returns,
        &switches,
        &bot_state.days,
        now_ms,
        built,
    )
    .await?;
    Ok(true)
}

/// Keep the bot's showcase artifact where the showcase switch publishes it
/// from, when the bot is on the operator's account, and hand it to the run's
/// publish. A hidden bot's artifact is kept too, so showing it on the console
/// needs no run.
#[allow(clippy::too_many_arguments)]
async fn collect_showcase(
    state: &AppState,
    bot: &Bot,
    operators: &mut Operators,
    returns: &model::ReturnSeries,
    switches: &[pbtb_rust::domain::configswitch::ConfigSwitchEvent],
    days: &[model::DayAgg],
    now_ms: i64,
    built: &mut Vec<PublicBotSeries>,
) -> anyhow::Result<()> {
    let operator = operators.is_operator(state, &bot.user_id).await?;
    if let Some(series) =
        model::showcase_artifact(bot, operator, returns, switches, days, now_ms / 1000)
    {
        state
            .showcase
            .put_shadow(&series)
            .await
            .with_context(|| format!("write showcase artifact {}", series.id))?;
        built.push(series);
    }
    Ok(())
}

/// Bring the public prefix in line with the rows as they are now, not as the
/// scan read them minutes ago, so a bot shown or hidden on the console while
/// the run was fetching keeps the operator's latest choice. Writes the built
/// artifacts of the bots published now, removes the artifacts of the bots not
/// published now, and rebuilds the listing from what is left. Returns how many
/// were written and how many removed.
async fn publish(
    state: &AppState,
    built: &[PublicBotSeries],
    operators: &mut Operators,
) -> anyhow::Result<(usize, usize)> {
    let rows = state.bots.find_all().await.context("re-read bots")?;
    let mut published = HashSet::new();
    let mut current = HashMap::new();
    for bot in &rows {
        if bot.on_showcase() && is_published(bot, operators.is_operator(state, &bot.user_id).await?)
        {
            let pid = public_id(&bot.user_id, &bot.id);
            current.insert(pid.clone(), bot);
            published.insert(pid);
        }
    }

    let listed = state
        .showcase
        .list_public()
        .await
        .context("list public artifacts")?;
    let (writes, removes) = model::publish_plan(built, &published, &listed);

    for series in &writes {
        let mut series = (*series).clone();
        if let Some(bot) = current.get(&series.id) {
            series.refresh_from(bot);
        }
        state
            .showcase
            .put_public(&series)
            .await
            .with_context(|| format!("write public artifact {}", series.id))?;
    }
    for pid in &removes {
        state
            .showcase
            .delete_public(pid)
            .await
            .with_context(|| format!("remove public artifact {pid}"))?;
    }
    state
        .showcase
        .rebuild_index()
        .await
        .context("rebuild the public listing")?;
    Ok((writes.len(), removes.len()))
}
