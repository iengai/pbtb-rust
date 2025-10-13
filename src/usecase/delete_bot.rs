
use std::sync::Arc;
use crate::domain::bot::BotRepository;

pub struct DeleteBotUseCase {
    bot_repository: Arc<dyn BotRepository + Send + Sync>,
}

impl DeleteBotUseCase {
    pub fn new(bot_repository: Arc<dyn BotRepository + Send + Sync>) -> Self {
        Self { bot_repository }
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

        // Delete the bot
        self.bot_repository.delete(user_id, bot_id).await
    }
}