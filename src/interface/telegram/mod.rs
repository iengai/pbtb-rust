// Rust
pub mod bind;
pub mod callbacks;
pub mod commands;
pub mod dialogue;
pub mod keyboards;
pub mod middlewares;
pub mod router;
pub mod states;
pub mod types;
pub mod views;
// The redaction policy is adapter-independent and lives at the interface root.
// Re-exported here so the `super::redaction` path the sibling modules use
// resolves without a `use` line in each of them.
pub use crate::interface::redaction;

// Dependencies aggregation for handlers
use crate::domain::bot::Bot;
use crate::domain::error::DomainError;
use crate::domain::runtime::RuntimePhase;
use crate::usecase::*;
use std::sync::Arc;

/// How long a single handler body may run before it is abandoned.
///
/// Generous enough for the slowest legitimate panel (a bot list decorated with
/// one runtime read per bot), short enough that a wedged handler does not take
/// the chat down with it.
const HANDLER_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

/// Runs one handler body under a deadline, and makes both failure modes visible.
///
/// Each handler maps its error to an empty `DependencyMap`, which teloxide
/// discards, so a failure reaches nobody unless it is recorded here. The
/// deadline matters just as much: teloxide processes one chat's
/// updates strictly in order, so a body that never returns blocks every later
/// update for that chat and reads as the whole bot being dead.
pub(crate) async fn with_deadline<F>(handler: &str, fut: F) -> anyhow::Result<()>
where
    F: std::future::Future<Output = anyhow::Result<()>>,
{
    match tokio::time::timeout(HANDLER_DEADLINE, fut).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            log::error!("handler {handler} failed: {e:#}");
            Err(e)
        }
        Err(_) => {
            log::error!(
                "handler {handler} exceeded {}s and was abandoned",
                HANDLER_DEADLINE.as_secs()
            );
            Err(anyhow::anyhow!("handler {handler} timed out"))
        }
    }
}

/// One of the caller's own bots by id. Every per-bot panel resolves the bot
/// under the Telegram user's own id, so a user can only ever read or change
/// their own bots; an id that is not theirs is indistinguishable from one that
/// does not exist.
pub(crate) async fn find_own_bot(
    deps: &Deps,
    user_id: &str,
    bot_id: &str,
) -> Result<Option<Bot>, DomainError> {
    Ok(deps
        .list_bots_usecase
        .execute(user_id)
        .await?
        .into_iter()
        .find(|b| b.id == bot_id))
}

/// Pair each bot with its OBSERVED runtime phase (for list buttons / status
/// lines). One runtime read per bot; phase is `None` when no record exists.
pub(crate) async fn bots_with_phase(
    deps: &Deps,
    user_id: &str,
    bots: Vec<Bot>,
) -> Vec<(Bot, Option<RuntimePhase>)> {
    let mut out = Vec::with_capacity(bots.len());
    for b in bots {
        // Best-effort status decoration: a runtime-read failure degrades to "no
        // phase" rather than failing the whole list, but is recorded — never
        // silently swallowed (docs/conventions.md § Error Handling).
        let phase = match deps.get_bot_runtime_usecase.execute(user_id, &b.id).await {
            Ok(runtime) => runtime.map(|r| r.phase),
            Err(e) => {
                tracing::warn!(
                    user_id,
                    bot_id = %b.id,
                    error = %e,
                    "failed to read runtime phase for list display"
                );
                None
            }
        };
        out.push((b, phase));
    }
    out
}

#[derive(Clone)]
pub struct Deps {
    // Bot management
    pub list_bots_usecase: Arc<ListBotsUseCase>,
    pub add_bot_usecase: Arc<AddBotUseCase>,
    pub delete_bot_usecase: Arc<DeleteBotUseCase>,

    // Template management
    pub list_templates_usecase: Arc<ListTemplatesUseCase>,

    // Bot config management
    pub apply_template_usecase: Arc<ApplyTemplateUseCase>,
    pub get_bot_config_usecase: Arc<GetBotConfigUseCase>,
    pub update_bot_config_usecase: Arc<UpdateBotConfigUseCase>,
    pub update_risk_level_usecase: Arc<UpdateRiskLevelUseCase>,
    pub set_strategy_side_usecase: Arc<SetStrategySideUseCase>,
    pub set_bot_runtime_usecase: Arc<SetBotRuntimeUseCase>,
    pub set_bot_public_url_usecase: Arc<SetBotPublicUrlUseCase>,

    // Runtime / desired-state management
    pub get_bot_runtime_usecase: Arc<GetBotRuntimeUseCase>,

    // Who the sender is, and binding/unbinding the Telegram id an account
    // speaks through (the ticket is minted on the web)
    pub resolve_sender_usecase: Arc<ResolveTelegramSenderUseCase>,
    pub bind_telegram_usecase: Arc<BindTelegramUseCase>,
    pub unbind_telegram_usecase: Arc<UnbindTelegramUseCase>,

    // ECS actuation (desired state -> real RunTask/StopTask)
    pub start_bot_usecase: Arc<StartBotUseCase>,
    pub stop_bot_usecase: Arc<StopBotUseCase>,
    pub restart_bot_usecase: Arc<RestartBotUseCase>,
}
