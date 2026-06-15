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

        // 2. Create RiskLevel value object (validated on construction)
        let risk = RiskLevel::new(risk_long, risk_short).map_err(|e| e.to_string())?;

        // 3. Apply risk level: sets risk, derives leverage, bumps updated_at.
        // The leverage policy lives in the domain now.
        bot_config
            .apply_risk_level(&risk, self.clock.now())
            .map_err(|e| e.to_string())?;

        // 4. Save updated config
        self.bot_config_repository.save(&bot_config).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use async_trait::async_trait;
    use serde_json::json;
    use crate::domain::botconfig::{BotConfig, BotType};

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> i64 { 1_700_000_000 }
    }

    /// Mock BotConfigRepository holding a single config that `get` returns and
    /// `save` overwrites (so the test can inspect the persisted result).
    struct InMemoryConfig {
        config: Mutex<BotConfig>,
    }
    #[async_trait]
    impl BotConfigRepository for InMemoryConfig {
        async fn get(&self, _user_id: &str, _bot_id: &str) -> Result<BotConfig, String> {
            Ok(self.config.lock().unwrap().clone())
        }
        async fn save(&self, config: &BotConfig) -> Result<(), String> {
            *self.config.lock().unwrap() = config.clone();
            Ok(())
        }
        async fn delete(&self, _user_id: &str, _bot_id: &str) -> Result<(), String> {
            Ok(())
        }
        async fn exists(&self, _user_id: &str, _bot_id: &str) -> Result<bool, String> {
            Ok(true)
        }
    }

    fn sample_config() -> BotConfig {
        BotConfig {
            user_id: "user-1".into(),
            bot_id: "bot-1".into(),
            bot_type: BotType::Passivbot,
            template_name: "t".into(),
            template_version: None,
            config_data: json!({
                "bot": {
                    "long": { "total_wallet_exposure_limit": 1.0 },
                    "short": { "total_wallet_exposure_limit": 1.0 }
                },
                "live": { "leverage": 2.0 }
            }),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[tokio::test]
    async fn update_applies_risk_and_derives_leverage() {
        let repo = Arc::new(InMemoryConfig { config: Mutex::new(sample_config()) });
        let uc = UpdateRiskLevelUseCase::new(repo.clone(), Arc::new(FixedClock));

        // long=3, short=5 => leverage = max(3,5)+1 = 6
        uc.execute("user-1", "bot-1", 3.0, 5.0).await.unwrap();

        let saved = repo.config.lock().unwrap().clone();
        let risk = saved.risk_level().unwrap();
        assert_eq!(risk.long, 3.0);
        assert_eq!(risk.short, 5.0);
        let lev = saved.leverage().unwrap();
        assert_eq!(lev.long, 6.0);
        assert_eq!(lev.short, 6.0);
        assert_eq!(saved.updated_at, 1_700_000_000);
    }

    #[tokio::test]
    async fn out_of_range_risk_is_rejected() {
        let repo = Arc::new(InMemoryConfig { config: Mutex::new(sample_config()) });
        let uc = UpdateRiskLevelUseCase::new(repo, Arc::new(FixedClock));
        // 11.0 is above the [0,10] range.
        let err = uc.execute("user-1", "bot-1", 11.0, 5.0).await.unwrap_err();
        assert!(!err.is_empty());
    }
}