//! `/start <token>`: the bot's half of binding a Telegram account.
//!
//! Routed ahead of sender resolution, because the sender is by definition not
//! yet known to the bot. The token was minted on the web for a signed-in
//! account and carried here by the deep link Telegram turns into this command;
//! the sender it arrives from is the Telegram id the account ends up bound to.
use teloxide::prelude::*;

use super::{Deps, keyboards, redaction::redact};
use crate::usecase::BindOutcome;

/// A bind token lifted from a `/start` command, so the endpoint only ever runs
/// for a message that carries one. A bare `/start` stays a normal command.
#[derive(Debug, Clone)]
pub struct StartToken(pub String);

pub fn routes() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry().branch(
        Update::filter_message()
            .filter_map(|msg: Message| msg.text().and_then(start_token).map(StartToken))
            .endpoint(
                |bot: Bot, msg: Message, token: StartToken, deps: Deps| async move {
                    super::with_deadline("bind_telegram", bind(bot, msg, token, deps))
                        .await
                        .map_err(|_| DependencyMap::new())
                },
            ),
    )
}

/// The payload of `/start <payload>` (or `/start@bot <payload>`), if any.
fn start_token(text: &str) -> Option<String> {
    let rest = text.strip_prefix("/start")?;
    let rest = match rest.chars().next() {
        Some('@') => rest.split_once(char::is_whitespace)?.1,
        Some(c) if c.is_whitespace() => rest,
        _ => return None,
    };
    let token = rest.trim();
    // Only what the web hands out: a hex token. Anything else is a `/start`
    // someone typed by hand, and gets the welcome instead of a redeem attempt.
    (!token.is_empty() && token.chars().all(|c| c.is_ascii_hexdigit())).then(|| token.to_string())
}

async fn bind(bot: Bot, msg: Message, token: StartToken, deps: Deps) -> anyhow::Result<()> {
    // A group would bind the first member to tap the link, not the person who
    // was handed it.
    if !msg.chat.is_private() {
        bot.send_message(
            msg.chat.id,
            "🔗 Open the bind link in a private chat with me.",
        )
        .await?;
        return Ok(());
    }
    let Some(sender) = msg.from() else {
        return Ok(());
    };
    let telegram_id = sender.id.to_string();

    let text = match deps
        .bind_telegram_usecase
        .execute(&token.0, &telegram_id)
        .await
    {
        Ok(BindOutcome::Bound { .. }) => {
            "✅ Telegram is now bound to your account. Choose an action from the menu below."
                .to_string()
        }
        Ok(BindOutcome::AlreadyBound { .. }) => {
            "✅ This Telegram account is already bound to your account.".to_string()
        }
        Ok(BindOutcome::InvalidTicket) => {
            "❌ This bind link is invalid, expired or already used. Get a fresh one from \
             your account page on the web."
                .to_string()
        }
        Ok(BindOutcome::TelegramTakenByAnother) => {
            "❌ This Telegram account is bound to a different account. Unbind it there \
             first."
                .to_string()
        }
        Ok(BindOutcome::AccountAlreadyBound) => {
            "❌ Your account already has a Telegram account bound. Unbind it from your \
             account page on the web, then get a fresh link."
                .to_string()
        }
        Err(e) => redact("binding your Telegram account", &e),
    };
    let reply = bot.send_message(msg.chat.id, text);
    match bound_now(&deps, &telegram_id).await {
        true => reply.reply_markup(keyboards::main_menu_keyboard()).await?,
        false => reply.await?,
    };
    Ok(())
}

/// Whether the sender can drive the bot after this exchange, so the menu is
/// offered only to someone whose next tap will be answered.
async fn bound_now(deps: &Deps, telegram_id: &str) -> bool {
    matches!(
        deps.resolve_sender_usecase.execute(telegram_id).await,
        Ok(crate::usecase::SenderResolution::Known(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_start_with_a_hex_payload_is_a_bind() {
        assert_eq!(start_token("/start abc123").as_deref(), Some("abc123"));
        assert_eq!(
            start_token("/start@pbtb_bot abc123").as_deref(),
            Some("abc123")
        );
        assert_eq!(start_token("/start   abc123  ").as_deref(), Some("abc123"));
        assert_eq!(start_token("/start"), None);
        assert_eq!(start_token("/start@pbtb_bot"), None);
        assert_eq!(start_token("/start hello world"), None);
        assert_eq!(start_token("/started abc"), None);
        assert_eq!(start_token("/list"), None);
    }
}
