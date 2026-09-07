// Rust
use teloxide::dispatching::dialogue::{Dialogue, InMemStorage};
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;

use super::{
    Deps, keyboards,
    redaction::redact,
    states::{BotContext, DialogueState},
};
use crate::domain::engine::Runtime;
use crate::usecase::SetRuntimeOutcome;

type MyDialogue = Dialogue<DialogueState, InMemStorage<DialogueState>>;
type MyBotContext = Dialogue<BotContext, InMemStorage<BotContext>>;

#[derive(BotCommands, Clone)]
#[command(description = "Available commands", rename_rule = "lowercase")]
pub enum Command {
    #[command(description = "Start the bot")]
    Start,
    #[command(description = "list bots")]
    List,
    /// `/runtime <bot_id> py|rs` sets which image the bot launches on;
    /// `/runtime <bot_id>` shows it. The argument line is parsed by hand so a
    /// bare `/runtime` gets usage text instead of falling through to the
    /// dialogue as plain text.
    #[command(description = "show or set a bot's runtime: /runtime <bot_id> [py|rs]")]
    Runtime(String),
}

pub fn routes() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry().branch(
        Update::filter_message()
            .filter_command::<Command>()
            .enter_dialogue::<Message, InMemStorage<DialogueState>, DialogueState>()
            .enter_dialogue::<Message, InMemStorage<BotContext>, BotContext>()
            .endpoint(dispatch_command),
    )
}

async fn dispatch_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    deps: Deps,
    dialogue: MyDialogue,
    bot_context: MyBotContext,
) -> Result<(), DependencyMap> {
    let result = async {
        match cmd {
            Command::Start => {
                // Reset dialogue state to Start (clears any ongoing conversation)
                dialogue.update(DialogueState::Start).await?;

                // Get current bot context to show selected bot info
                let ctx = bot_context.get().await?.unwrap_or_default();

                let welcome_msg = if let Some(ref bot_id) = ctx.selected_bot_id {
                    let user_id = msg.from()
                        .map(|user| user.id.to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    let selected = deps.list_bots_usecase.execute(&user_id).await
                        .ok()
                        .and_then(|bots| bots.into_iter().find(|b| &b.id == bot_id));

                    let bot_info = if let Some(b) = selected {
                        let strategy = deps
                            .get_bot_config_usecase
                            .execute(&user_id, &b.id)
                            .await
                            .ok()
                            .map(|c| super::views::format_strategies(&c.strategies()))
                            .unwrap_or_else(|| "—".to_string());
                        let runtime = deps
                            .get_bot_runtime_usecase
                            .execute(&user_id, &b.id)
                            .await
                            .ok()
                            .flatten();
                        let status =
                            super::views::format_runtime_phase(runtime.as_ref().map(|r| &r.phase));
                        format!(
                            "🤖 Selected Bot:\n• Exchange: {}\n• Name: {}\n• ID: {}\n• Strategy: {}\n• Runtime: {}\n• Status: {}",
                            b.exchange.as_str().to_uppercase(),
                            b.name,
                            b.id,
                            strategy,
                            super::views::format_bot_runtime(b.runtime),
                            status
                        )
                    } else {
                        format!("🤖 Selected Bot: {bot_id}")
                    };

                    format!(
                        "👋 Welcome! Choose an action from the menu below.\n\n{bot_info}"
                    )
                } else {
                    "👋 Welcome! Choose an action from the menu below.\n\n\
                    🤖 No bot selected"
                        .to_string()
                };

                bot.send_message(msg.chat.id, welcome_msg)
                    .reply_markup(keyboards::main_menu_keyboard())
                    .await?;
            }
            Command::Runtime(args) => {
                let user_id = msg
                    .from()
                    .map(|user| user.id.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                let text = runtime_command(&deps, &user_id, &args).await;
                bot.send_message(msg.chat.id, text)
                    .reply_markup(keyboards::main_menu_keyboard())
                    .await?;
            }
            Command::List => {
                // Get user_id from telegram message
                let user_id = msg
                    .from()
                    .map(|user| user.id.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                // Call use case to get bots
                match deps.list_bots_usecase.execute(&user_id).await {
                    Ok(bots) => {
                        if bots.is_empty() {
                            bot.send_message(
                                msg.chat.id,
                                "📋 Your bots:\n\n(No bots configured yet)",
                            )
                            .await?;
                        } else {
                            let ctx = bot_context.get().await?.unwrap_or_default();

                            let header = if let Some(ref bot_id) = ctx.selected_bot_id {
                                let selected = bots.iter()
                                    .find(|b| &b.id == bot_id)
                                    .map(|b| format!(
                                        "{} | {} | {}",
                                        b.exchange.as_str().to_uppercase(),
                                        b.name,
                                        b.id
                                    ))
                                    .unwrap_or_else(|| bot_id.clone());
                                format!("📋 Select a bot:\n\n✅ Currently selected: {selected}")
                            } else {
                                "📋 Select a bot:\n\n(No bot selected)".to_string()
                            };

                            let augmented = super::bots_with_phase(&deps, &user_id, bots).await;
                            bot.send_message(msg.chat.id, header)
                                .reply_markup(keyboards::bot_list_keyboard(&augmented))
                                .await?;
                        }
                    }
                    Err(e) => {
                        bot.send_message(msg.chat.id, redact("fetching bots", &e))
                            .await?;
                    }
                }
            }
        }
        anyhow::Ok(())
    }
    .await;

    result.map_err(|_| DependencyMap::new())
}

const RUNTIME_USAGE: &str = "Usage: /runtime <bot_id> [py|rs]\n\n\
    • py — passivbot (Python), the default\n\
    • rs — pb-runner (Rust)\n\n\
    The bot must be one of yours (see /list). A change applies on the next 'Run bot'.";

/// `/runtime` handler body. The bot is looked up under the caller's own
/// Telegram id, so a user can only ever read or move their own bots.
async fn runtime_command(deps: &Deps, user_id: &str, args: &str) -> String {
    let mut words = args.split_whitespace();
    let Some(bot_id) = words.next() else {
        return RUNTIME_USAGE.to_string();
    };
    let runtime = words.next();
    if words.next().is_some() {
        return RUNTIME_USAGE.to_string();
    }

    let Some(runtime) = runtime else {
        return match deps.list_bots_usecase.execute(user_id).await {
            Ok(bots) => match bots.iter().find(|b| b.id == bot_id) {
                Some(b) => format!(
                    "⚙️ Bot {bot_id} runtime: {}",
                    super::views::format_bot_runtime(b.runtime)
                ),
                None => format!("❌ Bot {bot_id} not found."),
            },
            Err(e) => redact("fetching bots", &e),
        };
    };
    let Ok(runtime) = runtime.parse::<Runtime>() else {
        return RUNTIME_USAGE.to_string();
    };

    match deps
        .set_bot_runtime_usecase
        .execute(user_id, bot_id, runtime)
        .await
    {
        Ok(SetRuntimeOutcome::Updated { previous, runtime }) if previous == runtime => format!(
            "⚙️ Bot {bot_id} already runs on {}.",
            super::views::format_bot_runtime(runtime)
        ),
        Ok(SetRuntimeOutcome::Updated { previous, runtime }) => format!(
            "⚙️ Bot {bot_id} runtime: {previous} → {}\n\n\
            ⚠️ Applies on the next 'Run bot'. A running task keeps its current image \
            until it is stopped and started again.",
            super::views::format_bot_runtime(runtime)
        ),
        Ok(SetRuntimeOutcome::BotNotFound) => format!("❌ Bot {bot_id} not found."),
        Err(e) => redact("setting the runtime", &e),
    }
}
