use std::sync::Arc;
use crate::domain::botconfig::BotConfigRepository;
use crate::domain::clock::Clock;

pub struct UpdateBotConfigUseCase {
    bot_config_repository: Arc<dyn BotConfigRepository>,
    clock: Arc<dyn Clock>,
}

impl UpdateBotConfigUseCase {
    pub fn new(
        bot_config_repository: Arc<dyn BotConfigRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            bot_config_repository,
            clock,
        }
    }

    pub async fn execute(
        &self,
        user_id: &str,
        bot_id: &str,
        new_config_data: serde_json::Value,
    ) -> Result<(), String> {
        // Get existing config
        let mut bot_config = self.bot_config_repository.get(user_id, bot_id).await?;

        // Update config data
        let now = self.clock.now();
        bot_config.update_config_data(new_config_data, now);

        // Save updated config
        self.bot_config_repository.save(&bot_config).await
    }
}