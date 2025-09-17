mod config;
mod domain;
mod infra;

use teloxide::{dispatching::UpdateFilterExt, prelude::*};

#[tokio::main]
async fn main() {
    println!("Starting Telegram bot...");

    let bot = Bot::from_env();

    let handler = Update::filter_message().endpoint(handle_message);

    Dispatcher::builder(bot, handler)
        .default_handler(|_| async {})
        .build()
        .dispatch()
        .await;
}

async fn handle_message(bot: Bot, msg: Message) -> ResponseResult<()> {
    if let Some(text) = msg.text() {
        let reply = format!("Echo: {}", text);
        bot.send_message(msg.chat.id, reply).await?;
    }

    Ok(())
}
