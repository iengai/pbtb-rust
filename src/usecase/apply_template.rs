
use std::sync::Arc;
use crate::domain::botconfig::{BotConfig, BotConfigRepository};
use crate::domain::configtemplate::ConfigTemplateRepository;
use crate::domain::clock::Clock;

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
        // 1. Get the template
        let template = self.template_repository.get(template_name).await?;

        let now = self.clock.now();

        // 2. Create bot config from template (sets live.user internally)
        let bot_config = BotConfig::from_template(
            user_id.to_string(),
            bot_id.to_string(),
            &template,
            now,
        )
        .map_err(|e| e.to_string())?;

        // 3. Save bot config to S3: {user_id}/{bot_id}.json
        self.bot_config_repository.save(&bot_config).await
    }
}