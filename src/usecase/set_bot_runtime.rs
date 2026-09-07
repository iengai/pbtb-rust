use crate::domain::bot::BotRepository;
use crate::domain::botconfig::BotConfigRepository;
use crate::domain::clock::Clock;
use crate::domain::engine::Runtime;
use crate::domain::error::DomainError;
use crate::usecase::engine_routing::EngineTaskDefinitions;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetRuntimeOutcome {
    /// The attribute is persisted; `previous` lets the caller say what changed.
    Updated {
        previous: Runtime,
        runtime: Runtime,
    },
    BotNotFound,
}

/// Move a bot between the Python passivbot image and the pb-runner image
/// within its engine line (per-bot opt-in, instant rollback by flipping back).
/// The running task is untouched: the attribute is read at launch only.
pub struct SetBotRuntimeUseCase {
    bots: Arc<dyn BotRepository>,
    configs: Arc<dyn BotConfigRepository>,
    engines: EngineTaskDefinitions,
    clock: Arc<dyn Clock>,
}

impl SetBotRuntimeUseCase {
    pub fn new(
        bots: Arc<dyn BotRepository>,
        configs: Arc<dyn BotConfigRepository>,
        engines: EngineTaskDefinitions,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            bots,
            configs,
            engines,
            clock,
        }
    }

    pub async fn execute(
        &self,
        user_id: &str,
        bot_id: &str,
        runtime: Runtime,
    ) -> Result<SetRuntimeOutcome, DomainError> {
        let mut bot = match self.bots.find(user_id, bot_id).await? {
            Some(b) => b,
            None => return Ok(SetRuntimeOutcome::BotNotFound),
        };

        // Refuse a runtime with no image for the line the bot's config targets,
        // at the moment the user asks rather than at the next Run: a bot whose
        // attribute says `rs` but can only ever launch the Python image is a
        // silent lie. A bot with no config yet has no line to check against.
        if self.configs.exists(user_id, bot_id).await? {
            let config = self.configs.get(user_id, bot_id).await?;
            self.engines.resolve(config.engine_version()?, runtime)?;
        }

        let previous = bot.runtime;
        if previous != runtime {
            bot.set_runtime(runtime, self.clock.now());
            self.bots.save(&bot).await?;
        }
        Ok(SetRuntimeOutcome::Updated { previous, runtime })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::bot::Bot;
    use crate::domain::botconfig::{BotConfig, BotType};
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> i64 {
            1_700_000_000
        }
    }

    #[derive(Default)]
    struct InMemoryBots {
        rows: Mutex<HashMap<String, Bot>>,
    }
    #[async_trait]
    impl BotRepository for InMemoryBots {
        async fn find(&self, _u: &str, bot_id: &str) -> Result<Option<Bot>, DomainError> {
            Ok(self.rows.lock().unwrap().get(bot_id).cloned())
        }
        async fn save(&self, bot: &Bot) -> Result<(), DomainError> {
            self.rows
                .lock()
                .unwrap()
                .insert(bot.id.clone(), bot.clone());
            Ok(())
        }
        async fn find_by_user_id(&self, _u: &str) -> Result<Vec<Bot>, DomainError> {
            Ok(self.rows.lock().unwrap().values().cloned().collect())
        }
        async fn delete(&self, _u: &str, bot_id: &str) -> Result<(), DomainError> {
            self.rows.lock().unwrap().remove(bot_id);
            Ok(())
        }
    }

    /// Holds at most one config; `None` models a bot with no config applied.
    struct InMemoryConfig(Option<BotConfig>);
    #[async_trait]
    impl BotConfigRepository for InMemoryConfig {
        async fn get(&self, _u: &str, _b: &str) -> Result<BotConfig, DomainError> {
            self.0
                .clone()
                .ok_or_else(|| DomainError::MissingConfigPath("config"))
        }
        async fn save(&self, _c: &BotConfig) -> Result<(), DomainError> {
            Ok(())
        }
        async fn delete(&self, _u: &str, _b: &str) -> Result<(), DomainError> {
            Ok(())
        }
        async fn exists(&self, _u: &str, _b: &str) -> Result<bool, DomainError> {
            Ok(self.0.is_some())
        }
    }

    fn config(stamp: &str) -> BotConfig {
        BotConfig {
            user_id: "u".into(),
            bot_id: "b".into(),
            bot_type: BotType::Passivbot,
            template_name: "t".into(),
            template_version: None,
            config_data: json!({ "config_version": stamp }),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn engines() -> EngineTaskDefinitions {
        EngineTaskDefinitions::parse("7=arn:v7,8=arn:v8,8rs=arn:v8rs").unwrap()
    }

    async fn bots_with(bot: Bot) -> Arc<InMemoryBots> {
        let bots = Arc::new(InMemoryBots::default());
        bots.save(&bot).await.unwrap();
        bots
    }

    #[tokio::test]
    async fn switches_runtime_and_stamps_updated_at() {
        let bots = bots_with(Bot::create(
            "u".into(),
            "b".into(),
            "ak".into(),
            "sk".into(),
            1,
        ))
        .await;
        let uc = SetBotRuntimeUseCase::new(
            bots.clone(),
            Arc::new(InMemoryConfig(Some(config("v8.1.0")))),
            engines(),
            Arc::new(FixedClock),
        );

        let out = uc.execute("u", "b", Runtime::Rs).await.unwrap();
        assert_eq!(
            out,
            SetRuntimeOutcome::Updated {
                previous: Runtime::Py,
                runtime: Runtime::Rs
            }
        );
        let saved = bots.find("u", "b").await.unwrap().unwrap();
        assert_eq!(saved.runtime, Runtime::Rs);
        assert_eq!(saved.updated_at, 1_700_000_000);
    }

    #[tokio::test]
    async fn same_runtime_is_a_no_op_write() {
        let bots = bots_with(Bot::create(
            "u".into(),
            "b".into(),
            "ak".into(),
            "sk".into(),
            1,
        ))
        .await;
        let uc = SetBotRuntimeUseCase::new(
            bots.clone(),
            Arc::new(InMemoryConfig(None)),
            engines(),
            Arc::new(FixedClock),
        );
        let out = uc.execute("u", "b", Runtime::Py).await.unwrap();
        assert_eq!(
            out,
            SetRuntimeOutcome::Updated {
                previous: Runtime::Py,
                runtime: Runtime::Py
            }
        );
        assert_eq!(bots.find("u", "b").await.unwrap().unwrap().updated_at, 1);
    }

    #[tokio::test]
    async fn refuses_a_runtime_with_no_image_for_the_bots_line() {
        let bots = bots_with(Bot::create(
            "u".into(),
            "b".into(),
            "ak".into(),
            "sk".into(),
            1,
        ))
        .await;
        let uc = SetBotRuntimeUseCase::new(
            bots.clone(),
            Arc::new(InMemoryConfig(Some(config("v7.12.0")))),
            engines(),
            Arc::new(FixedClock),
        );
        let err = uc.execute("u", "b", Runtime::Rs).await.unwrap_err();
        assert!(matches!(err, DomainError::InvalidConfig(_)), "{err}");
        assert_eq!(
            bots.find("u", "b").await.unwrap().unwrap().runtime,
            Runtime::Py,
            "the attribute is left untouched"
        );
    }

    #[tokio::test]
    async fn no_config_yet_skips_the_image_gate() {
        let bots = bots_with(Bot::create(
            "u".into(),
            "b".into(),
            "ak".into(),
            "sk".into(),
            1,
        ))
        .await;
        let uc = SetBotRuntimeUseCase::new(
            bots.clone(),
            Arc::new(InMemoryConfig(None)),
            EngineTaskDefinitions::parse("7=arn:v7").unwrap(),
            Arc::new(FixedClock),
        );
        uc.execute("u", "b", Runtime::Rs).await.unwrap();
        assert_eq!(
            bots.find("u", "b").await.unwrap().unwrap().runtime,
            Runtime::Rs
        );
    }

    #[tokio::test]
    async fn missing_bot_is_an_outcome_not_an_error() {
        let uc = SetBotRuntimeUseCase::new(
            Arc::new(InMemoryBots::default()),
            Arc::new(InMemoryConfig(None)),
            engines(),
            Arc::new(FixedClock),
        );
        let out = uc.execute("u", "nope", Runtime::Rs).await.unwrap();
        assert_eq!(out, SetRuntimeOutcome::BotNotFound);
    }
}
