use std::sync::Arc;
use crate::domain::botconfig::{BotConfig, BotConfigRepository};

use crate::domain::clock::Clock;
use crate::domain::ConfigTemplateRepository;

pub struct ApplyTemplateUseCase {
    template_repository: Arc<dyn ConfigTemplateRepository>,
    bot_config_repository: Arc<dyn BotConfigRepository>,
    clock: Arc<dyn Clock>,
}

impl ApplyTemplateUseCase {
    pub fn new(
        template_repository: Arc<dyn ConfigTemplateRepository>,
        bot_config_repository: Arc<dyn BotConfigRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            template_repository,
            bot_config_repository,
            clock,
        }
    }

    pub async fn execute(
        &self,
        user_id: &str,
        bot_id: &str,
        template_name: &str,
    ) -> Result<(), String> {
        // Get the template
        let template = self.template_repository.get(template_name).await?;

        let now = self.clock.now();

        // Create bot config from template
        let bot_config = BotConfig::from_template(
            user_id.to_string(),
            bot_id.to_string(),
            &template,
            now,
        );

        // Save bot config
        self.bot_config_repository.save(&bot_config).await
    }
}