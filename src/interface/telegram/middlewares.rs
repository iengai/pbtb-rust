// Rust
use teloxide::prelude::*;
use teloxide::types::UpdateKind;

use super::{Deps, redaction::redact};
use crate::usecase::{SenderResolution, TelegramSender};

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

/// What became of resolving an update's sender, carried in the dependency map
/// for the branches after it.
#[derive(Debug, Clone)]
pub enum Sender {
    Known(TelegramSender),
    Unbound,
    Suspended,
    /// A channel post, an edited-message service update: nobody sent it.
    Nobody,
    /// The lookup itself failed. Redacted for the chat, logged in full.
    Unavailable(String),
}

/// Resolves the sender of every update into the account behind it.
///
/// Routed ahead of the command, callback and dialogue branches, which extract
/// [`TelegramSender`] and never see an update whose sender is not a known,
/// active account. This is the only place a Telegram id is turned into a
/// `user_id`, so no handler has a fallback tenant of its own.
pub fn resolve_sender() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry().map_async(|u: Update, deps: Deps| async move {
        let Some(telegram_id) = u.user().map(|user| user.id.0.to_string()) else {
            return Sender::Nobody;
        };
        match deps.resolve_sender_usecase.execute(&telegram_id).await {
            Ok(SenderResolution::Known(sender)) => Sender::Known(sender),
            Ok(SenderResolution::Unbound) => Sender::Unbound,
            Ok(SenderResolution::Suspended) => Sender::Suspended,
            Err(e) => Sender::Unavailable(redact("checking your account", &e)),
        }
    })
}

/// The gate the handler branches sit behind: only a known sender passes, and
/// what passes is the account, not the Telegram id.
pub fn known_sender() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::filter_map(|sender: Sender| match sender {
        Sender::Known(sender) => Some(sender),
        _ => None,
    })
}

/// Terminates every update the gate did not pass, telling the sender why.
///
/// Answering costs one API call but keeps a refusal distinguishable from the
/// bot being down; a silent drop reproduces exactly the "telebot never answers"
/// symptom. Nothing about any tenant is disclosed.
pub fn refuse_unresolved(site_url: String) -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::endpoint(move |bot: Bot, u: Update, sender: Sender| {
        let site_url = site_url.clone();
        async move {
            log::warn!(
                "refused update {} from user={:?} chat={:?}: {}",
                u.id,
                u.user().map(|user| user.id.0),
                u.chat().map(|c| c.id.0),
                match &sender {
                    Sender::Unbound => "no account bound to this telegram id",
                    Sender::Suspended => "account suspended",
                    Sender::Nobody => "no sender",
                    Sender::Unavailable(_) => "sender lookup failed",
                    Sender::Known(_) => "unreachable: a known sender was routed",
                }
            );
            let text = match sender {
                Sender::Unbound | Sender::Nobody => onboarding_text(&site_url),
                Sender::Suspended => "⛔ This account is suspended.".to_string(),
                Sender::Unavailable(text) => text,
                Sender::Known(_) => return Ok(()),
            };
            if let Some(chat) = u.chat()
                && let Err(e) = bot.send_message(chat.id, text).await
            {
                log::warn!("failed to deliver refusal notice: {e}");
            }
            Ok::<(), DependencyMap>(())
        }
    })
}

/// Where a stranger is sent. Signing up is a web action, so the bot can only
/// point at it.
fn onboarding_text(site_url: &str) -> String {
    let site = site_url.trim();
    if site.is_empty() {
        "👋 This Telegram account is not bound to any account. Sign up on the web with \
         Google, then bind Telegram from your account page."
            .to_string()
    } else {
        format!(
            "👋 This Telegram account is not bound to any account. Sign up at {site} with \
             Google, then bind Telegram from your account page there."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_names_the_site_when_there_is_one() {
        assert!(onboarding_text("https://example.test/").contains("https://example.test/"));
        assert!(onboarding_text("  ").contains("Sign up on the web"));
    }
}
