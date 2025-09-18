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
    if let Some(data) = &q.data {
        match types::CallbackData::decode(data) {
            Ok(types::CallbackData::Action(action)) => match action {
                types::CallbackAction::Hello => {
                    if let Some(msg) = q.message.as_ref() {
                        bot.edit_message_text(msg.chat.id, msg.id, views::hello_text())
                            .reply_markup(keyboards::main_menu())
                            .await?;
                    }
                }
            },
            Ok(_) => { /* 其它类型按需处理 */ }
            Err(_) => { /* 忽略无法解析的回调数据，或记录日志 */ }
        }
    }
    // 及时答复 callback（可选显示/隐藏 toast）
    bot.answer_callback_query(q.id.clone()).await.ok();
    Ok(())
}