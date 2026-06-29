use crate::domain::bot::{ApiKeyRepository, Bot, BotRepository};
use crate::domain::clock::Clock;
use std::sync::Arc;

pub struct AddBotUseCase {
    bot_repository: Arc<dyn BotRepository + Send + Sync>,
    api_keys_repository: Arc<dyn ApiKeyRepository>,
    clock: Arc<dyn Clock>,
}

impl AddBotUseCase {
    pub fn new(
        bot_repository: Arc<dyn BotRepository + Send + Sync>,
        api_keys_repository: Arc<dyn ApiKeyRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            bot_repository,
            api_keys_repository,
            clock,
        }
    }

    pub async fn execute(
        &self,
        user_id: &str,
        name: String,
        api_key: String,
        secret_key: String,
    ) -> Result<Bot, String> {
        let bot = Bot::create(
            user_id.to_string(),
            name,
            api_key.clone(),
            secret_key.clone(),
            self.clock.now(),
        );

        // Save to DynamoDB
        self.bot_repository
            .save(&bot)
            .await
            .map_err(|e| format!("Failed to save bot: {}", e))?;

        // Save API keys to S3 api-keys.json
        self.api_keys_repository
            .save(&bot)
            .await
            .map_err(|e| format!("Failed to save API keys to S3: {}", e))?;

        Ok(bot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::DomainError;
    use crate::domain::exchange::Exchange;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> i64 {
            1_700_000_000
        }
    }

    #[derive(Default)]
    struct InMemoryBots {
        bots: Mutex<HashMap<(String, String), Bot>>,
    }
    impl InMemoryBots {
        fn get(&self, user_id: &str, bot_id: &str) -> Option<Bot> {
            self.bots
                .lock()
                .unwrap()
                .get(&(user_id.to_string(), bot_id.to_string()))
                .cloned()
        }
    }
    #[async_trait]
    impl BotRepository for InMemoryBots {
        async fn find(&self, user_id: &str, bot_id: &str) -> Result<Option<Bot>, DomainError> {
            Ok(self.get(user_id, bot_id))
        }
        async fn save(&self, bot: &Bot) -> Result<(), DomainError> {
            self.bots
                .lock()
                .unwrap()
                .insert((bot.user_id.clone(), bot.id.clone()), bot.clone());
            Ok(())
        }
        async fn find_by_user_id(&self, user_id: &str) -> Result<Vec<Bot>, DomainError> {
            Ok(self
                .bots
                .lock()
                .unwrap()
                .values()
                .filter(|b| b.user_id == user_id)
                .cloned()
                .collect())
        }
        async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), String> {
            self.bots
                .lock()
                .unwrap()
                .remove(&(user_id.to_string(), bot_id.to_string()));
            Ok(())
        }
    }

    /// In-memory ApiKeyRepository whose save/delete always succeed, capturing
    /// the last saved bot so the test can exercise the full success path.
    #[derive(Default)]
    struct MockApiKeyRepository {
        saved: Mutex<Option<Bot>>,
    }
    #[async_trait]
    impl ApiKeyRepository for MockApiKeyRepository {
        async fn save(&self, bot: &Bot) -> Result<(), String> {
            *self.saved.lock().unwrap() = Some(bot.clone());
            Ok(())
        }
        async fn delete(&self, _user_id: &str, _bot_id: &str) -> Result<(), String> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn add_bot_saves_disabled_bot_with_id_equal_to_name() {
        let bots = Arc::new(InMemoryBots::default());
        let api_keys = Arc::new(MockApiKeyRepository::default());
        let uc = AddBotUseCase::new(bots.clone(), api_keys.clone(), Arc::new(FixedClock));

        // Full success path: both DynamoDB and S3 saves succeed.
        let bot = uc
            .execute(
                "user-1",
                "my-bot".to_string(),
                "ak".to_string(),
                "sk".to_string(),
            )
            .await
            .expect("execute succeeds when both repos succeed");

        // Returned bot reflects the construction policy.
        assert_eq!(bot.id, "my-bot", "id is derived from name");
        assert_eq!(bot.name, "my-bot");
        assert_eq!(bot.user_id, "user-1");
        assert!(!bot.enabled, "new bots start disabled");
        assert_eq!(bot.exchange, Exchange::Bybit);
        assert_eq!(bot.created_at, 1_700_000_000);
        assert_eq!(bot.updated_at, 1_700_000_000);

        // The bot was persisted to the bot repo.
        let saved = bots.get("user-1", "my-bot").expect("bot saved to bot repo");
        assert_eq!(saved.id, "my-bot");
        assert!(!saved.enabled);

        // The api keys repo received the same bot.
        let api_saved = api_keys
            .saved
            .lock()
            .unwrap()
            .clone()
            .expect("api keys saved");
        assert_eq!(api_saved.id, "my-bot");
        assert_eq!(api_saved.exchange, Exchange::Bybit);
    }
}
