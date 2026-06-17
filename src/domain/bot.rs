use async_trait::async_trait;
use crate::domain::exchange::Exchange;
use crate::domain::error::DomainError;

#[derive(Debug, Clone)]
pub struct Bot {
    pub id: String,
    pub user_id: String,
    pub exchange: Exchange,
    pub name: String,
    pub api_key: String,
    pub secret_key: String,
    pub enabled: bool,
    pub created_at: i64,  // Unix timestamp in seconds
    pub updated_at: i64,  // Unix timestamp in seconds
}

impl Bot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        user_id: String,
        exchange: Exchange,
        name: String,
        api_key: String,
        secret_key: String,
        enabled: bool,
        created_at: i64,
        updated_at: i64,
    ) -> Self {
        Self {
            id,
            user_id,
            exchange,
            name,
            api_key,
            secret_key,
            enabled,
            created_at,
            updated_at,
        }
    }

    /// Factory encapsulating the construction policy for a newly added bot.
    /// id is derived from the name, exchange defaults to Bybit, and the bot
    /// starts disabled (desired state off).
    pub fn create(
        user_id: String,
        name: String,
        api_key: String,
        secret_key: String,
        now: i64,
    ) -> Self {
        Self {
            id: name.clone(),
            user_id,
            exchange: Exchange::Bybit,
            name,
            api_key,
            secret_key,
            enabled: false,
            created_at: now,
            updated_at: now,
        }
    }

    /// Desired-state transition: user turned the bot on.
    pub fn enable(&mut self, now: i64) {
        self.enabled = true;
        self.updated_at = now;
    }

    /// Desired-state transition: user turned the bot off.
    pub fn disable(&mut self, now: i64) {
        self.enabled = false;
        self.updated_at = now;
    }
}

#[async_trait]
pub trait BotRepository: Send + Sync {
    async fn find(&self, user_id: &str, bot_id: &str) -> Option<Bot>;
    async fn save(&self, bot: &Bot) -> Result<(), DomainError>;
    async fn find_by_user_id(&self, user_id: &str) -> Vec<Bot>;
    async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), String>;
}

/// Domain port for persisting a bot's exchange API keys (e.g. to object
/// storage). Use cases depend on this abstraction, not the concrete infra impl.
#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    async fn save(&self, bot: &Bot) -> Result<(), String>;
    async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_sets_defaults() {
        let bot = Bot::create("user-1".into(), "mybot".into(), "ak".into(), "sk".into(), 42);
        assert_eq!(bot.id, "mybot");
        assert_eq!(bot.name, "mybot");
        assert_eq!(bot.user_id, "user-1");
        assert_eq!(bot.exchange, Exchange::Bybit);
        assert!(!bot.enabled);
        assert_eq!(bot.created_at, 42);
        assert_eq!(bot.updated_at, 42);
    }

    #[test]
    fn enable_disable_transitions() {
        let mut bot = Bot::create("u".into(), "b".into(), "ak".into(), "sk".into(), 1);
        bot.enable(100);
        assert!(bot.enabled);
        assert_eq!(bot.updated_at, 100);
        bot.disable(200);
        assert!(!bot.enabled);
        assert_eq!(bot.updated_at, 200);
    }
}
