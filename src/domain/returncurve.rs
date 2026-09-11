//! A bot's return series, as the daily collector writes it, read back for
//! the account that owns the bot.

use crate::domain::error::DomainError;
use async_trait::async_trait;
use serde_json::Value;

/// Read access to a bot's return series, keyed by tenant and bot.
///
/// The series is the collector's artifact — a return index and the realized
/// PnL, never a balance — and nothing here interprets it: it is handed to the
/// owner as the JSON it was stored as, so the collector's shape can grow
/// without a domain type chasing it.
#[async_trait]
pub trait ReturnCurveRepository: Send + Sync {
    /// `Ok(None)` when the collector has not written a series for this bot
    /// yet — a bot that has not traded, or was added since the last run.
    async fn get(&self, user_id: &str, bot_id: &str) -> Result<Option<Value>, DomainError>;
}
