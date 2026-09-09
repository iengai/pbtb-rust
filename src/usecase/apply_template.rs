use crate::domain::bot::BotRepository;
use crate::domain::botconfig::{BotConfig, BotConfigRepository};
use crate::domain::clock::Clock;
use crate::domain::configswitch::{ConfigSwitchEvent, ConfigSwitchRepository};
use crate::domain::configtemplate::ConfigTemplateRepository;
use crate::domain::entitlement;
use crate::domain::error::DomainError;
use crate::usecase::engine_routing::EngineTaskDefinitions;
use std::sync::Arc;

pub struct ApplyTemplateUseCase {
    template_repository: Arc<dyn ConfigTemplateRepository>,
    bot_repository: Arc<dyn BotRepository>,
    bot_config_repository: Arc<dyn BotConfigRepository>,
    config_switch_repository: Arc<dyn ConfigSwitchRepository>,
    clock: Arc<dyn Clock>,
    engines: EngineTaskDefinitions,
}

impl ApplyTemplateUseCase {
    pub fn new(
        template_repository: Arc<dyn ConfigTemplateRepository>,
        bot_repository: Arc<dyn BotRepository>,
        bot_config_repository: Arc<dyn BotConfigRepository>,
        config_switch_repository: Arc<dyn ConfigSwitchRepository>,
        clock: Arc<dyn Clock>,
        engines: EngineTaskDefinitions,
    ) -> Self {
        Self {
            template_repository,
            bot_repository,
            bot_config_repository,
            config_switch_repository,
            clock,
            engines,
        }
    }

    /// `vip_level` is the caller's, as their account row reads at this request;
    /// a template above it is refused before anything is built.
    pub async fn execute(
        &self,
        user_id: &str,
        vip_level: u8,
        bot_id: &str,
        template_name: &str,
    ) -> Result<(), DomainError> {
        // 1. Build the bot config from the template (sets live.user internally).
        let bot_config = self
            .preview(user_id, vip_level, bot_id, template_name)
            .await?;

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
    ///
    /// The level gate sits here too: the preview is what the user confirms, so
    /// a template their level cannot apply is refused at the first tap rather
    /// than after they have read and agreed to it.
    pub async fn preview(
        &self,
        user_id: &str,
        vip_level: u8,
        bot_id: &str,
        template_name: &str,
    ) -> Result<BotConfig, DomainError> {
        let template = self.template_repository.get(template_name).await?;
        let required = template.min_vip_level();
        if !entitlement::meets(vip_level, required) {
            return Err(DomainError::InsufficientLevel {
                required,
                current: vip_level,
            });
        }
        let now = self.clock.now();
        let config =
            BotConfig::from_template(user_id.to_string(), bot_id.to_string(), &template, now)?;
        // A config that targets an engine with no registered image (on the
        // runtime this bot is set to) could never launch; refuse it here, at the
        // confirmation modal, rather than at the next Run. The preview is what
        // the user confirms, so the gate sits on it.
        let runtime = self
            .bot_repository
            .find(user_id, bot_id)
            .await?
            .map(|b| b.runtime)
            .unwrap_or_default();
        self.engines.resolve(config.engine_version()?, runtime)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::bot::Bot;
    use crate::domain::configswitch::ConfigSwitchKind;
    use crate::domain::configtemplate::ConfigTemplate;
    use crate::domain::engine::Runtime;
    use crate::domain::error::Retryability;
    use crate::domain::exchange::Exchange;
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Mutex;

    const NOW: i64 = 1_700_000_000;
    const USER: &str = "u-1";
    const BOT: &str = "alpha";
    /// A level no template in these tests asks more than.
    const TOP: u8 = crate::domain::user::MAX_VIP_LEVEL;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> i64 {
            NOW
        }
    }

    struct Templates(ConfigTemplate);
    #[async_trait]
    impl ConfigTemplateRepository for Templates {
        async fn get(&self, name: &str) -> Result<ConfigTemplate, DomainError> {
            if name == self.0.name {
                Ok(self.0.clone())
            } else {
                Err(DomainError::InvalidConfig(format!("no template {name}")))
            }
        }
        async fn list(&self) -> Result<Vec<String>, DomainError> {
            Ok(vec![self.0.name.clone()])
        }
        async fn exists(&self, name: &str) -> Result<bool, DomainError> {
            Ok(name == self.0.name)
        }
    }

    /// A single bot, or none when `bot` is `None`.
    struct Bots(Option<Bot>);
    #[async_trait]
    impl BotRepository for Bots {
        async fn find(&self, _u: &str, _b: &str) -> Result<Option<Bot>, DomainError> {
            Ok(self.0.clone())
        }
        async fn save(&self, _bot: &Bot) -> Result<(), DomainError> {
            Ok(())
        }
        async fn find_by_user_id(&self, _u: &str) -> Result<Vec<Bot>, DomainError> {
            Ok(self.0.clone().into_iter().collect())
        }
        async fn delete(&self, _u: &str, _b: &str) -> Result<(), DomainError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct Configs(Mutex<Option<BotConfig>>);
    #[async_trait]
    impl BotConfigRepository for Configs {
        async fn get(&self, _u: &str, _b: &str) -> Result<BotConfig, DomainError> {
            self.0
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| DomainError::InvalidConfig("no config".into()))
        }
        async fn save(&self, config: &BotConfig) -> Result<(), DomainError> {
            *self.0.lock().unwrap() = Some(config.clone());
            Ok(())
        }
        async fn delete(&self, _u: &str, _b: &str) -> Result<(), DomainError> {
            Ok(())
        }
        async fn exists(&self, _u: &str, _b: &str) -> Result<bool, DomainError> {
            Ok(self.0.lock().unwrap().is_some())
        }
    }

    #[derive(Default)]
    struct Switches {
        recorded: Mutex<Vec<ConfigSwitchEvent>>,
        fail: bool,
    }
    #[async_trait]
    impl ConfigSwitchRepository for Switches {
        async fn record(&self, event: &ConfigSwitchEvent) -> Result<(), DomainError> {
            if self.fail {
                return Err(DomainError::Repository {
                    context: "timeline unavailable".into(),
                    retry: Retryability::Transient,
                    source: "boom".into(),
                });
            }
            self.recorded.lock().unwrap().push(event.clone());
            Ok(())
        }
        async fn list_for_bot(
            &self,
            _u: &str,
            _b: &str,
        ) -> Result<Vec<ConfigSwitchEvent>, DomainError> {
            Ok(self.recorded.lock().unwrap().clone())
        }
    }

    fn a_template() -> ConfigTemplate {
        ConfigTemplate {
            name: "steady".to_string(),
            description: Some("a steady preset".to_string()),
            config_data: json!({ "config_version": "v7.12.0", "live": {} }),
            version: Some("3".to_string()),
        }
    }

    fn a_bot(runtime: Runtime) -> Bot {
        Bot::new(
            BOT.to_string(),
            USER.to_string(),
            Exchange::Bybit,
            BOT.to_string(),
            "ak".to_string(),
            "sk".to_string(),
            false,
            runtime,
            NOW,
            NOW,
        )
    }

    fn usecase(
        template: ConfigTemplate,
        bot: Option<Bot>,
        switches: Arc<Switches>,
        engines: &str,
    ) -> (ApplyTemplateUseCase, Arc<Configs>) {
        let configs = Arc::new(Configs::default());
        let uc = ApplyTemplateUseCase::new(
            Arc::new(Templates(template)),
            Arc::new(Bots(bot)),
            configs.clone(),
            switches,
            Arc::new(FixedClock),
            EngineTaskDefinitions::parse(engines).expect("engine table"),
        );
        (uc, configs)
    }

    #[tokio::test]
    async fn applying_saves_the_config_stamped_with_the_template() {
        let switches = Arc::new(Switches::default());
        let (uc, configs) = usecase(a_template(), Some(a_bot(Runtime::Py)), switches, "7=arn:7");

        uc.execute(USER, TOP, BOT, "steady").await.expect("apply");

        let saved = configs
            .0
            .lock()
            .unwrap()
            .clone()
            .expect("a config is saved");
        assert_eq!(saved.template_name, "steady");
        assert_eq!(saved.template_version.as_deref(), Some("3"));
        assert_eq!(saved.updated_at, NOW);
        // `live.user` names the bot, which is how passivbot finds its keys.
        assert_eq!(saved.config_data["live"]["user"], BOT);
    }

    #[tokio::test]
    async fn applying_marks_the_timeline_at_the_configs_own_timestamp() {
        let switches = Arc::new(Switches::default());
        let (uc, _) = usecase(
            a_template(),
            Some(a_bot(Runtime::Py)),
            switches.clone(),
            "7=arn:7",
        );

        uc.execute(USER, TOP, BOT, "steady").await.expect("apply");

        let events = switches.recorded.lock().unwrap().clone();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, ConfigSwitchKind::Template);
        assert_eq!(events[0].template_name, "steady");
        assert_eq!(
            events[0].applied_at, NOW,
            "the mark must line up with the config it annotates"
        );
    }

    #[tokio::test]
    async fn a_timeline_failure_does_not_undo_the_switch() {
        let switches = Arc::new(Switches {
            fail: true,
            ..Default::default()
        });
        let (uc, configs) = usecase(a_template(), Some(a_bot(Runtime::Py)), switches, "7=arn:7");

        uc.execute(USER, TOP, BOT, "steady")
            .await
            .expect("the switch itself succeeded, so the user's action must not fail");

        assert!(
            configs.0.lock().unwrap().is_some(),
            "the config is already saved; the timeline is an annotation, not the act"
        );
    }

    #[tokio::test]
    async fn a_config_with_no_image_for_its_engine_is_refused_before_saving() {
        let switches = Arc::new(Switches::default());
        // The template targets the v7 line; only v8 has an image registered.
        let (uc, configs) = usecase(a_template(), Some(a_bot(Runtime::Py)), switches, "8=arn:8");

        let err = uc
            .execute(USER, TOP, BOT, "steady")
            .await
            .expect_err("a config that could never launch must not be applied");

        assert!(
            err.to_string().contains("engine"),
            "the refusal should name the engine line: {err}"
        );
        assert!(
            configs.0.lock().unwrap().is_none(),
            "the gate sits before the save, so a refused apply leaves the old config"
        );
    }

    #[tokio::test]
    async fn the_gate_reads_the_bots_own_runtime() {
        let switches = Arc::new(Switches::default());
        // The bot runs `rs`, and only the `py` image of the v7 line exists.
        let (uc, _) = usecase(
            a_template(),
            Some(a_bot(Runtime::Rs)),
            switches,
            "7=arn:7py",
        );

        let err = uc
            .execute(USER, TOP, BOT, "steady")
            .await
            .expect_err("no rs image is registered for this line");
        assert!(err.to_string().contains("rs"), "{err}");
    }

    #[tokio::test]
    async fn a_bot_that_does_not_exist_is_gated_on_the_default_runtime() {
        let switches = Arc::new(Switches::default());
        let (uc, configs) = usecase(a_template(), None, switches, "7=arn:7py");

        uc.execute(USER, TOP, BOT, "steady")
            .await
            .expect("the default runtime is py, which is registered");
        assert!(configs.0.lock().unwrap().is_some());
    }

    fn a_gated_template(min_vip_level: u8) -> ConfigTemplate {
        let mut template = a_template();
        template.config_data["pbtb"] = json!({ "min_vip_level": min_vip_level });
        template
    }

    #[tokio::test]
    async fn a_template_above_the_callers_level_is_refused_before_anything_is_built() {
        let switches = Arc::new(Switches::default());
        let (uc, configs) = usecase(
            a_gated_template(3),
            Some(a_bot(Runtime::Py)),
            switches.clone(),
            "7=arn:7",
        );

        let err = uc
            .execute(USER, 2, BOT, "steady")
            .await
            .expect_err("VIP 2 may not apply a VIP 3 template");
        assert!(
            matches!(
                err,
                DomainError::InsufficientLevel {
                    required: 3,
                    current: 2
                }
            ),
            "{err}"
        );
        assert!(configs.0.lock().unwrap().is_none());
        assert!(switches.recorded.lock().unwrap().is_empty());

        let err = uc
            .preview(USER, 2, BOT, "steady")
            .await
            .expect_err("the preview is what the user confirms, so it is gated too");
        assert!(matches!(err, DomainError::InsufficientLevel { .. }));
    }

    #[tokio::test]
    async fn a_template_at_the_callers_level_applies() {
        let switches = Arc::new(Switches::default());
        let (uc, configs) = usecase(
            a_gated_template(3),
            Some(a_bot(Runtime::Py)),
            switches,
            "7=arn:7",
        );

        uc.execute(USER, 3, BOT, "steady")
            .await
            .expect("the gate is met at the level itself");
        assert!(configs.0.lock().unwrap().is_some());
    }

    #[tokio::test]
    async fn preview_builds_the_same_config_without_saving_it() {
        let switches = Arc::new(Switches::default());
        let (uc, configs) = usecase(
            a_template(),
            Some(a_bot(Runtime::Py)),
            switches.clone(),
            "7=arn:7",
        );

        let preview = uc.preview(USER, TOP, BOT, "steady").await.expect("preview");

        assert_eq!(preview.config_data["live"]["user"], BOT);
        assert!(
            configs.0.lock().unwrap().is_none(),
            "a preview is what the user confirms, not what they have already applied"
        );
        assert!(switches.recorded.lock().unwrap().is_empty());
    }
}
