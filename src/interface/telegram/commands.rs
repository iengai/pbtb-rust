// Rust
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;

use super::{keyboards, views, Deps};

#[derive(BotCommands, Clone)]
#[command(description = "Available commands", rename_rule = "lowercase")]
pub enum Command {
    #[command(description = "Start the bot")]
    Start,
    #[command(description = "Show help")]
    Help,
}

pub fn routes() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry()
        .branch(
            Update::filter_message()
                .filter_command::<Command>()
                .endpoint(|bot: Bot, msg: Message, cmd: Command, deps: Deps| async move {
                    // 将 ResponseResult<()> 转换为 Result<(), DependencyMap>
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
        Command::Start => on_start(bot, msg, deps).await?,
        Command::Help => on_help(bot, msg).await?,
    }
    Ok(())
}

async fn on_start(bot: Bot, msg: Message, _deps: Deps) -> anyhow::Result<()> {
    bot.send_message(
        msg.chat.id,
        views::welcome_text(), // 统一从 views 输出文案
    )
        .reply_markup(keyboards::main_menu())
        .await?;
    Ok(())
}

async fn on_help(bot: Bot, msg: Message) -> anyhow::Result<()> {
    bot.send_message(msg.chat.id, Command::descriptions().to_string())
        .await?;
    Ok(())
}