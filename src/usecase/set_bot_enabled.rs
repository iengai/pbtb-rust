use std::sync::Arc;
use crate::domain::bot::BotRepository;
use crate::domain::clock::Clock;

pub struct SetBotEnabledUseCase { bots: Arc<dyn BotRepository>, clock: Arc<dyn Clock> }
impl SetBotEnabledUseCase {
    pub fn new(bots: Arc<dyn BotRepository>, clock: Arc<dyn Clock>) -> Self { Self { bots, clock } }
    pub async fn execute(&self, user_id: &str, bot_id: &str, enabled: bool) -> Result<(), String> {
        let mut bot = self.bots.find(user_id, bot_id).await.ok_or_else(|| "Bot not found".to_string())?;
        let now = self.clock.now();
        if enabled { bot.enable(now); } else { bot.disable(now); }
        self.bots.save(&bot).await.map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use async_trait::async_trait;
    use crate::domain::bot::Bot;
    use crate::domain::error::DomainError;
    use crate::domain::exchange::Exchange;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> i64 { 1_700_000_000 }
    }

    #[derive(Default)]
    struct InMemoryBots {
        bots: Mutex<HashMap<(String, String), Bot>>,
    }
    impl InMemoryBots {
        fn with(bot: Bot) -> Self {
            let mut map = HashMap::new();
            map.insert((bot.user_id.clone(), bot.id.clone()), bot);
            Self { bots: Mutex::new(map) }
        }
        fn get(&self, user_id: &str, bot_id: &str) -> Option<Bot> {
            self.bots.lock().unwrap().get(&(user_id.to_string(), bot_id.to_string())).cloned()
        }
    }
    #[async_trait]
    impl BotRepository for InMemoryBots {
        async fn find(&self, user_id: &str, bot_id: &str) -> Option<Bot> {
            self.get(user_id, bot_id)
        }
        async fn save(&self, bot: &Bot) -> Result<(), DomainError> {
            self.bots.lock().unwrap().insert((bot.user_id.clone(), bot.id.clone()), bot.clone());
            Ok(())
        }
        async fn find_by_user_id(&self, user_id: &str) -> Vec<Bot> {
            self.bots.lock().unwrap().values().filter(|b| b.user_id == user_id).cloned().collect()
        }
        async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), String> {
            self.bots.lock().unwrap().remove(&(user_id.to_string(), bot_id.to_string()));
            Ok(())
        }
    }

    fn sample_bot(enabled: bool) -> Bot {
        Bot::new(
            "bot-1".to_string(),
            "user-1".to_string(),
            Exchange::Bybit,
            "bot-1".to_string(),
            "ak".to_string(),
            "sk".to_string(),
            enabled,
            1,
            1,
        )
    }

    #[tokio::test]
    async fn enable_then_disable_flips_and_persists() {
        let bots = Arc::new(InMemoryBots::with(sample_bot(false)));
        let uc = SetBotEnabledUseCase::new(bots.clone(), Arc::new(FixedClock));

        uc.execute("user-1", "bot-1", true).await.unwrap();
        let after_enable = bots.get("user-1", "bot-1").unwrap();
        assert!(after_enable.enabled);
        assert_eq!(after_enable.updated_at, 1_700_000_000);

        uc.execute("user-1", "bot-1", false).await.unwrap();
        let after_disable = bots.get("user-1", "bot-1").unwrap();
        assert!(!after_disable.enabled);
    }

    #[tokio::test]
    async fn missing_bot_errors() {
        let bots = Arc::new(InMemoryBots::default());
        let uc = SetBotEnabledUseCase::new(bots, Arc::new(FixedClock));
        let err = uc.execute("user-1", "nope", true).await.unwrap_err();
        assert!(err.contains("not found"));
    }
}
