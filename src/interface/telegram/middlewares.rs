// Rust
use std::collections::HashSet;
use teloxide::prelude::*;
use teloxide::types::{UpdateKind, UserId};

// 可在此添加节流、统一错误拦截、日志上下文等横切逻辑。
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

/// Terminates every update whose sender is not on the allowlist, before it can
/// reach a command, callback or dialogue handler.
///
/// Routed as the first branch of the schema, so the handlers behind it never see
/// an unauthorized update and need no check of their own. It also closes the
/// `"unknown"` tenant bucket that the handlers' `user_id` fallback would
/// otherwise create: an update carrying no sender matches no id, so it stops
/// here.
pub fn reject_unauthorized(
    allowed: HashSet<String>,
) -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::filter(move |u: Update| !is_allowed(&allowed, u.user().map(|user| user.id))).endpoint(
        |bot: Bot, u: Update| async move {
            log::warn!(
                "rejected update {} from user={:?} chat={:?}: not on the allowlist",
                u.id,
                u.user().map(|user| user.id.0),
                u.chat().map(|c| c.id.0)
            );
            // Answering costs one API call but keeps a rejection distinguishable
            // from the bot being down; a silent drop reproduces exactly the
            // "telebot never answers" symptom.
            if let Some(chat) = u.chat()
                && let Err(e) = bot
                    .send_message(chat.id, "⛔ This bot is limited to authorized users.")
                    .await
            {
                log::warn!("failed to deliver rejection notice: {e}");
            }
            Ok::<(), DependencyMap>(())
        },
    )
}

/// Whether an update's sender may use the bot. A sender-less update (a channel
/// post, an edited-message service update) belongs to nobody on the list.
fn is_allowed(allowed: &HashSet<String>, user: Option<UserId>) -> bool {
    user.is_some_and(|id| allowed.contains(&id.0.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowlist(ids: &str) -> HashSet<String> {
        ids.split(',').map(str::to_owned).collect()
    }

    #[test]
    fn allowlisted_sender_passes() {
        assert!(is_allowed(
            &allowlist("5351347639"),
            Some(UserId(5351347639))
        ));
    }

    #[test]
    fn other_sender_is_rejected() {
        assert!(!is_allowed(&allowlist("5351347639"), Some(UserId(42))));
    }

    #[test]
    fn update_without_sender_is_rejected() {
        assert!(!is_allowed(&allowlist("5351347639"), None));
    }
}
