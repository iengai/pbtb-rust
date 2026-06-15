use async_trait::async_trait;
use crate::domain::error::DomainError;

/// Observed runtime state of a bot's ECS task (separate from Bot.enabled which is desired state).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePhase { Running, Stopped }

impl RuntimePhase {
    pub fn as_str(&self) -> &'static str { match self { RuntimePhase::Running => "running", RuntimePhase::Stopped => "stopped" } }
    pub fn from_str(s: &str) -> Option<Self> { match s.to_lowercase().as_str() { "running" => Some(Self::Running), "stopped" => Some(Self::Stopped), _ => None } }
}

#[derive(Debug, Clone)]
pub struct BotRuntime {
    pub user_id: String,
    pub bot_id: String,
    pub task_id: Option<String>,
    pub phase: RuntimePhase,
    pub version: i64,        // restart counter / task generation
    pub observed_at: i64,
}

impl BotRuntime {
    pub fn running(user_id: String, bot_id: String, task_id: String, version: i64, now: i64) -> Self {
        Self { user_id, bot_id, task_id: Some(task_id), phase: RuntimePhase::Running, version, observed_at: now }
    }
    pub fn stopped(user_id: String, bot_id: String, version: i64, now: i64) -> Self {
        Self { user_id, bot_id, task_id: None, phase: RuntimePhase::Stopped, version, observed_at: now }
    }
}

#[async_trait]
pub trait BotRuntimeRepository: Send + Sync {
    async fn find(&self, user_id: &str, bot_id: &str) -> Result<Option<BotRuntime>, DomainError>;
    async fn record(&self, runtime: &BotRuntime) -> Result<(), DomainError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_phase_round_trip() {
        assert_eq!(RuntimePhase::Running.as_str(), "running");
        assert_eq!(RuntimePhase::Stopped.as_str(), "stopped");
        assert_eq!(RuntimePhase::from_str("running"), Some(RuntimePhase::Running));
        assert_eq!(RuntimePhase::from_str("STOPPED"), Some(RuntimePhase::Stopped));
        assert_eq!(RuntimePhase::from_str("bogus"), None);
    }

    #[test]
    fn running_constructor_sets_fields() {
        let r = BotRuntime::running("u".into(), "b".into(), "task-1".into(), 3, 100);
        assert_eq!(r.user_id, "u");
        assert_eq!(r.bot_id, "b");
        assert_eq!(r.task_id.as_deref(), Some("task-1"));
        assert_eq!(r.phase, RuntimePhase::Running);
        assert_eq!(r.version, 3);
        assert_eq!(r.observed_at, 100);
    }

    #[test]
    fn stopped_constructor_clears_task() {
        let r = BotRuntime::stopped("u".into(), "b".into(), 4, 200);
        assert_eq!(r.task_id, None);
        assert_eq!(r.phase, RuntimePhase::Stopped);
        assert_eq!(r.version, 4);
        assert_eq!(r.observed_at, 200);
    }
}
