use std::sync::Arc;
use crate::domain::bot::{Bot, BotRepository};
use crate::domain::clock::Clock;
use crate::infra::apikeyrepository::S3ApiKeyRepository;

pub struct AddBotUseCase {
    bot_repository: Arc<dyn BotRepository + Send + Sync>,
    api_keys_repository: Arc<S3ApiKeyRepository>,
    clock: Arc<dyn Clock>,
}

impl AddBotUseCase {
    pub fn new(
        bot_repository: Arc<dyn BotRepository + Send + Sync>,
        api_keys_repository: Arc<S3ApiKeyRepository>,
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
    use std::collections::HashMap;
    use std::sync::Mutex;
    use async_trait::async_trait;
    use crate::domain::error::DomainError;
    use crate::domain::exchange::Exchange;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> i64 { 1_700_000_000 }
    }

    #[derive(Default)]
    struct InMemoryBots {
        bots: Mutex<HashMap<(String, String), Bot>>,
    }
    impl InMemoryBots {
        fn get(&self, user_id: &str, bot_id: &str) -> Option<Bot> {
            self.bots.lock().unwrap().get(&(user_id.to_string(), bot_id.to_string())).cloned()
        }
    }
    #[async_trait]
    impl BotRepository for InMemoryBots {
        async fn find(&self, user_id: &str, bot_id: &str) -> Option<Bot> {
            self.get(user_id, bot_id)
        }
        async fn save(&self, bot: &Bot) -> Result<(), DomainError> {
            self.bots.lock().unwrap().insert((bot.user_id.clone(), bot.id.clone()), bot.clone());
            Ok(())
        }
        async fn find_by_user_id(&self, user_id: &str) -> Vec<Bot> {
            self.bots.lock().unwrap().values().filter(|b| b.user_id == user_id).cloned().collect()
        }
        async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), String> {
            self.bots.lock().unwrap().remove(&(user_id.to_string(), bot_id.to_string()));
            Ok(())
        }
    }

    /// Build an S3 client pointed at an unreachable endpoint. AddBotUseCase
    /// depends on the *concrete* S3ApiKeyRepository (not a trait), so we cannot
    /// mock the S3 call. The DynamoDB save happens BEFORE the S3 save, so we
    /// drive execute(), let the S3 step fail, and assert the bot was persisted
    /// to the in-memory bot repo with the correct construction policy.
    fn unreachable_s3_repo() -> S3ApiKeyRepository {
        let creds = aws_sdk_s3::config::Credentials::new("test", "test", None, None, "pbtb-tests");
        let conf = aws_sdk_s3::config::Builder::new()
            .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .endpoint_url("http://127.0.0.1:1")
            .force_path_style(true)
            .credentials_provider(creds)
            .build();
        let client = aws_sdk_s3::Client::from_conf(conf);
        S3ApiKeyRepository::new(client, "test-bucket".to_string())
    }

    #[tokio::test]
    async fn add_bot_saves_disabled_bot_with_id_equal_to_name() {
        let bots = Arc::new(InMemoryBots::default());
        let s3 = Arc::new(unreachable_s3_repo());
        let uc = AddBotUseCase::new(bots.clone(), s3, Arc::new(FixedClock));

        // The S3 step is expected to fail (unreachable endpoint), but the
        // DynamoDB save runs first, so the bot must already be persisted.
        let _ = uc
            .execute("user-1", "my-bot".to_string(), "ak".to_string(), "sk".to_string())
            .await;

        let saved = bots.get("user-1", "my-bot").expect("bot saved before S3 step");
        assert_eq!(saved.id, "my-bot", "id is derived from name");
        assert_eq!(saved.name, "my-bot");
        assert_eq!(saved.user_id, "user-1");
        assert!(!saved.enabled, "new bots start disabled");
        assert_eq!(saved.exchange, Exchange::Bybit);
        assert_eq!(saved.created_at, 1_700_000_000);
        assert_eq!(saved.updated_at, 1_700_000_000);
    }
}