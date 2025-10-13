use teloxide::prelude::*;
use teloxide::dispatching::dialogue::{InMemStorage, Dialogue};

use super::{Deps, states::{DialogueState, BotContext}};

type MyDialogue = Dialogue<DialogueState, InMemStorage<DialogueState>>;
type MyBotContext = Dialogue<BotContext, InMemStorage<BotContext>>;

pub fn routes() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry()
        .branch(
            Update::filter_message()
                .enter_dialogue::<Message, InMemStorage<DialogueState>, DialogueState>()
                .enter_dialogue::<Message, InMemStorage<BotContext>, BotContext>()
                .branch(dptree::case![DialogueState::Start].endpoint(handle_start_state))
                .branch(dptree::case![DialogueState::ReceiveBotName].endpoint(receive_bot_name))
                .branch(dptree::case![DialogueState::ReceiveApiKey { name }].endpoint(receive_api_key))
                .branch(dptree::case![DialogueState::ReceiveSecretKey { name, api_key }].endpoint(receive_secret_key))
                .branch(dptree::case![DialogueState::ConfirmDelete { bot_id }].endpoint(confirm_delete))
        )
}

async fn handle_start_state(
    bot: Bot,
    dialogue: MyDialogue,
    bot_context: MyBotContext,
    msg: Message,
    deps: Deps,
) -> Result<(), DependencyMap> {
    let result = async {
        let text = match msg.text() {
            Some(t) => t,
            None => return Ok(()),
        };

        // Handle keyboard button text
        match text {
            "State" => {
                let ctx = bot_context.get().await?
                    .unwrap_or_default();

                let bot_info = if let Some(ref bot_id) = ctx.selected_bot_id {
                    format!("📊 Bot State: Idle\n🤖 Selected Bot: {}", bot_id)
                } else {
                    "📊 Bot State: Idle\n🤖 No bot selected".to_string()
                };

                bot.send_message(msg.chat.id, bot_info)
                    .await?;
            }
            "Balance" => {
                bot.send_message(msg.chat.id, "💰 Balance: $0.00")
                    .await?;
            }
            "Add bot" => {
                bot.send_message(msg.chat.id, "🤖 Let's add a new bot!\n\nPlease enter the bot name:")
                    .await?;
                dialogue.update(DialogueState::ReceiveBotName).await?;
            }
            "Choose config..." => {
                bot.send_message(msg.chat.id, "⚙️ Choose config... (Feature coming soon)")
                    .await?;
            }
            "Risk level" => {
                bot.send_message(msg.chat.id, "⚠️ Risk Level: Medium")
                    .await?;
            }
            "Run bot" => {
                let ctx = bot_context.get().await?
                    .unwrap_or_default();

                if let Some(ref bot_id) = ctx.selected_bot_id {
                    bot.send_message(msg.chat.id, format!("▶️ Starting bot {}...", bot_id))
                        .await?;
                } else {
                    bot.send_message(msg.chat.id, "❌ Please select a bot first using 'List'")
                        .await?;
                }
            }
            "Stop bot" => {
                let ctx = bot_context.get().await?
                    .unwrap_or_default();

                if let Some(ref bot_id) = ctx.selected_bot_id {
                    bot.send_message(msg.chat.id, format!("⏹️ Stopping bot {}...", bot_id))
                        .await?;
                } else {
                    bot.send_message(msg.chat.id, "❌ Please select a bot first using 'List'")
                        .await?;
                }
            }
            "Unstuck" => {
                bot.send_message(msg.chat.id, "🔧 Unstuck operation... (Feature coming soon)")
                    .await?;
            }
            "Delete API key" => {
                let ctx = bot_context.get().await?
                    .unwrap_or_default();

                if let Some(ref bot_id) = ctx.selected_bot_id {
                    bot.send_message(
                        msg.chat.id,
                        format!(
                            "⚠️ Are you sure you want to delete this bot?\n\n\
                            🤖 Bot ID: {}\n\n\
                            ❗ This action cannot be undone!\n\n\
                            Reply 'yes' to confirm or any other message to cancel.",
                            bot_id
                        )
                    )
                        .await?;

                    dialogue.update(DialogueState::ConfirmDelete {
                        bot_id: bot_id.clone()
                    }).await?;
                } else {
                    bot.send_message(
                        msg.chat.id,
                        "❌ No bot selected. Please use 'List' to select a bot first."
                    )
                        .await?;
                }
            }
            "List" => {
                // Get user_id from telegram message
                let user_id = msg.from()
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
                            let ctx = bot_context.get().await?
                                .unwrap_or_default();

                            let header = if let Some(ref bot_id) = ctx.selected_bot_id {
                                format!("📋 Select a bot:\n\n✅ Currently selected: {}", bot_id)
                            } else {
                                "📋 Select a bot:\n\n(No bot selected)".to_string()
                            };

                            bot.send_message(msg.chat.id, header)
                                .reply_markup(super::keyboards::bot_list_keyboard(&bots))
                                .await?;
                        }
                    }
                    Err(e) => {
                        bot.send_message(
                            msg.chat.id,
                            format!("❌ Error fetching bots: {}", e),
                        )
                            .await?;
                    }
                }
            }
            _ => {
                // ignore unknown text
            }
        }

        anyhow::Ok(())
    }.await;

    result.map_err(|_| DependencyMap::new())
}

async fn receive_bot_name(
    bot: Bot,
    dialogue: MyDialogue,
    _bot_context: MyBotContext,
    msg: Message,
) -> Result<(), DependencyMap> {
    let result = async {
        match msg.text() {
            Some(name) => {
                bot.send_message(msg.chat.id, format!("✅ Bot name: {}\n\nNow, please enter the API key:", name))
                    .await?;
                dialogue.update(DialogueState::ReceiveApiKey {
                    name: name.to_string(),
                }).await?;
            }
            None => {
                bot.send_message(msg.chat.id, "❌ Please send text for bot name.")
                    .await?;
            }
        }
        anyhow::Ok(())
    }.await;

    result.map_err(|_| DependencyMap::new())
}

async fn receive_api_key(
    bot: Bot,
    dialogue: MyDialogue,
    _bot_context: MyBotContext,
    name: String,
    msg: Message,
) -> Result<(), DependencyMap> {
    let result = async {
        match msg.text() {
            Some(api_key) => {
                bot.send_message(msg.chat.id, "✅ API key received!\n\nFinally, please enter the secret key:")
                    .await?;
                dialogue.update(DialogueState::ReceiveSecretKey {
                    name,
                    api_key: api_key.to_string(),
                }).await?;
            }
            None => {
                bot.send_message(msg.chat.id, "❌ Please send text for API key.")
                    .await?;
            }
        }
        anyhow::Ok(())
    }.await;

    result.map_err(|_| DependencyMap::new())
}

async fn receive_secret_key(
    bot: Bot,
    dialogue: MyDialogue,
    _bot_context: MyBotContext,
    (name, api_key): (String, String),
    msg: Message,
    deps: Deps,
) -> Result<(), DependencyMap> {
    let result = async {
        match msg.text() {
            Some(secret_key) => {
                let user_id = msg.from()
                    .map(|user| user.id.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                // Save bot using use case
                match deps.add_bot_usecase.execute(
                    &user_id,
                    name.clone(),
                    api_key,
                    secret_key.to_string(),
                ).await {
                    Ok(new_bot) => {
                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "✅ Bot added successfully!\n\n\
                                📝 Name: {}\n\
                                🆔 ID: {}\n\
                                ⏸️ Status: Disabled (default)\n\n\
                                You can enable it later.",
                                new_bot.name,
                                new_bot.id
                            ),
                        )
                            .await?;
                    }
                    Err(e) => {
                        bot.send_message(
                            msg.chat.id,
                            format!("❌ Error saving bot: {}", e),
                        )
                            .await?;
                    }
                }

                // Reset dialogue to start
                dialogue.update(DialogueState::Start).await?;
            }
            None => {
                bot.send_message(msg.chat.id, "❌ Please send text for secret key.")
                    .await?;
            }
        }
        anyhow::Ok(())
    }.await;

    result.map_err(|_| DependencyMap::new())
}

async fn confirm_delete(
    bot: Bot,
    dialogue: MyDialogue,
    bot_context: MyBotContext,
    bot_id: String,
    msg: Message,
    deps: Deps,
) -> Result<(), DependencyMap> {
    let result = async {
        match msg.text() {
            Some(text) => {
                if text.trim().eq_ignore_ascii_case("yes") {
                    // User confirmed deletion
                    let user_id = msg.from()
                        .map(|user| user.id.to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    match deps.delete_bot_usecase.execute(&user_id, &bot_id).await {
                        Ok(_) => {
                            // Clear the selected bot from context
                            bot_context.update(BotContext {
                                selected_bot_id: None,
                            }).await?;

                            bot.send_message(
                                msg.chat.id,
                                format!("✅ Bot deleted successfully!\n\n🤖 Bot ID: {}", bot_id),
                            )
                                .await?;
                        }
                        Err(e) => {
                            bot.send_message(
                                msg.chat.id,
                                format!("❌ Error deleting bot: {}", e),
                            )
                                .await?;
                        }
                    }
                } else {
                    // User cancelled
                    bot.send_message(
                        msg.chat.id,
                        "🚫 Deletion cancelled.",
                    )
                        .await?;
                }

                // Reset dialogue to start
                dialogue.update(DialogueState::Start).await?;
            }
            None => {
                bot.send_message(msg.chat.id, "❌ Please send text to confirm.")
                    .await?;
            }
        }
        anyhow::Ok(())
    }.await;

    result.map_err(|_| DependencyMap::new())
}