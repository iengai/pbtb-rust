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
        let now = self.clock.now();

        let bot = Bot {
            id: name.clone(),  // Use name as id
            user_id: user_id.to_string(),
            name,
            api_key: api_key.clone(),
            secret_key: secret_key.clone(),
            enabled: false,
            created_at: now,
            updated_at: now,
        };

        // Save to DynamoDB
        self.bot_repository.save(&bot).await;

        // Save API keys to S3 api-keys.json
        self.api_keys_repository
            .upsert_bot_key(&bot.id, &api_key, &secret_key)
            .await
            .map_err(|e| format!("Failed to save API keys to S3: {}", e))?;

        Ok(bot)
    }
}