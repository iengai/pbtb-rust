// Rust
use teloxide::prelude::*;
use teloxide::types::CallbackQuery;

use super::{types, views, Deps, keyboards};

pub fn routes() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry()
        .branch(
            Update::filter_callback_query()
                .endpoint(|bot: Bot, q: CallbackQuery, deps: Deps| async move {
                    super::callbacks::handle_callback(bot, q, deps)
                        .await
                        .map_err(|_e| DependencyMap::new())
                }),
        )
}

async fn handle_callback(bot: Bot, q: CallbackQuery, _deps: Deps) -> anyhow::Result<()> {
    let data = q.data.as_ref().map(|s| s.as_str()).unwrap_or("");
    
    bot.answer_callback_query(q.id.clone()).await.ok();
    
    let callback = types::CallbackData::decode(data);

    match callback {
        types::CallbackData::Action(action) => {
            handle_action(bot, q, action).await?;
        }
        types::CallbackData::Unknown => {
            // 未知的 callback
            if let Some(message) = q.message {
                bot.send_message(message.chat.id, "⚠️ Unknown action")
                    .await?;
            }
        }
    }

    Ok(())
}


async fn handle_action(
    bot: Bot,
    q: CallbackQuery,
    action: types::CallbackAction,
) -> anyhow::Result<()> {
    let message = match q.message {
        Some(msg) => msg,
        None => return Ok(()),
    };

    match action {
        types::CallbackAction::Hello => {
            bot.send_message(message.chat.id, "👋 Hello! How can I help you?")
                .await?;
        }
    }

    Ok(())
}