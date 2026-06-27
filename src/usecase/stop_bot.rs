use crate::domain::bot::BotRepository;
use crate::domain::clock::Clock;
use crate::domain::runtime::{BotRuntime, BotRuntimeRepository, RuntimePhase};
use crate::usecase::stop_task::{TaskController, TaskLiveness};
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq)]
pub enum StopOutcome {
    Stopped {
        task_id: String,
    },
    NotRunning,
    /// A launch is mid-flight and its task id is not recorded yet, so the task
    /// cannot be located to stop. Desired state is already off; a retry once the
    /// RUNNING event lands will stop it.
    StartInProgress,
    /// The task is already winding down (a prior Stop is still in flight); there
    /// is nothing new to do.
    AlreadyStopping,
    BotNotFound,
}

/// Turns the user's "Stop bot" intent into an ECS StopTask.
///
/// Desired state is flipped OFF first so that the STOPPED event from our own
/// StopTask (and any racing observation) is reconciled as user-initiated and
/// never auto-restarted. The task to stop is located by the task id recorded on
/// the runtime row, read strongly-consistently so a just-started task is seen.
pub struct StopBotUseCase {
    bots: Arc<dyn BotRepository>,
    runtimes: Arc<dyn BotRuntimeRepository>,
    stopper: Arc<dyn TaskController>,
    clock: Arc<dyn Clock>,
    cluster_arn: String,
}

impl StopBotUseCase {
    pub fn new(
        bots: Arc<dyn BotRepository>,
        runtimes: Arc<dyn BotRuntimeRepository>,
        stopper: Arc<dyn TaskController>,
        clock: Arc<dyn Clock>,
        cluster_arn: String,
    ) -> Self {
        Self {
            bots,
            runtimes,
            stopper,
            clock,
            cluster_arn,
        }
    }

    pub async fn execute(&self, user_id: &str, bot_id: &str) -> Result<StopOutcome, String> {
        let mut bot = match self.bots.find(user_id, bot_id).await {
            Some(b) => b,
            None => return Ok(StopOutcome::BotNotFound),
        };

        // Desired state OFF first — this is what stops the reconcile Lambda from
        // restarting the task when its STOPPED event arrives.
        let now = self.clock.now();
        bot.disable(now);
        self.bots.save(&bot).await.map_err(|e| e.to_string())?;

        let runtime = self
            .runtimes
            .find_consistent(user_id, bot_id)
            .await
            .map_err(|e| e.to_string())?;
        match runtime {
            Some(rt) if matches!(rt.phase, RuntimePhase::Running | RuntimePhase::Starting) => {
                let version = rt.version;
                match rt.task_id {
                    Some(task_id) => {
                        match self
                            .stopper
                            .stop(&self.cluster_arn, &task_id, "stopped by user via telebot")
                            .await
                        {
                            Ok(()) => {
                                // Stamp observed Stopping so the UI shows the wind-down
                                // and a racing Run sees `stopping` rather than a stale
                                // `running`. Best-effort: a failed stamp must not fail
                                // the stop — the Lambda's STOPPED event still settles it.
                                if let Err(e) = self
                                    .runtimes
                                    .record(&BotRuntime::stopping(
                                        user_id.to_string(),
                                        bot_id.to_string(),
                                        task_id.clone(),
                                        version,
                                        now,
                                    ))
                                    .await
                                {
                                    tracing::warn!(
                                        "failed to record stopping phase for bot {bot_id}: {e}"
                                    );
                                }
                                Ok(StopOutcome::Stopped { task_id })
                            }
                            // StopTask failed. If ECS confirms the task is already
                            // gone, the runtime row was stale (a missed STOPPED event)
                            // — reconcile observed to Stopped and report success rather
                            // than a hard error. Only a stop failure on a task that is
                            // still Alive (or whose liveness can't be confirmed) is a
                            // real error worth surfacing.
                            Err(stop_err) => {
                                match self.stopper.liveness(&self.cluster_arn, &task_id).await {
                                    Ok(TaskLiveness::Gone) => {
                                        if let Err(e) = self
                                            .runtimes
                                            .record(&BotRuntime::stopped(
                                                user_id.to_string(),
                                                bot_id.to_string(),
                                                version,
                                                now,
                                            ))
                                            .await
                                        {
                                            tracing::warn!(
                                                "failed to reconcile stopped for gone task, bot {bot_id}: {e}"
                                            );
                                        }
                                        Ok(StopOutcome::NotRunning)
                                    }
                                    _ => Err(stop_err.to_string()),
                                }
                            }
                        }
                    }
                    None => Ok(StopOutcome::StartInProgress),
                }
            }
            Some(rt) if rt.phase == RuntimePhase::Stopping => Ok(StopOutcome::AlreadyStopping),
            _ => Ok(StopOutcome::NotRunning),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::bot::Bot;
    use crate::domain::error::DomainError;
    use crate::domain::exchange::Exchange;
    use crate::domain::runtime::BotRuntime;
    use anyhow::Result;
    use async_trait::async_trait;
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
        bots: Mutex<HashMap<(String, String), Bot>>,
    }
    impl InMemoryBots {
        fn with(bot: Bot) -> Self {
            let mut map = HashMap::new();
            map.insert((bot.user_id.clone(), bot.id.clone()), bot);
            Self {
                bots: Mutex::new(map),
            }
        }
        fn get(&self, user_id: &str, bot_id: &str) -> Option<Bot> {
            self.bots
                .lock()
                .unwrap()
                .get(&(user_id.to_string(), bot_id.to_string()))
                .cloned()
        }
    }
    #[async_trait]
    impl BotRepository for InMemoryBots {
        async fn find(&self, user_id: &str, bot_id: &str) -> Option<Bot> {
            self.get(user_id, bot_id)
        }
        async fn save(&self, bot: &Bot) -> Result<(), DomainError> {
            self.bots
                .lock()
                .unwrap()
                .insert((bot.user_id.clone(), bot.id.clone()), bot.clone());
            Ok(())
        }
        async fn find_by_user_id(&self, user_id: &str) -> Vec<Bot> {
            self.bots
                .lock()
                .unwrap()
                .values()
                .filter(|b| b.user_id == user_id)
                .cloned()
                .collect()
        }
        async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), String> {
            self.bots
                .lock()
                .unwrap()
                .remove(&(user_id.to_string(), bot_id.to_string()));
            Ok(())
        }
    }

    #[derive(Default)]
    struct InMemoryRuntimes {
        runtimes: Mutex<HashMap<(String, String), BotRuntime>>,
    }
    impl InMemoryRuntimes {
        fn with(rt: BotRuntime) -> Self {
            let mut map = HashMap::new();
            map.insert((rt.user_id.clone(), rt.bot_id.clone()), rt);
            Self {
                runtimes: Mutex::new(map),
            }
        }
    }
    #[async_trait]
    impl BotRuntimeRepository for InMemoryRuntimes {
        async fn find(
            &self,
            user_id: &str,
            bot_id: &str,
        ) -> Result<Option<BotRuntime>, DomainError> {
            Ok(self
                .runtimes
                .lock()
                .unwrap()
                .get(&(user_id.to_string(), bot_id.to_string()))
                .cloned())
        }
        async fn record(&self, runtime: &BotRuntime) -> Result<(), DomainError> {
            self.runtimes.lock().unwrap().insert(
                (runtime.user_id.clone(), runtime.bot_id.clone()),
                runtime.clone(),
            );
            Ok(())
        }
    }

    #[derive(Default)]
    struct MockStopper {
        stopped: Mutex<Vec<String>>,
        /// When true, `stop` returns an error (simulating an ECS StopTask failure).
        fail_stop: bool,
        /// Liveness answer; defaults to `Gone` to match the common case.
        liveness: Option<TaskLiveness>,
    }
    #[async_trait]
    impl TaskController for MockStopper {
        async fn stop(&self, _cluster_arn: &str, task_id: &str, _reason: &str) -> Result<()> {
            if self.fail_stop {
                return Err(anyhow::anyhow!("ecs stop_task failed"));
            }
            self.stopped.lock().unwrap().push(task_id.to_string());
            Ok(())
        }
        async fn liveness(&self, _cluster_arn: &str, _task_id: &str) -> Result<TaskLiveness> {
            Ok(self.liveness.unwrap_or(TaskLiveness::Gone))
        }
    }

    fn bot(enabled: bool) -> Bot {
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

    fn starting_runtime(task_id: Option<String>) -> BotRuntime {
        BotRuntime {
            user_id: "user-1".to_string(),
            bot_id: "bot-1".to_string(),
            task_id,
            phase: RuntimePhase::Starting,
            version: 1,
            observed_at: 1_699_999_000,
        }
    }

    fn use_case(
        bots: Arc<InMemoryBots>,
        runtimes: Arc<InMemoryRuntimes>,
        stopper: Arc<MockStopper>,
    ) -> StopBotUseCase {
        StopBotUseCase::new(
            bots,
            runtimes,
            stopper,
            Arc::new(FixedClock),
            "cluster".to_string(),
        )
    }

    #[tokio::test]
    async fn running_bot_is_stopped_and_disabled() {
        let bots = Arc::new(InMemoryBots::with(bot(true)));
        let runtimes = Arc::new(InMemoryRuntimes::with(BotRuntime::running(
            "user-1".to_string(),
            "bot-1".to_string(),
            "task-xyz".to_string(),
            3,
            1_699_999_000,
        )));
        let stopper = Arc::new(MockStopper::default());
        let uc = use_case(bots.clone(), runtimes, stopper.clone());

        let out = uc.execute("user-1", "bot-1").await.unwrap();
        assert_eq!(
            out,
            StopOutcome::Stopped {
                task_id: "task-xyz".to_string()
            }
        );
        assert!(
            !bots.get("user-1", "bot-1").unwrap().enabled,
            "desired state flipped off"
        );
        assert_eq!(
            *stopper.stopped.lock().unwrap(),
            vec!["task-xyz".to_string()]
        );
    }

    #[tokio::test]
    async fn starting_bot_with_task_id_is_stopped() {
        let bots = Arc::new(InMemoryBots::with(bot(true)));
        let runtimes = Arc::new(InMemoryRuntimes::with(starting_runtime(Some(
            "task-new".to_string(),
        ))));
        let stopper = Arc::new(MockStopper::default());
        let uc = use_case(bots, runtimes, stopper.clone());

        let out = uc.execute("user-1", "bot-1").await.unwrap();
        assert_eq!(
            out,
            StopOutcome::Stopped {
                task_id: "task-new".to_string()
            }
        );
        assert_eq!(
            *stopper.stopped.lock().unwrap(),
            vec!["task-new".to_string()]
        );
    }

    #[tokio::test]
    async fn starting_bot_without_task_id_reports_start_in_progress() {
        let bots = Arc::new(InMemoryBots::with(bot(true)));
        let runtimes = Arc::new(InMemoryRuntimes::with(starting_runtime(None)));
        let stopper = Arc::new(MockStopper::default());
        let uc = use_case(bots.clone(), runtimes, stopper.clone());

        let out = uc.execute("user-1", "bot-1").await.unwrap();
        assert_eq!(out, StopOutcome::StartInProgress);
        assert!(
            !bots.get("user-1", "bot-1").unwrap().enabled,
            "desired off even when task id unknown"
        );
        assert!(
            stopper.stopped.lock().unwrap().is_empty(),
            "nothing to stop yet"
        );
    }

    #[tokio::test]
    async fn stopped_bot_reports_not_running() {
        let bots = Arc::new(InMemoryBots::with(bot(true)));
        let runtimes = Arc::new(InMemoryRuntimes::with(BotRuntime::stopped(
            "user-1".to_string(),
            "bot-1".to_string(),
            4,
            1_699_999_000,
        )));
        let stopper = Arc::new(MockStopper::default());
        let uc = use_case(bots, runtimes, stopper.clone());

        let out = uc.execute("user-1", "bot-1").await.unwrap();
        assert_eq!(out, StopOutcome::NotRunning);
        assert!(stopper.stopped.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn no_runtime_row_reports_not_running() {
        let bots = Arc::new(InMemoryBots::with(bot(true)));
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let stopper = Arc::new(MockStopper::default());
        let uc = use_case(bots, runtimes, stopper.clone());

        let out = uc.execute("user-1", "bot-1").await.unwrap();
        assert_eq!(out, StopOutcome::NotRunning);
    }

    #[tokio::test]
    async fn missing_bot_returns_not_found() {
        let bots = Arc::new(InMemoryBots::default());
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let stopper = Arc::new(MockStopper::default());
        let uc = use_case(bots, runtimes, stopper);

        let out = uc.execute("user-1", "ghost").await.unwrap();
        assert_eq!(out, StopOutcome::BotNotFound);
    }

    #[tokio::test]
    async fn stop_records_stopping_phase_keeping_task_id_and_version() {
        let bots = Arc::new(InMemoryBots::with(bot(true)));
        let runtimes = Arc::new(InMemoryRuntimes::with(BotRuntime::running(
            "user-1".to_string(),
            "bot-1".to_string(),
            "task-xyz".to_string(),
            3,
            1_699_999_000,
        )));
        let stopper = Arc::new(MockStopper::default());
        let uc = use_case(bots, runtimes.clone(), stopper);

        let out = uc.execute("user-1", "bot-1").await.unwrap();
        assert_eq!(
            out,
            StopOutcome::Stopped {
                task_id: "task-xyz".to_string()
            }
        );

        let rt = runtimes.find("user-1", "bot-1").await.unwrap().unwrap();
        assert_eq!(
            rt.phase,
            RuntimePhase::Stopping,
            "observed flips to stopping"
        );
        assert_eq!(rt.task_id.as_deref(), Some("task-xyz"));
        assert_eq!(rt.version, 3, "stopping preserves the version");
    }

    #[tokio::test]
    async fn second_stop_on_stopping_reports_already_stopping_without_re_stopping() {
        let bots = Arc::new(InMemoryBots::with(bot(true)));
        let runtimes = Arc::new(InMemoryRuntimes::with(BotRuntime::stopping(
            "user-1".to_string(),
            "bot-1".to_string(),
            "task-xyz".to_string(),
            3,
            1_699_999_000,
        )));
        let stopper = Arc::new(MockStopper::default());
        let uc = use_case(bots, runtimes, stopper.clone());

        let out = uc.execute("user-1", "bot-1").await.unwrap();
        assert_eq!(out, StopOutcome::AlreadyStopping);
        assert!(
            stopper.stopped.lock().unwrap().is_empty(),
            "no second StopTask for an already-stopping bot"
        );
    }

    #[tokio::test]
    async fn stop_on_a_gone_task_reconciles_to_stopped_instead_of_erroring() {
        // Runtime row is stale: it claims Running with a task ECS no longer has
        // (a missed STOPPED event). StopTask fails, liveness reports Gone.
        let bots = Arc::new(InMemoryBots::with(bot(true)));
        let runtimes = Arc::new(InMemoryRuntimes::with(BotRuntime::running(
            "user-1".to_string(),
            "bot-1".to_string(),
            "ghost-task".to_string(),
            2,
            1_699_999_000,
        )));
        let stopper = Arc::new(MockStopper {
            fail_stop: true,
            liveness: Some(TaskLiveness::Gone),
            ..Default::default()
        });
        let uc = use_case(bots.clone(), runtimes.clone(), stopper);

        let out = uc.execute("user-1", "bot-1").await.unwrap();
        assert_eq!(
            out,
            StopOutcome::NotRunning,
            "a gone task self-heals to stopped rather than surfacing an error"
        );
        assert!(
            !bots.get("user-1", "bot-1").unwrap().enabled,
            "desired still off"
        );
        let rt = runtimes.find("user-1", "bot-1").await.unwrap().unwrap();
        assert_eq!(rt.phase, RuntimePhase::Stopped);
        assert_eq!(rt.task_id, None);
    }

    #[tokio::test]
    async fn stop_failure_on_a_still_live_task_surfaces_the_error() {
        // StopTask fails but ECS says the task is still Alive — this is a real
        // error (e.g. transient/permissions), not a stale row, so do not pretend
        // it stopped.
        let bots = Arc::new(InMemoryBots::with(bot(true)));
        let runtimes = Arc::new(InMemoryRuntimes::with(BotRuntime::running(
            "user-1".to_string(),
            "bot-1".to_string(),
            "live-task".to_string(),
            2,
            1_699_999_000,
        )));
        let stopper = Arc::new(MockStopper {
            fail_stop: true,
            liveness: Some(TaskLiveness::Alive),
            ..Default::default()
        });
        let uc = use_case(bots, runtimes.clone(), stopper);

        let err = uc.execute("user-1", "bot-1").await.unwrap_err();
        assert!(
            !err.is_empty(),
            "a live task whose stop failed is a real error"
        );
        let rt = runtimes.find("user-1", "bot-1").await.unwrap().unwrap();
        assert_eq!(
            rt.phase,
            RuntimePhase::Running,
            "observed state is not falsely reconciled when the task is still alive"
        );
    }
}
