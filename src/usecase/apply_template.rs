use crate::domain::botconfig::{BotConfig, BotConfigRepository};
use crate::domain::clock::Clock;
use crate::domain::configswitch::{ConfigSwitchEvent, ConfigSwitchRepository};
use crate::domain::configtemplate::ConfigTemplateRepository;
use crate::domain::error::DomainError;
use crate::usecase::engine_routing::EngineTaskDefinitions;
use std::sync::Arc;

pub struct ApplyTemplateUseCase {
    template_repository: Arc<dyn ConfigTemplateRepository>,
    bot_config_repository: Arc<dyn BotConfigRepository>,
    config_switch_repository: Arc<dyn ConfigSwitchRepository>,
    clock: Arc<dyn Clock>,
    engines: EngineTaskDefinitions,
}

impl ApplyTemplateUseCase {
    pub fn new(
        template_repository: Arc<dyn ConfigTemplateRepository>,
        bot_config_repository: Arc<dyn BotConfigRepository>,
        config_switch_repository: Arc<dyn ConfigSwitchRepository>,
        clock: Arc<dyn Clock>,
        engines: EngineTaskDefinitions,
    ) -> Self {
        Self {
            template_repository,
            bot_config_repository,
            config_switch_repository,
            clock,
            engines,
        }
    }

    pub async fn execute(
        &self,
        user_id: &str,
        bot_id: &str,
        template_name: &str,
    ) -> Result<(), DomainError> {
        // 1. Build the bot config from the template (sets live.user internally).
        let bot_config = self.preview(user_id, bot_id, template_name).await?;

        // 2. Save bot config to S3: {user_id}/{bot_id}.json
        self.bot_config_repository.save(&bot_config).await?;

        // 3. Append a config-switch event to the bot's timeline so the return-curve
        //    chart can mark when this config took effect. `applied_at` reuses the
        //    timestamp already stamped on the saved config, so the mark lines up
        //    with the config. The switch itself has already succeeded above, so a
        //    failure to record this annotation is logged (never with the
        //    key/secret) and swallowed rather than failing the user's action.
        let event = ConfigSwitchEvent::template(
            user_id.to_string(),
            bot_id.to_string(),
            bot_config.template_name.clone(),
            bot_config.template_version.clone(),
            bot_config.updated_at,
        );
        if let Err(e) = self.config_switch_repository.record(&event).await {
            tracing::warn!(
                user_id = %user_id,
                bot_id = %bot_id,
                template_name = %bot_config.template_name,
                applied_at = bot_config.updated_at,
                "failed to record config-switch event: {e:#}"
            );
        }

        Ok(())
    }

    /// Build the bot config that `execute` would apply, WITHOUT saving it — for a
    /// confirmation preview (coins, exposure, strategy, description). `live.user`
    /// is set exactly as the real apply, so the preview matches what gets saved.
    pub async fn preview(
        &self,
        user_id: &str,
        bot_id: &str,
        template_name: &str,
    ) -> Result<BotConfig, DomainError> {
        let template = self.template_repository.get(template_name).await?;
        let now = self.clock.now();
        let config =
            BotConfig::from_template(user_id.to_string(), bot_id.to_string(), &template, now)?;
        // A config that targets an engine with no registered image could never
        // launch; refuse it here, at the confirmation modal, rather than at the
        // next Run. The preview is what the user confirms, so the gate sits on it.
        self.engines.resolve(config.engine_version()?)?;
        Ok(config)
    }
}
