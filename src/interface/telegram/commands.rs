// Rust
use teloxide::dispatching::dialogue::{Dialogue, InMemStorage};
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;

use super::{
    Deps, keyboards,
    states::{BotContext, DialogueState},
};

type MyDialogue = Dialogue<DialogueState, InMemStorage<DialogueState>>;
type MyBotContext = Dialogue<BotContext, InMemStorage<BotContext>>;

#[derive(BotCommands, Clone)]
#[command(description = "Available commands", rename_rule = "lowercase")]
pub enum Command {
    #[command(description = "Start the bot")]
    Start,
    #[command(description = "list bots")]
    List,
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
                            "🤖 Selected Bot:\n• Exchange: {}\n• Name: {}\n• ID: {}\n• Strategy: {}\n• Status: {}",
                            b.exchange.as_str().to_uppercase(),
                            b.name,
                            b.id,
                            strategy,
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
                        bot.send_message(msg.chat.id, format!("❌ Error fetching bots: {e}"))
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
