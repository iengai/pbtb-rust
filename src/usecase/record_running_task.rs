use std::sync::Arc;
use anyhow::Result;
use crate::domain::runtime::{BotRuntime, BotRuntimeRepository, RuntimePhase};

#[derive(Debug, PartialEq, Eq)]
pub enum RecordRunningOutcome {
    Recorded { version: i64 },
    SkippedStale,
}

/// Records observed-running for a bot whose ECS task reached RUNNING.
///
/// Driven by the ECS Task State Change Lambda. This is the counterpart to
/// `ReconcileStoppedTaskUseCase` (which owns the STOPPED path): together they
/// keep `BotRuntime` (observed state) in sync with reality, event by event.
///
/// It is an OBSERVATION, not a restart: it preserves the version/restart counter
/// (owned by the reconcile path) and never starts or stops a task. `observed_at`
/// is the EventBridge event time; an event older than the last observation is
/// ignored so a late/reordered RUNNING cannot resurrect a task we already saw stop.
pub struct RecordRunningTaskUseCase {
    runtimes: Arc<dyn BotRuntimeRepository>,
}

impl RecordRunningTaskUseCase {
    pub fn new(runtimes: Arc<dyn BotRuntimeRepository>) -> Self {
        Self { runtimes }
    }

    pub async fn execute(&self, user_id: &str, bot_id: &str, task_id: &str, observed_at: i64) -> Result<RecordRunningOutcome> {
        let existing = self
            .runtimes
            .find(user_id, bot_id)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        // Stale-event guard: ignore an event older than what we have already
        // observed. At an equal second a recorded STOPPED wins the tie — RUNNING
        // always precedes STOPPED for a task, so a same-second RUNNING is the
        // reordered/stale one. The repository enforces the same rule atomically.
        if let Some(prev) = &existing {
            let stale = prev.observed_at > observed_at
                || (prev.observed_at == observed_at && prev.phase == RuntimePhase::Stopped);
            if stale {
                return Ok(RecordRunningOutcome::SkippedStale);
            }
        }

        // Preserve the restart/generation counter; observing a running task is not a restart.
        let version = existing.map(|r| r.version).unwrap_or(0).max(1);

        self.runtimes
            .record(&BotRuntime::running(
                user_id.to_string(),
                bot_id.to_string(),
                task_id.to_string(),
                version,
                observed_at,
            ))
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        Ok(RecordRunningOutcome::Recorded { version })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use async_trait::async_trait;
    use crate::domain::error::DomainError;
    use crate::domain::runtime::RuntimePhase;

    #[derive(Default)]
    struct InMemoryRuntimes {
        runtimes: Mutex<HashMap<(String, String), BotRuntime>>,
    }
    #[async_trait]
    impl BotRuntimeRepository for InMemoryRuntimes {
        async fn find(&self, user_id: &str, bot_id: &str) -> Result<Option<BotRuntime>, DomainError> {
            Ok(self.runtimes.lock().unwrap().get(&(user_id.to_string(), bot_id.to_string())).cloned())
        }
        async fn record(&self, runtime: &BotRuntime) -> Result<(), DomainError> {
            self.runtimes.lock().unwrap().insert((runtime.user_id.clone(), runtime.bot_id.clone()), runtime.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn records_running_for_new_bot_at_version_one() {
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let uc = RecordRunningTaskUseCase::new(runtimes.clone());

        let outcome = uc.execute("u", "b", "task-1", 1_700_000_000).await.unwrap();
        assert_eq!(outcome, RecordRunningOutcome::Recorded { version: 1 });

        let rt = runtimes.find("u", "b").await.unwrap().unwrap();
        assert_eq!(rt.phase, RuntimePhase::Running);
        assert_eq!(rt.task_id.as_deref(), Some("task-1"));
        assert_eq!(rt.version, 1);
        assert_eq!(rt.observed_at, 1_700_000_000);
    }

    #[tokio::test]
    async fn preserves_version_from_prior_runtime() {
        let runtimes = Arc::new(InMemoryRuntimes::default());
        runtimes
            .record(&BotRuntime::running("u".into(), "b".into(), "task-old".into(), 7, 1_699_000_000))
            .await
            .unwrap();
        let uc = RecordRunningTaskUseCase::new(runtimes.clone());

        let outcome = uc.execute("u", "b", "task-new", 1_700_000_000).await.unwrap();
        assert_eq!(outcome, RecordRunningOutcome::Recorded { version: 7 });

        let rt = runtimes.find("u", "b").await.unwrap().unwrap();
        assert_eq!(rt.task_id.as_deref(), Some("task-new"));
        assert_eq!(rt.version, 7, "observation must not bump the restart counter");
    }

    #[tokio::test]
    async fn skips_stale_event_older_than_last_observation() {
        let runtimes = Arc::new(InMemoryRuntimes::default());
        // We already observed a stop at t=2000.
        runtimes
            .record(&BotRuntime::stopped("u".into(), "b".into(), 3, 2000))
            .await
            .unwrap();
        let uc = RecordRunningTaskUseCase::new(runtimes.clone());

        // A reordered RUNNING event timestamped t=1000 must not resurrect it.
        let outcome = uc.execute("u", "b", "task-stale", 1000).await.unwrap();
        assert_eq!(outcome, RecordRunningOutcome::SkippedStale);

        let rt = runtimes.find("u", "b").await.unwrap().unwrap();
        assert_eq!(rt.phase, RuntimePhase::Stopped, "stale running must not flip it back to running");
    }

    #[tokio::test]
    async fn skips_running_at_same_second_as_recorded_stop() {
        let runtimes = Arc::new(InMemoryRuntimes::default());
        // Stop observed at t=2000.
        runtimes
            .record(&BotRuntime::stopped("u".into(), "b".into(), 3, 2000))
            .await
            .unwrap();
        let uc = RecordRunningTaskUseCase::new(runtimes.clone());

        // A RUNNING event on the SAME second must not resurrect the stop (tie -> stopped wins).
        let outcome = uc.execute("u", "b", "task-tie", 2000).await.unwrap();
        assert_eq!(outcome, RecordRunningOutcome::SkippedStale);

        let rt = runtimes.find("u", "b").await.unwrap().unwrap();
        assert_eq!(rt.phase, RuntimePhase::Stopped);
    }
}
