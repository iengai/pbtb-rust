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
use crate::usecase::{SetPublicUrlOutcome, SetRuntimeOutcome, TelegramSender};

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
    /// `/public <bot_id> <url>` gives the bot its Bybit copy-trading link,
    /// which marks it for the public showcase page; `/public <bot_id> off`
    /// clears the mark; `/public <bot_id>` shows it. Setting is the
    /// operator's account only.
    #[command(description = "show or set a bot's public link: /public <bot_id> [<bybit url>|off]")]
    Public(String),
    /// Releases the caller's Telegram id from their account, so another can be
    /// bound from the web. The account itself is untouched.
    #[command(description = "unbind this Telegram account from your account")]
    Unlink,
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
    sender: TelegramSender,
    dialogue: MyDialogue,
    bot_context: MyBotContext,
) -> Result<(), DependencyMap> {
    let result = super::with_deadline("dispatch_command", async {
        match cmd {
            Command::Start => {
                // Reset dialogue state to Start (clears any ongoing conversation)
                dialogue.update(DialogueState::Start).await?;

                // Get current bot context to show selected bot info
                let ctx = bot_context.get().await?.unwrap_or_default();

                let welcome_msg = if let Some(ref bot_id) = ctx.selected_bot_id {
                    let user_id = sender.user_id.clone();

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
            Command::Unlink => {
                let text = match deps.unbind_telegram_usecase.execute(&sender.user_id).await {
                    Ok(0) => "🔗 No Telegram account is bound to your account.".to_string(),
                    Ok(_) => "🔗 Unbound. This Telegram account can no longer drive your \
                              bots; bind one again from your account page on the web."
                        .to_string(),
                    Err(e) => redact("unbinding your Telegram account", &e),
                };
                // No menu: the next tap from this sender would be refused.
                bot.send_message(msg.chat.id, text).await?;
            }
            Command::Runtime(args) => {
                let user_id = sender.user_id.clone();
                let text = runtime_command(&deps, &user_id, &args).await;
                bot.send_message(msg.chat.id, text)
                    .reply_markup(keyboards::main_menu_keyboard())
                    .await?;
            }
            Command::Public(args) => {
                let text = public_command(&deps, &sender, &args).await;
                bot.send_message(msg.chat.id, text)
                    .reply_markup(keyboards::main_menu_keyboard())
                    .await?;
            }
            Command::List => {
                // Get user_id from telegram message
                let user_id = sender.user_id.clone();

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
    })
    .await;

    result.map_err(|_| DependencyMap::new())
}

const RUNTIME_USAGE: &str = "Usage: /runtime <bot_id> [py|rs]\n\n\
    • py — passivbot (Python), the default\n\
    • rs — pb-runner (Rust)\n\n\
    The bot must be one of yours (see /list). A change applies on the next 'Run bot'.\n\
    The 'Runtime' menu button does the same thing for the selected bot.";

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
        return match super::find_own_bot(deps, user_id, bot_id).await {
            Ok(Some(b)) => format!(
                "⚙️ Bot {bot_id} runtime: {}",
                super::views::format_bot_runtime(b.runtime)
            ),
            Ok(None) => format!("❌ Bot {bot_id} not found."),
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
        Ok(SetRuntimeOutcome::Unchanged { runtime }) => format!(
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

const PUBLIC_USAGE: &str = "Usage: /public <bot_id> [<url>|off]\n\n\
    • <url> — the bot's Bybit copy-trading page (https://…bybit.com/…); marks the bot \
    for the public showcase page\n\
    • off — clears the mark\n\n\
    The bot must be one of yours (see /list). Setting the link is for the operator's account.";

/// `/public` handler body. The bot is looked up under the caller's own account,
/// so a user can only ever read or change their own bots; the use case refuses
/// a change from any account but the operator's.
async fn public_command(deps: &Deps, sender: &TelegramSender, args: &str) -> String {
    let mut words = args.split_whitespace();
    let Some(bot_id) = words.next() else {
        return PUBLIC_USAGE.to_string();
    };
    let value = words.next();
    if words.next().is_some() {
        return PUBLIC_USAGE.to_string();
    }

    let Some(value) = value else {
        return match super::find_own_bot(deps, &sender.user_id, bot_id).await {
            Ok(Some(b)) => match b.public_url {
                Some(url) => format!("🌐 Bot {bot_id} is public: {url}"),
                None => format!("🌐 Bot {bot_id} is private."),
            },
            Ok(None) => format!("❌ Bot {bot_id} not found."),
            Err(e) => redact("fetching bots", &e),
        };
    };
    let public_url = (!value.eq_ignore_ascii_case("off")).then(|| value.to_string());

    match deps
        .set_bot_public_url_usecase
        .execute(sender.role, &sender.user_id, bot_id, public_url)
        .await
    {
        Ok(SetPublicUrlOutcome::Unchanged {
            public_url: Some(url),
        }) => format!("🌐 Bot {bot_id} is already public: {url}"),
        Ok(SetPublicUrlOutcome::Unchanged { public_url: None }) => {
            format!("🌐 Bot {bot_id} is already private.")
        }
        Ok(SetPublicUrlOutcome::Updated {
            public_url: Some(url),
            ..
        }) => format!("🌐 Bot {bot_id} is now marked public: {url}"),
        Ok(SetPublicUrlOutcome::Updated {
            public_url: None, ..
        }) => format!("🌐 Bot {bot_id} is now private."),
        Ok(SetPublicUrlOutcome::BotNotFound) => format!("❌ Bot {bot_id} not found."),
        Err(e) => redact("setting the public link", &e),
    }
}
