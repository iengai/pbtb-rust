// Rust
use teloxide::prelude::*;

use super::Deps;

pub fn routes() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry()
        .branch(
            Update::filter_message()
                .endpoint(|bot: Bot, msg: Message, deps: Deps| async move {
                    handle_text_message(bot, msg, deps)
                        .await
                        .map_err(|_e| DependencyMap::new())
                }),
        )
}
async fn handle_text_message(
    bot: Bot,
    msg: Message,
    deps: Deps,
) -> anyhow::Result<()> {
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
            bot.send_message(msg.chat.id, "🤖 Adding bot... (Feature coming soon)")
                .await?;
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

    Ok(())
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