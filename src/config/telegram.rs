// src/config/telegram.rs
use anyhow::bail;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Debug, Deserialize)]
pub struct TelegramConfig {
    /// The Telegram user ids the MCP binaries may act as, comma-separated
    /// (`5351347639,12345678`); `APP__MCP__USER_ID` must be one of them. The
    /// bot itself does not read this: it resolves each sender through the
    /// `telegram` identity rows instead.
    ///
    /// A `String` rather than a `Vec<String>` because `Environment` tries `i64`
    /// before its list separator: a single numeric id would arrive as an
    /// integer and refuse to deserialize into a list.
    ///
    /// Env: APP__TELEGRAM__ALLOWED_USER_IDS.
    #[serde(default)]
    pub allowed_user_ids: String,

    /// The web console, where accounts are created and a Telegram id is bound.
    /// The bot points a sender it does not know there; empty leaves the
    /// directions generic.
    ///
    /// Env: APP__TELEGRAM__SITE_URL.
    #[serde(default)]
    pub site_url: String,

    /// The bot's `@username`, for the `https://t.me/<username>?start=<token>`
    /// deep link the web hands out to bind a Telegram account. Empty hands out
    /// the token alone.
    ///
    /// Env: APP__TELEGRAM__BOT_USERNAME.
    #[serde(default)]
    pub bot_username: String,
}

impl TelegramConfig {
    /// The allowlist as a set.
    ///
    /// An empty list fails startup instead of admitting everyone, the same way a
    /// bad engine table does (`EngineTaskDefinitions::parse`): a missing
    /// allowlist is a deploy mistake, and failing here makes it one loud failure
    /// rather than a bot silently open to whoever finds it — which would let a
    /// stranger create a tenant, bind exchange keys and run live tasks on this
    /// account's ECS capacity.
    pub fn allowlist(&self) -> anyhow::Result<HashSet<String>> {
        let ids: HashSet<String> = self
            .allowed_user_ids
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if ids.is_empty() {
            bail!(
                "allowlist is empty; set it to the comma-separated Telegram user ids that may use this bot"
            );
        }
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(ids: &str) -> TelegramConfig {
        TelegramConfig {
            allowed_user_ids: ids.to_string(),
            site_url: String::new(),
            bot_username: String::new(),
        }
    }

    #[test]
    fn ids_are_split_and_trimmed() {
        assert_eq!(
            config(" 1 , 2,3 ,").allowlist().expect("non-empty"),
            HashSet::from(["1".into(), "2".into(), "3".into()])
        );
    }

    #[test]
    fn a_blank_allowlist_is_a_startup_error() {
        assert!(config("").allowlist().is_err());
        assert!(config(" , ").allowlist().is_err());
    }
}
