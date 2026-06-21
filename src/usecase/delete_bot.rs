use crate::domain::bot::{ApiKeyRepository, BotRepository};
use std::sync::Arc;

pub struct DeleteBotUseCase {
    bot_repository: Arc<dyn BotRepository + Send + Sync>,
    api_keys_repository: Arc<dyn ApiKeyRepository>,
}

impl DeleteBotUseCase {
    pub fn new(
        bot_repository: Arc<dyn BotRepository + Send + Sync>,
        api_keys_repository: Arc<dyn ApiKeyRepository>,
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
