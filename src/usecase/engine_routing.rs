use crate::domain::botconfig::BotConfigRepository;
use crate::domain::engine::EngineVersion;
use crate::domain::error::DomainError;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;

/// The ECS task definition (hence the passivbot image) registered for each
/// engine line. Parsed once at process start from
/// `APP__ECS__TD_PASSIVBOT_BY_ENGINE` (`7=<arn>,8=<arn>`), so a malformed table
/// fails boot, not a launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineTaskDefinitions {
    by_major: BTreeMap<u32, String>,
}

impl EngineTaskDefinitions {
    pub fn parse(spec: &str) -> Result<Self> {
        let mut by_major = BTreeMap::new();
        for entry in spec.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let (major, arn) = entry
                .split_once('=')
                .ok_or_else(|| anyhow!("entry {entry:?} is not <major>=<task-def arn>"))?;
            let major: u32 = major
                .trim()
                .parse()
                .with_context(|| format!("engine major in {entry:?} is not a number"))?;
            let arn = arn.trim();
            if arn.is_empty() {
                return Err(anyhow!("engine v{major} has an empty task-def arn"));
            }
            if by_major.insert(major, arn.to_string()).is_some() {
                return Err(anyhow!("engine v{major} is registered twice"));
            }
        }
        if by_major.is_empty() {
            return Err(anyhow!("no passivbot engines registered"));
        }
        Ok(Self { by_major })
    }

    /// The task definition for one engine line. An unregistered line is a
    /// user-facing error naming what is registered: a config must never fall
    /// back to some other engine silently.
    pub fn resolve(&self, engine: EngineVersion) -> Result<&str, DomainError> {
        self.by_major
            .get(&engine.major())
            .map(String::as_str)
            .ok_or_else(|| {
                DomainError::InvalidConfig(format!(
                    "this config needs passivbot engine {engine}, but no image is registered \
                     for it (registered: {})",
                    self.registered()
                ))
            })
    }

    /// `v7, v8` — for messages.
    pub fn registered(&self) -> String {
        self.by_major
            .keys()
            .map(|m| format!("v{m}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// What a bot launch resolves to: the engine its config targets and the task
/// definition registered for that engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchTarget {
    pub engine: EngineVersion,
    pub td_arn: String,
}

/// Port: decide which task definition a bot must launch on. Both launch paths
/// (the user's Run and the auto-restart) go through this so they can never
/// disagree on the engine.
#[async_trait]
pub trait LaunchTargetResolver: Send + Sync {
    async fn resolve(&self, user_id: &str, bot_id: &str) -> Result<LaunchTarget, DomainError>;
}

/// Routes by the bot's stored config: its `config_version` picks the engine
/// line, the registered table picks the image.
pub struct EngineRoutedResolver {
    configs: Arc<dyn BotConfigRepository>,
    engines: EngineTaskDefinitions,
}

impl EngineRoutedResolver {
    pub fn new(configs: Arc<dyn BotConfigRepository>, engines: EngineTaskDefinitions) -> Self {
        Self { configs, engines }
    }
}

#[async_trait]
impl LaunchTargetResolver for EngineRoutedResolver {
    async fn resolve(&self, user_id: &str, bot_id: &str) -> Result<LaunchTarget, DomainError> {
        let config = self.configs.get(user_id, bot_id).await?;
        let engine = config.engine_version()?;
        let td_arn = self.engines.resolve(engine)?.to_string();
        Ok(LaunchTarget { engine, td_arn })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> EngineTaskDefinitions {
        EngineTaskDefinitions::parse("7=arn:v7, 8=arn:v8").unwrap()
    }

    #[test]
    fn parses_pairs_in_any_order_and_trims() {
        let t = EngineTaskDefinitions::parse(" 8=arn:v8 ,7=arn:v7,").unwrap();
        assert_eq!(t.resolve(EngineVersion::new(7)).unwrap(), "arn:v7");
        assert_eq!(t.resolve(EngineVersion::new(8)).unwrap(), "arn:v8");
        assert_eq!(t.registered(), "v7, v8");
    }

    #[test]
    fn malformed_table_fails_parse() {
        for bad in ["", "7", "x=arn", "7=", "7=a,7=b"] {
            assert!(EngineTaskDefinitions::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn unregistered_engine_is_a_named_config_error() {
        let err = table().resolve(EngineVersion::new(9)).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, DomainError::InvalidConfig(_)), "{msg}");
        assert!(msg.contains("v9") && msg.contains("v7, v8"), "{msg}");
    }
}
