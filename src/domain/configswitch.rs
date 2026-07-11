use crate::domain::error::DomainError;
use async_trait::async_trait;

/// Which kind of config change a recorded switch represents.
///
/// Only whole-template switches are recorded today (the return curve is
/// annotated with when a bot switched config). The enum leaves room to record
/// in-place edits (risk level, side toggles) later without a storage change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSwitchKind {
    Template,
}

impl ConfigSwitchKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConfigSwitchKind::Template => "template",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "template" => Some(ConfigSwitchKind::Template),
            _ => None,
        }
    }
}

/// A point-in-time record that a bot switched to a named config. Events are
/// appended, never overwritten, so together they form the timeline the
/// return-curve chart annotates. `applied_at` is the same timestamp stamped on
/// the saved config, in Unix seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigSwitchEvent {
    pub user_id: String,
    pub bot_id: String,
    pub kind: ConfigSwitchKind,
    pub template_name: String,
    pub template_version: Option<String>,
    pub applied_at: i64,
}

impl ConfigSwitchEvent {
    /// A switch to a whole template — the only kind recorded today.
    pub fn template(
        user_id: String,
        bot_id: String,
        template_name: String,
        template_version: Option<String>,
        applied_at: i64,
    ) -> Self {
        Self {
            user_id,
            bot_id,
            kind: ConfigSwitchKind::Template,
            template_name,
            template_version,
            applied_at,
        }
    }
}

/// Domain port for the per-bot config-switch timeline. Reads are fallible and an
/// `Err` is a genuine fault, never collapsed into an empty timeline (see
/// docs/conventions.md § Error Handling).
#[async_trait]
pub trait ConfigSwitchRepository: Send + Sync {
    /// Append one switch event to the bot's timeline.
    async fn record(&self, event: &ConfigSwitchEvent) -> Result<(), DomainError>;

    /// The bot's switch timeline, oldest first. Consumed by the return-curve
    /// collector to mark on the chart when each config took effect.
    async fn list_for_bot(
        &self,
        user_id: &str,
        bot_id: &str,
    ) -> Result<Vec<ConfigSwitchEvent>, DomainError>;
}
