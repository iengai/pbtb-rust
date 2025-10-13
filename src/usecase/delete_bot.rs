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
        // Verify bot exists and belongs to user
        match self.bot_repository.find_by_id(bot_id).await {
            Some(bot) => {
                if bot.user_id != user_id {
                    return Err("Bot does not belong to this user".to_string());
                }
            }
            None => {
                return Err("Bot not found".to_string());
            }
        }

        // Delete from DynamoDB
        self.bot_repository.delete(user_id, bot_id).await?;

        // Delete API keys from S3
        self.api_keys_repository
            .remove_bot_key(bot_id)
            .await
            .map_err(|e| format!("Failed to remove API keys from S3: {}", e))?;

        Ok(())
    }
}