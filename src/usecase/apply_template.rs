
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

        // 2. Create bot config from template
        let mut bot_config = BotConfig::from_template(
            user_id.to_string(),
            bot_id.to_string(),
            &template,
            now,
        );

        // 3. Override config["live"]["user"] with bot_id
        if let Err(e) = override_live_user(&mut bot_config, bot_id) {
            return Err(format!("Failed to override live.user: {}", e));
        }

        // 4. Save bot config to S3: {user_id}/{bot_id}.json
        self.bot_config_repository.save(&bot_config).await
    }
}

/// Override config["live"]["user"] with bot_id
fn override_live_user(bot_config: &mut BotConfig, bot_id: &str) -> Result<(), String> {
    // Get mutable reference to config_data
    let config_data = &mut bot_config.config_data;

    // Navigate to config["live"]["user"] and set it to bot_id
    if let Some(live) = config_data.get_mut("live") {
        if let Some(live_obj) = live.as_object_mut() {
            live_obj.insert("user".to_string(), serde_json::json!(bot_id));
            Ok(())
        } else {
            Err("config.live is not an object".to_string())
        }
    } else {
        Err("config.live not found".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_override_live_user() {
        let config_data = json!({
            "live": {
                "user": "original_user",
                "leverage": 3.3
            }
        });

        let mut bot_config = BotConfig {
            user_id: "user123".to_string(),
            bot_id: "bot456".to_string(),
            bot_type: crate::domain::botconfig::BotType::Passivbot,
            template_name: "test".to_string(),
            template_version: Option::from("0".to_string()),
            config_data,
            created_at: 0,
            updated_at: 0,
        };

        // Override user field
        override_live_user(&mut bot_config, "bot456").unwrap();

        // Verify
        assert_eq!(
            bot_config.config_data["live"]["user"],
            json!("bot456")
        );
    }
}