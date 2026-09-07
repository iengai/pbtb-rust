use crate::domain::bot::Bot;
use crate::domain::botconfig::BotConfigRepository;
use crate::domain::engine::{EngineVersion, Runtime};
use crate::domain::error::DomainError;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;

/// The ECS task definition (hence the image) registered for each engine line
/// and runtime. Parsed once at process start from
/// `APP__ECS__TD_PASSIVBOT_BY_ENGINE` (`7=<arn>,8=<arn>,8rs=<arn>`), so a
/// malformed table fails boot, not a launch.
///
/// A key is `<major>[<runtime>]`: a bare major is the Python passivbot image
/// (`py` may be spelled out), `rs` is the pb-runner image of the same line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineTaskDefinitions {
    by_key: BTreeMap<(u32, Runtime), String>,
}

impl EngineTaskDefinitions {
    pub fn parse(spec: &str) -> Result<Self> {
        let mut by_key = BTreeMap::new();
        for entry in spec.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let (key, arn) = entry.split_once('=').ok_or_else(|| {
                anyhow!("entry {entry:?} is not <major>[<runtime>]=<task-def arn>")
            })?;
            let (major, runtime) = Self::parse_key(key.trim())
                .with_context(|| format!("engine key in {entry:?} is not <major>[py|rs]"))?;
            let arn = arn.trim();
            if arn.is_empty() {
                return Err(anyhow!(
                    "engine {} has an empty task-def arn",
                    Self::key_label(major, runtime)
                ));
            }
            if by_key.insert((major, runtime), arn.to_string()).is_some() {
                return Err(anyhow!(
                    "engine {} is registered twice",
                    Self::key_label(major, runtime)
                ));
            }
        }
        if by_key.is_empty() {
            return Err(anyhow!("no passivbot engines registered"));
        }
        Ok(Self { by_key })
    }

    /// `8` -> (8, Py); `8py` -> (8, Py); `8rs` -> (8, Rs). Anything else is an
    /// error: a typo in the table must never silently register a line.
    fn parse_key(key: &str) -> Result<(u32, Runtime)> {
        let digits_end = key.find(|c: char| !c.is_ascii_digit()).unwrap_or(key.len());
        let (digits, suffix) = key.split_at(digits_end);
        let major: u32 = digits.parse().context("engine major is not a number")?;
        let runtime = if suffix.is_empty() {
            Runtime::default()
        } else {
            suffix.parse::<Runtime>()?
        };
        Ok((major, runtime))
    }

    /// `v8` for the Python image, `v8rs` for pb-runner: the table's own key
    /// syntax, so a message names the entry the operator has to add.
    fn key_label(major: u32, runtime: Runtime) -> String {
        match runtime {
            Runtime::Py => format!("v{major}"),
            Runtime::Rs => format!("v{major}{runtime}"),
        }
    }

    /// The task definition for one engine line on one runtime. An unregistered
    /// pair is a user-facing error naming what is registered: a config must
    /// never fall back to some other engine, and a bot set to pb-runner must
    /// never silently launch the Python image (or vice versa).
    pub fn resolve(&self, engine: EngineVersion, runtime: Runtime) -> Result<&str, DomainError> {
        if let Some(arn) = self.by_key.get(&(engine.major(), runtime)) {
            return Ok(arn);
        }
        let line_registered = self.by_key.keys().any(|(m, _)| *m == engine.major());
        let msg = if line_registered {
            format!(
                "this bot is set to runtime `{runtime}` ({}), but no {runtime} image is \
                 registered for passivbot engine {engine} (registered: {})",
                runtime.image_label(),
                self.registered()
            )
        } else {
            format!(
                "this config needs passivbot engine {engine}, but no image is registered \
                 for it (registered: {})",
                self.registered()
            )
        };
        Err(DomainError::InvalidConfig(msg))
    }

    /// `v7, v8, v8rs` — for messages.
    pub fn registered(&self) -> String {
        self.by_key
            .keys()
            .map(|(m, rt)| Self::key_label(*m, *rt))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// What a bot launch resolves to: the engine its config targets, the runtime
/// the bot is set to, and the task definition registered for that pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchTarget {
    pub engine: EngineVersion,
    pub runtime: Runtime,
    pub td_arn: String,
}

/// Port: decide which task definition a bot must launch on. Both launch paths
/// (the user's Run and the auto-restart) go through this so they can never
/// disagree on the engine or the runtime. Takes the already-loaded bot: its
/// `runtime` attribute is half of the routing key.
#[async_trait]
pub trait LaunchTargetResolver: Send + Sync {
    async fn resolve(&self, bot: &Bot) -> Result<LaunchTarget, DomainError>;
}

/// Routes by the bot's stored config and its runtime attribute: the config's
/// `config_version` picks the engine line, `Bot::runtime` picks the image
/// within it, the registered table picks the task definition.
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
    async fn resolve(&self, bot: &Bot) -> Result<LaunchTarget, DomainError> {
        let config = self.configs.get(&bot.user_id, &bot.id).await?;
        let engine = config.engine_version()?;
        let runtime = bot.runtime;
        let td_arn = self.engines.resolve(engine, runtime)?.to_string();
        Ok(LaunchTarget {
            engine,
            runtime,
            td_arn,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> EngineTaskDefinitions {
        EngineTaskDefinitions::parse("7=arn:v7, 8=arn:v8, 8rs=arn:v8rs").unwrap()
    }

    #[test]
    fn parses_pairs_in_any_order_and_trims() {
        let t = EngineTaskDefinitions::parse(" 8=arn:v8 ,7=arn:v7,").unwrap();
        assert_eq!(
            t.resolve(EngineVersion::new(7), Runtime::Py).unwrap(),
            "arn:v7"
        );
        assert_eq!(
            t.resolve(EngineVersion::new(8), Runtime::Py).unwrap(),
            "arn:v8"
        );
        assert_eq!(t.registered(), "v7, v8");
    }

    #[test]
    fn runtime_suffix_selects_the_image_within_a_line() {
        let t = table();
        assert_eq!(
            t.resolve(EngineVersion::new(8), Runtime::Py).unwrap(),
            "arn:v8"
        );
        assert_eq!(
            t.resolve(EngineVersion::new(8), Runtime::Rs).unwrap(),
            "arn:v8rs"
        );
        assert_eq!(t.registered(), "v7, v8, v8rs");
        // An explicit `py` suffix is the same entry as the bare major.
        let t = EngineTaskDefinitions::parse("8py=arn:v8").unwrap();
        assert_eq!(
            t.resolve(EngineVersion::new(8), Runtime::Py).unwrap(),
            "arn:v8"
        );
        assert!(EngineTaskDefinitions::parse("8=a,8py=b").is_err());
    }

    #[test]
    fn malformed_table_fails_parse() {
        for bad in [
            "", "7", "x=arn", "7=", "7=a,7=b", "8xx=arn", "rs8=arn", "8-rs=arn", "8 rs=arn",
        ] {
            assert!(EngineTaskDefinitions::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn unregistered_engine_is_a_named_config_error() {
        let err = table()
            .resolve(EngineVersion::new(9), Runtime::Py)
            .unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, DomainError::InvalidConfig(_)), "{msg}");
        assert!(msg.contains("v9") && msg.contains("v7, v8, v8rs"), "{msg}");
    }

    #[test]
    fn unregistered_runtime_on_a_known_line_names_the_runtime() {
        let err = table()
            .resolve(EngineVersion::new(7), Runtime::Rs)
            .unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, DomainError::InvalidConfig(_)), "{msg}");
        assert!(msg.contains("`rs`") && msg.contains("v7"), "{msg}");
        assert!(msg.contains("v7, v8, v8rs"), "{msg}");
        // It must not read as an unknown line.
        assert!(!msg.contains("no image is registered for it"), "{msg}");
    }
}
