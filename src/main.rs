mod config;
mod domain;
mod infra;
mod interface;

use teloxide::{prelude::*};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Starting Telegram bot...");

    // 初始化日志（可换成 tracing/subscriber 等）
    env_logger::init();

    // 从环境变量 TELEGRAM_BOT_TOKEN 读取 token（或从你的 config 层注入）
    let bot = Bot::from_env();

    // 构造你的依赖（domain/services/repos 等），此处仅示例
    let deps = interface::telegram::Deps::default();

    interface::telegram::router::run(bot, deps).await
}
