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
        // Command::Start => on_start(bot, msg, deps).await?,
        Command::Start => {
            bot.send_message(
                msg.chat.id,
                "👋 Welcome! Choose an action from the menu below.",
            )
                .reply_markup(keyboards::main_menu_keyboard())
                .await?;
        }
        Command::List => {
            // TODO: Implement listing all bots functionality
            bot.send_message(
                msg.chat.id,
                "📋 Your bots:\n\n(No bots configured yet)",
            )
                .await?;
        }
    }
    Ok(())
}