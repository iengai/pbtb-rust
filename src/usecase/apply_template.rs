use crate::domain::botconfig::{BotConfig, BotConfigRepository};
use crate::domain::clock::Clock;
use crate::domain::configtemplate::ConfigTemplateRepository;
use std::sync::Arc;

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
        // 1. Build the bot config from the template (sets live.user internally).
        let bot_config = self.preview(user_id, bot_id, template_name).await?;

        // 2. Save bot config to S3: {user_id}/{bot_id}.json
        self.bot_config_repository.save(&bot_config).await
    }

    /// Build the bot config that `execute` would apply, WITHOUT saving it — for a
    /// confirmation preview (coins, exposure, strategy, description). `live.user`
    /// is set exactly as the real apply, so the preview matches what gets saved.
    pub async fn preview(
        &self,
        user_id: &str,
        bot_id: &str,
        template_name: &str,
    ) -> Result<BotConfig, String> {
        let template = self.template_repository.get(template_name).await?;
        let now = self.clock.now();
        BotConfig::from_template(user_id.to_string(), bot_id.to_string(), &template, now)
            .map_err(|e| e.to_string())
    }
}
