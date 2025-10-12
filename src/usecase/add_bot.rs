use std::sync::Arc;
use uuid::Uuid;
use crate::domain::bot::{Bot, BotRepository};

pub struct AddBotUseCase {
    bot_repository: Arc<dyn BotRepository + Send + Sync>,
}

impl AddBotUseCase {
    pub fn new(bot_repository: Arc<dyn BotRepository + Send + Sync>) -> Self {
        Self { bot_repository }
    }

    pub async fn execute(
        &self,
        user_id: &str,
        name: String,
        api_key: String,
        secret_key: String,
    ) -> Result<Bot, String> {
        let bot = Bot {
            id: name.clone(),  // Use name as id
            user_id: user_id.to_string(),
            name,
            api_key,
            secret_key,
            enabled: false, // Default to false
        };

        self.bot_repository.save(&bot).await;
        Ok(bot)
    }
}