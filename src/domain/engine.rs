use crate::domain::error::DomainError;
use serde_json::Value;
use std::fmt;

/// The passivbot engine major line a config targets.
///
/// A strategy is only proven on the engine line it was validated on: v8 broke
/// the v7 schema (the engine ships a `legacy_v7` migration precisely because of
/// it), and a migrated config is not the same strategy — a stale key the newer
/// engine still half-honours changes the numbers. So a bot is launched on the
/// image registered for its config's line, never on "the latest".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EngineVersion(u32);

impl EngineVersion {
    pub const fn new(major: u32) -> Self {
        Self(major)
    }

    pub fn major(self) -> u32 {
        self.0
    }

    /// Classify a passivbot config document.
    ///
    /// `config_version` is passivbot's own schema stamp (`v7.12.0`, `v8.1.0`);
    /// its major is the line. A stamp that is present but unparseable is
    /// refused rather than guessed — guessing an engine for a config that
    /// claims a version is exactly how a proven strategy ends up on the wrong
    /// binary. A config with no stamp at all (templates from before v7.12 never
    /// carried one) is classified by shape: only the v8 schema nests the
    /// per-side wallet exposure under `bot.<side>.risk`.
    pub fn of_config(config: &Value) -> Result<Self, DomainError> {
        match config.get("config_version") {
            Some(Value::String(stamp)) => Self::parse_stamp(stamp),
            Some(other) => Err(DomainError::InvalidConfig(format!(
                "config_version must be a version string like v8.1.0, got {other}"
            ))),
            None => Ok(if has_v8_risk_block(config) {
                Self(8)
            } else {
                Self(7)
            }),
        }
    }

    fn parse_stamp(stamp: &str) -> Result<Self, DomainError> {
        let digits = stamp.trim().trim_start_matches(['v', 'V']);
        let major = digits.split('.').next().unwrap_or_default();
        major.parse::<u32>().map(Self).map_err(|_| {
            DomainError::InvalidConfig(format!(
                "config_version {stamp:?} is not a semantic version like v8.1.0"
            ))
        })
    }
}

/// True when either side carries the v8 `risk` object.
fn has_v8_risk_block(config: &Value) -> bool {
    ["long", "short"]
        .iter()
        .any(|side| config["bot"][side]["risk"].is_object())
}

impl fmt::Display for EngineVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stamp_major_is_the_line() {
        assert_eq!(
            EngineVersion::of_config(&json!({"config_version": "v7.12.0"})).unwrap(),
            EngineVersion::new(7)
        );
        assert_eq!(
            EngineVersion::of_config(&json!({"config_version": "v8.1.0"})).unwrap(),
            EngineVersion::new(8)
        );
        // No leading `v` and surrounding whitespace are tolerated.
        assert_eq!(
            EngineVersion::of_config(&json!({"config_version": " 8.1.0 "})).unwrap(),
            EngineVersion::new(8)
        );
    }

    #[test]
    fn unstamped_config_is_classified_by_shape() {
        let v7 = json!({"bot": {"long": {"total_wallet_exposure_limit": 1.0}}});
        assert_eq!(
            EngineVersion::of_config(&v7).unwrap(),
            EngineVersion::new(7)
        );

        let v8 = json!({"bot": {"short": {"risk": {"total_wallet_exposure_limit": 1.0}}}});
        assert_eq!(
            EngineVersion::of_config(&v8).unwrap(),
            EngineVersion::new(8)
        );

        assert_eq!(
            EngineVersion::of_config(&json!({})).unwrap(),
            EngineVersion::new(7)
        );
    }

    #[test]
    fn explicit_but_unparseable_stamp_is_refused_not_guessed() {
        // A v8-shaped body must not rescue a broken stamp.
        let cfg = json!({
            "config_version": "banana",
            "bot": {"long": {"risk": {"total_wallet_exposure_limit": 1.0}}}
        });
        let err = EngineVersion::of_config(&cfg).unwrap_err();
        assert!(matches!(err, DomainError::InvalidConfig(_)), "{err}");

        let err = EngineVersion::of_config(&json!({"config_version": 8})).unwrap_err();
        assert!(matches!(err, DomainError::InvalidConfig(_)), "{err}");
    }

    #[test]
    fn displays_as_major_line() {
        assert_eq!(EngineVersion::new(8).to_string(), "v8");
    }
}
