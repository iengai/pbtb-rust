use teloxide::prelude::*;
use teloxide::dispatching::dialogue::{InMemStorage, Dialogue};

use super::{Deps, states::DialogueState};

type MyDialogue = Dialogue<DialogueState, InMemStorage<DialogueState>>;

pub fn routes() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry()
        .branch(
            Update::filter_message()
                .enter_dialogue::<Message, InMemStorage<DialogueState>, DialogueState>()
                .branch(dptree::case![DialogueState::Start].endpoint(handle_start_state))
                .branch(dptree::case![DialogueState::ReceiveBotName].endpoint(receive_bot_name))
                .branch(dptree::case![DialogueState::ReceiveApiKey { name }].endpoint(receive_api_key))
                .branch(dptree::case![DialogueState::ReceiveSecretKey { name, api_key }].endpoint(receive_secret_key))
        )
}

async fn handle_start_state(
    bot: Bot,
    dialogue: MyDialogue,
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
                bot.send_message(msg.chat.id, "📊 Bot State: Idle")
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
                bot.send_message(msg.chat.id, "▶️ Starting bot... (Feature coming soon)")
                    .await?;
            }
            "Stop bot" => {
                bot.send_message(msg.chat.id, "⏹️ Stopping bot... (Feature coming soon)")
                    .await?;
            }
            "Unstuck" => {
                bot.send_message(msg.chat.id, "🔧 Unstuck operation... (Feature coming soon)")
                    .await?;
            }
            "Delete API key" => {
                bot.send_message(msg.chat.id, "🗑️ Delete API key... (Feature coming soon)")
                    .await?;
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
                            let bot_list = format_bot_list(&bots);
                            bot.send_message(msg.chat.id, bot_list)
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

fn format_bot_list(bots: &[crate::domain::bot::Bot]) -> String {
    let mut message = String::from("📋 Your bots:\n\n");

    for (index, bot) in bots.iter().enumerate() {
        let status = if bot.enabled { "✅" } else { "⏸️" };
        message.push_str(&format!(
            "{}. {} {}\n   ID: {}\n\n",
            index + 1,
            status,
            bot.name,
            bot.id
        ));
    }

    message
}