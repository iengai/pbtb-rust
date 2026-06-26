use crate::domain::botconfig::BotConfigRepository;
use crate::domain::clock::Clock;
use std::sync::Arc;

/// Turn a bot's strategy side (`long`/`short`) on or off by editing
/// `live.forced_mode_<side>` in the bot config and persisting it. The change
/// takes effect on the next bot launch (the running task reads config at start).
pub struct SetStrategySideUseCase {
    bot_config_repository: Arc<dyn BotConfigRepository>,
    clock: Arc<dyn Clock>,
}

impl SetStrategySideUseCase {
    pub fn new(bot_config_repository: Arc<dyn BotConfigRepository>, clock: Arc<dyn Clock>) -> Self {
        Self {
            bot_config_repository,
            clock,
        }
    }

    /// Set `side` to `enabled`, persist, and return the resulting enabled state.
    pub async fn execute(
        &self,
        user_id: &str,
        bot_id: &str,
        side: &str,
        enabled: bool,
    ) -> Result<bool, String> {
        let mut config = self.bot_config_repository.get(user_id, bot_id).await?;
        config
            .set_side_enabled(side, enabled, self.clock.now())
            .map_err(|e| e.to_string())?;
        self.bot_config_repository.save(&config).await?;
        Ok(config.side_enabled(side))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::botconfig::{BotConfig, BotConfigRepository, BotType};
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Mutex;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> i64 {
            1_700_000_000
        }
    }

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
            user_id: "u".into(),
            bot_id: "b".into(),
            bot_type: BotType::Passivbot,
            template_name: "t".into(),
            template_version: None,
            config_data: json!({ "live": { "forced_mode_long": "", "forced_mode_short": "" } }),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[tokio::test]
    async fn disable_then_enable_short() {
        let repo = Arc::new(InMemoryConfig {
            config: Mutex::new(sample_config()),
        });
        let uc = SetStrategySideUseCase::new(repo.clone(), Arc::new(FixedClock));

        let now_enabled = uc.execute("u", "b", "short", false).await.unwrap();
        assert!(!now_enabled);
        let saved = repo.config.lock().unwrap().clone();
        assert_eq!(
            saved.config_data["live"]["forced_mode_short"].as_str(),
            Some("graceful_stop")
        );
        assert!(!saved.side_enabled("short"));
        assert!(saved.side_enabled("long"));
        assert_eq!(saved.updated_at, 1_700_000_000);

        let now_enabled = uc.execute("u", "b", "short", true).await.unwrap();
        assert!(now_enabled);
        assert!(repo.config.lock().unwrap().side_enabled("short"));
    }

    #[tokio::test]
    async fn invalid_side_is_rejected() {
        let repo = Arc::new(InMemoryConfig {
            config: Mutex::new(sample_config()),
        });
        let uc = SetStrategySideUseCase::new(repo, Arc::new(FixedClock));
        assert!(uc.execute("u", "b", "sideways", false).await.is_err());
    }
}
