use std::sync::Arc;
use crate::domain::bot::{Bot, BotRepository};

pub struct ListBotsUseCase {
    // Use concrete constraint where needed
    bot_repository: Arc<dyn BotRepository + Send + Sync>,
}

impl ListBotsUseCase {
    pub fn new(bot_repository: Arc<dyn BotRepository + Send + Sync>) -> Self {
        Self { bot_repository }
    }

    pub async fn execute(&self, user_id: &str) -> Result<Vec<Bot>, String> {
        let bots = self.bot_repository.find_by_user_id(user_id).await;
        Ok(bots)
    }
}