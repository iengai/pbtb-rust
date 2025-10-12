// Rust
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;

use super::{keyboards, views, Deps};

#[derive(BotCommands, Clone)]
#[command(description = "Available commands", rename_rule = "lowercase")]
pub enum Command {
    #[command(description = "Start the bot")]
    Start,
    #[command(description = "list bots")]
    List,
}

pub fn routes() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry()
        .branch(
            Update::filter_message()
                .filter_command::<Command>()
                .endpoint(|bot: Bot, msg: Message, cmd: Command, deps: Deps| async move {
                    // Convert ResponseResult<()> to Result<(), DependencyMap>
                    super::commands::dispatch_command(bot, msg, cmd, deps)
                        .await
                        .map_err(|_e| DependencyMap::new())
                }),
        )
}

async fn dispatch_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    deps: super::Deps,
) -> anyhow::Result<()> {
    match cmd {
        Command::Start => {
            bot.send_message(
                msg.chat.id,
                "👋 Welcome! Choose an action from the menu below.",
            )
                .reply_markup(keyboards::main_menu_keyboard())
                .await?;
        }
        Command::List => {
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