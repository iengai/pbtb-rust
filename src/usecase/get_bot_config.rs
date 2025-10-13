use std::sync::Arc;
use crate::domain::botconfig::{BotConfig, BotConfigRepository};

pub struct GetBotConfigUseCase {
    bot_config_repository: Arc<dyn BotConfigRepository>,
}

impl GetBotConfigUseCase {
    pub fn new(bot_config_repository: Arc<dyn BotConfigRepository>) -> Self {
        Self { bot_config_repository }
    }

    pub async fn execute(&self, user_id: &str, bot_id: &str) -> Result<BotConfig, String> {
        self.bot_config_repository.get(user_id, bot_id).await
    }
}