// Rust
use teloxide::prelude::*;
use teloxide::types::UpdateKind;

// 可在此添加鉴权、节流、统一错误拦截、日志上下文等横切逻辑。
// 返回一个可链式组合的 Handler。

/// Records every update that reaches the dispatcher, before any routing.
///
/// This is the only place that can tell "the bot never received it" apart from
/// "the bot received it and did nothing": teloxide itself logs an update only
/// when nothing in the handler tree matches it.
///
/// Message text is deliberately never logged: the add-bot dialogue receives
/// exchange API keys and secret keys as ordinary message text. Callback data is
/// safe, being strings this crate itself puts on the buttons.
pub fn install() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry().chain(dptree::inspect(|u: Update| {
        let chat = u.chat().map(|c| c.id.0);
        match &u.kind {
            UpdateKind::Message(m) => log::info!(
                "update {} Message chat={chat:?} text_len={}",
                u.id,
                m.text().map(str::len).unwrap_or(0)
            ),
            UpdateKind::CallbackQuery(q) => log::info!(
                "update {} CallbackQuery chat={chat:?} data={}",
                u.id,
                q.data.as_deref().unwrap_or("")
            ),
            _ => log::info!("update {} (other kind) chat={chat:?}", u.id),
        }
    }))
}
