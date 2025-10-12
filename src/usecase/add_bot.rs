use std::sync::Arc;
use crate::domain::bot::{Bot, BotRepository};
use crate::domain::clock::Clock;

pub struct AddBotUseCase {
    bot_repository: Arc<dyn BotRepository + Send + Sync>,
    clock: Arc<dyn Clock>,
}

impl AddBotUseCase {
    pub fn new(bot_repository: Arc<dyn BotRepository + Send + Sync>, clock: Arc<dyn Clock>) -> Self {
        Self {
            bot_repository,
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
            api_key,
            secret_key,
            enabled: false, // Default to false
            created_at: now,
            updated_at: now,
        };

        self.bot_repository.save(&bot).await;
        Ok(bot)
    }
}