use std::sync::Arc;
use crate::domain::botconfig::BotConfigRepository;
use crate::domain::RiskLevel;
use crate::domain::clock::Clock;

pub struct UpdateRiskLevelUseCase {
    bot_config_repository: Arc<dyn BotConfigRepository>,
    clock: Arc<dyn Clock>,
}

impl UpdateRiskLevelUseCase {
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
        risk_long: f64,
        risk_short: f64,
    ) -> Result<(), String> {
        // 1. Get existing bot config
        let mut bot_config = self.bot_config_repository.get(user_id, bot_id).await?;

        // 2. Create RiskLevel value object
        let risk_level = RiskLevel::new(risk_long, risk_short);
        risk_level.validate()?;

        // 3. Set risk level in config
        bot_config.set_risk_level(&risk_level)?;

        // 4. Calculate and set leverage (max risk + 1)
        let max_risk = risk_long.max(risk_short);
        let leverage_value = max_risk + 1.0;

        // Update leverage in config_data
        if let Some(live) = bot_config.config_data.get_mut("live") {
            live["leverage"] = serde_json::json!(leverage_value);
        } else {
            return Err("Missing live section in config".to_string());
        }

        // 5. Update timestamp
        let now = self.clock.now();
        bot_config.updated_at = now;

        // 6. Save updated config
        self.bot_config_repository.save(&bot_config).await?;

        Ok(())
    }
}