use std::sync::Arc;
use crate::domain::bot::BotRepository;
use crate::infra::apikeyrepository::S3ApiKeyRepository;

pub struct DeleteBotUseCase {
    bot_repository: Arc<dyn BotRepository + Send + Sync>,
    api_keys_repository: Arc<S3ApiKeyRepository>,
}

impl DeleteBotUseCase {
    pub fn new(
        bot_repository: Arc<dyn BotRepository + Send + Sync>,
        api_keys_repository: Arc<S3ApiKeyRepository>,
    ) -> Self {
        Self {
            bot_repository,
            api_keys_repository,
        }
    }

    pub async fn execute(&self, user_id: &str, bot_id: &str) -> Result<(), String> {
        // Delete from DynamoDB
        self.bot_repository.delete(user_id, bot_id).await?;

        // Delete API keys from S3
        self.api_keys_repository
            .delete(user_id, bot_id)
            .await
            .map_err(|e| format!("Failed to remove API keys from S3: {}", e))?;

        Ok(())
    }
}