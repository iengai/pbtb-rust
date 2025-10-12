// Rust
use teloxide::prelude::*;

use super::Deps;

pub fn routes() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry()
        .branch(
            Update::filter_message()
                .endpoint(|bot: Bot, msg: Message, _deps: Deps| async move {
                    handle_text_message(bot, msg)
                        .await
                        .map_err(|_e| DependencyMap::new())
                }),
        )
}

async fn handle_text_message(
    bot: Bot,
    msg: Message,
) -> anyhow::Result<()> {
    let text = match msg.text() {
        Some(t) => t,
        None => return Ok(()),
    };
    
    // 处理键盘按钮文本
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
            bot.send_message(msg.chat.id, "📋 Bot list... (Feature coming soon)")
                .await?;
        }
        _ => {
            // ignore unknown text
        }
    }
    
    Ok(())
}
