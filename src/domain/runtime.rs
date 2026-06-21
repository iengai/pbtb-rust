use crate::domain::error::DomainError;
use async_trait::async_trait;

/// Observed runtime state of a bot's ECS task (separate from Bot.enabled which is desired state).
///
/// `Starting` is the transient lock state the telebot stamps the instant it
/// claims the right to launch a task and before the ECS RUNNING event arrives.
/// It exists so a concurrent launch can be rejected and so a stop issued during
/// startup can locate the task. The Lambda only ever writes `Running`/`Stopped`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePhase {
    Starting,
    Running,
    Stopped,
}

impl RuntimePhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            RuntimePhase::Starting => "starting",
            RuntimePhase::Running => "running",
            RuntimePhase::Stopped => "stopped",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "starting" => Some(Self::Starting),
            "running" => Some(Self::Running),
            "stopped" => Some(Self::Stopped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BotRuntime {
    pub user_id: String,
    pub bot_id: String,
    pub task_id: Option<String>,
    pub phase: RuntimePhase,
    pub version: i64, // restart counter / task generation
    pub observed_at: i64,
}

impl BotRuntime {
    pub fn running(
        user_id: String,
        bot_id: String,
        task_id: String,
        version: i64,
        now: i64,
    ) -> Self {
        Self {
            user_id,
            bot_id,
            task_id: Some(task_id),
            phase: RuntimePhase::Running,
            version,
            observed_at: now,
        }
    }
    pub fn stopped(user_id: String, bot_id: String, version: i64, now: i64) -> Self {
        Self {
            user_id,
            bot_id,
            task_id: None,
            phase: RuntimePhase::Stopped,
            version,
            observed_at: now,
        }
    }
}

#[async_trait]
pub trait BotRuntimeRepository: Send + Sync {
    async fn find(&self, user_id: &str, bot_id: &str) -> Result<Option<BotRuntime>, DomainError>;
    /// Strongly-consistent read for decisions that must not act on a stale
    /// replica — stopping a task needs the freshest `task_id`. Defaults to
    /// `find`; the DynamoDB implementation overrides it with a consistent read.
    async fn find_consistent(
        &self,
        user_id: &str,
        bot_id: &str,
    ) -> Result<Option<BotRuntime>, DomainError> {
        self.find(user_id, bot_id).await
    }
    async fn record(&self, runtime: &BotRuntime) -> Result<(), DomainError>;
}

/// Outcome of attempting to claim the exclusive right to launch a bot's task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartClaim {
    /// The caller won the lock and must now launch exactly one task.
    Acquired,
    /// The task is already running; nothing to launch.
    AlreadyRunning,
    /// Another launch is already in flight (a fresh `starting` lock is held).
    AlreadyStarting,
}

/// The exclusive-start lock that prevents a bot from ever getting two
/// live-trading tasks. The money-critical guarantee lives in
/// `try_acquire_start`'s atomic conditional write — a strongly-consistent read
/// alone cannot prevent two concurrent claimers from both launching.
#[async_trait]
pub trait StartLockRepository: Send + Sync {
    /// Atomically transition the runtime row to `starting`, succeeding only when
    /// it is safe to launch: the row is absent/stopped, or holds a `starting`
    /// lock older than `stale_after` seconds (an abandoned launch). `now` is
    /// wall-clock seconds. Concurrent callers are serialized per row, so at most
    /// one receives `Acquired`.
    async fn try_acquire_start(
        &self,
        user_id: &str,
        bot_id: &str,
        now: i64,
        stale_after: i64,
    ) -> Result<StartClaim, DomainError>;
    /// Atomically claim the right to restart after `stopped_task_id` stopped:
    /// transition the row to `starting` ONLY while that task is still the bot's
    /// current task (the row's `task_id` still matches), bumping the restart
    /// counter. This is the idempotency gate for the Lambda's auto-restart —
    /// duplicate STOPPED events for the same task find the id already cleared and
    /// are rejected, so a stopped task can be replaced at most once. `Acquired`
    /// means the caller must launch; any other variant means the stopped task is
    /// no longer current and there is nothing to restart.
    async fn try_acquire_restart(
        &self,
        user_id: &str,
        bot_id: &str,
        stopped_task_id: &str,
        now: i64,
    ) -> Result<StartClaim, DomainError>;
    /// Record the launched `task_id` on the held `starting` lock so a stop issued
    /// before the RUNNING event can still find the task. A no-op if the row has
    /// already advanced past `starting` (a real observation won the row).
    async fn attach_started_task(
        &self,
        user_id: &str,
        bot_id: &str,
        task_id: &str,
    ) -> Result<(), DomainError>;
    /// Release a held `starting` lock back to `stopped` after a failed launch. A
    /// no-op if the row has already advanced past `starting`.
    async fn release_start(&self, user_id: &str, bot_id: &str, now: i64)
    -> Result<(), DomainError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_phase_round_trip() {
        assert_eq!(RuntimePhase::Running.as_str(), "running");
        assert_eq!(RuntimePhase::Stopped.as_str(), "stopped");
        assert_eq!(
            RuntimePhase::from_str("running"),
            Some(RuntimePhase::Running)
        );
        assert_eq!(
            RuntimePhase::from_str("STOPPED"),
            Some(RuntimePhase::Stopped)
        );
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
