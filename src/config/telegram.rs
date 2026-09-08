// src/config/telegram.rs
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TelegramConfig {
    /// The Telegram user ids allowed to reach any handler, comma-separated
    /// (`5351347639,12345678`). Every other update is answered with a refusal
    /// and dropped before routing — see
    /// `interface::telegram::middlewares::reject_unauthorized`. Parsed at
    /// process start by `parse_allowed_user_ids`, where an empty list fails
    /// startup rather than leaving the bot open to anyone who finds it.
    ///
    /// A `String` rather than a `Vec<String>` because `Environment` tries `i64`
    /// before its list separator: a single numeric id would arrive as an
    /// integer and refuse to deserialize into a list.
    ///
    /// Env: APP__TELEGRAM__ALLOWED_USER_IDS.
    pub allowed_user_ids: String,
}
