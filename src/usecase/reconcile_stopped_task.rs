use std::sync::Arc;
use anyhow::Result;
use crate::domain::bot::BotRepository;
use crate::domain::runtime::{BotRuntime, BotRuntimeRepository, StartClaim, StartLockRepository};
use crate::usecase::run_task::TaskRunner;
use crate::usecase::stop_task::TaskController;

/// Why a task stopped (parsed from the ECS event by the Lambda).
pub struct StopInfo { pub exit_code: i32, pub stop_code: String }
impl StopInfo {
    pub fn is_memory_related(&self) -> bool {
        self.exit_code == 137 && !self.stop_code.contains("UserInitiated")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReconcileOutcome {
    Restarted { task_id: String },
    SkippedNotEnabled,        // user intent is OFF -> do NOT restart
    SkippedNotMemoryRelated,
    SkippedSuperseded,        // the stopped task is no longer current (e.g. duplicate STOPPED)
    BotNotFound,
}

pub struct ReconcileStoppedTaskUseCase {
    bots: Arc<dyn BotRepository>,
    runtimes: Arc<dyn BotRuntimeRepository>,
    locks: Arc<dyn StartLockRepository>,
    run_task: Arc<dyn TaskRunner>,
    stopper: Arc<dyn TaskController>,
}

impl ReconcileStoppedTaskUseCase {
    pub fn new(bots: Arc<dyn BotRepository>, runtimes: Arc<dyn BotRuntimeRepository>, locks: Arc<dyn StartLockRepository>, run_task: Arc<dyn TaskRunner>, stopper: Arc<dyn TaskController>) -> Self {
        Self { bots, runtimes, locks, run_task, stopper }
    }

    /// `observed_at` is the EventBridge event time (may lag wall-clock when the
    /// event is delivered late); it stamps observed-state writes so the
    /// repository's monotonic conditional write orders events consistently. `now`
    /// is fresh wall-clock seconds and stamps the start lock, so a just-claimed
    /// restart lock can never already look stale to a concurrent telebot start.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute(&self, user_id: &str, bot_id: &str, stopped_task_id: &str, cluster_arn: &str, td_arn: &str, container_name: &str, stop: StopInfo, observed_at: i64, now: i64) -> Result<ReconcileOutcome> {
        // Compute prev_version up front: it does not depend on the bot, and we
        // need it on the bot-not-found path to record a stopped runtime.
        let prev_version = self.runtimes.find(user_id, bot_id).await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .map(|r| r.version).unwrap_or(0);

        let bot = match self.bots.find(user_id, bot_id).await {
            Some(b) => b,
            None => {
                // A bot that no longer exists must not be left showing Running.
                let _ = self.runtimes.record(&BotRuntime::stopped(user_id.to_string(), bot_id.to_string(), prev_version, observed_at)).await;
                return Ok(ReconcileOutcome::BotNotFound);
            }
        };

        // Desired state OFF (user manually stopped) -> reflect stopped, never restart. THIS is the rule the old Lambda was missing.
        if !bot.enabled {
            self.runtimes.record(&BotRuntime::stopped(user_id.to_string(), bot_id.to_string(), prev_version, observed_at)).await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            return Ok(ReconcileOutcome::SkippedNotEnabled);
        }
        if !stop.is_memory_related() {
            self.runtimes.record(&BotRuntime::stopped(user_id.to_string(), bot_id.to_string(), prev_version, observed_at)).await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            return Ok(ReconcileOutcome::SkippedNotMemoryRelated);
        }

        // Claim the restart through the same exclusive lock the telebot uses,
        // keyed on the stopped task. Only the claim that finds this task still
        // current launches, so a duplicate STOPPED event (EventBridge is
        // at-least-once) cannot spawn a second live-trading task. The lock is
        // stamped with wall-clock `now`, not the (possibly stale) event time, so
        // a concurrent telebot start cannot mistake this fresh lock for an
        // abandoned one and reclaim it.
        match self.locks.try_acquire_restart(user_id, bot_id, stopped_task_id, now).await
            .map_err(|e| anyhow::anyhow!(e.to_string()))? {
            StartClaim::Acquired => {}
            // The stopped task was already replaced or claimed by another launcher;
            // a task is or will be running, so do not relaunch and do not record stopped.
            _ => return Ok(ReconcileOutcome::SkippedSuperseded),
        }

        // Re-validate desired state inside the held lock: the user may have
        // disabled the bot between the read above and the claim. Roll the lock
        // back rather than resurrect a bot that was just turned off.
        let still_enabled = self.bots.find_consistent(user_id, bot_id).await.map(|b| b.enabled).unwrap_or(false);
        if !still_enabled {
            let _ = self.locks.release_start(user_id, bot_id, now).await;
            return Ok(ReconcileOutcome::SkippedNotEnabled);
        }

        // Lock held (row is `starting`): launch, then attach. On failure release
        // the lock back to stopped so the bot can be started again.
        let task_id = match self.run_task.run(user_id, bot_id, cluster_arn, td_arn, container_name).await {
            Ok(id) => id,
            Err(e) => {
                let _ = self.locks.release_start(user_id, bot_id, now).await;
                return Err(e);
            }
        };
        self.locks.attach_started_task(user_id, bot_id, &task_id).await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        // Post-launch re-check: a disable that landed during the launch window —
        // after the gate above but before the task id was attached — is invisible
        // to StopBot (it saw no task id and returned without stopping). Now that the
        // id is attached, stop the task ourselves so it never trades against an OFF
        // intent. The StopTask makes ECS stamp the next STOPPED as UserInitiated, so
        // it is not auto-restarted.
        let still_enabled = self.bots.find_consistent(user_id, bot_id).await.map(|b| b.enabled).unwrap_or(false);
        if !still_enabled {
            let _ = self.stopper.stop(cluster_arn, &task_id, "stopped: bot disabled during restart").await;
            return Ok(ReconcileOutcome::SkippedNotEnabled);
        }
        Ok(ReconcileOutcome::Restarted { task_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::runtime::RuntimePhase;
    use crate::usecase::run_task::RunTaskUseCase;

    // Fixed event time used by the behaviour tests below.
    const EVENT_AT: i64 = 1_700_000_000;

    #[test]
    fn memory_related_when_137_and_not_user_initiated() {
        let stop = StopInfo { exit_code: 137, stop_code: "TaskFailedToStart".to_string() };
        assert!(stop.is_memory_related());
    }

    #[test]
    fn not_memory_related_when_137_but_user_initiated() {
        let stop = StopInfo { exit_code: 137, stop_code: "UserInitiated".to_string() };
        assert!(!stop.is_memory_related());
    }

    #[test]
    fn not_memory_related_when_other_exit_code() {
        let stop = StopInfo { exit_code: 0, stop_code: "TaskFailedToStart".to_string() };
        assert!(!stop.is_memory_related());
    }

    // --- Use-case behaviour tests with in-memory mock repositories ---

    use std::collections::HashMap;
    use std::sync::Mutex;
    use crate::domain::bot::Bot;
    use crate::domain::error::DomainError;
    use crate::domain::exchange::Exchange;
    use async_trait::async_trait;

    /// In-memory BotRepository keyed by (user_id, bot_id).
    #[derive(Default)]
    struct InMemoryBots {
        bots: Mutex<HashMap<(String, String), Bot>>,
    }
    impl InMemoryBots {
        fn with(bot: Bot) -> Self {
            let mut map = HashMap::new();
            map.insert((bot.user_id.clone(), bot.id.clone()), bot);
            Self { bots: Mutex::new(map) }
        }
    }
    #[async_trait]
    impl BotRepository for InMemoryBots {
        async fn find(&self, user_id: &str, bot_id: &str) -> Option<Bot> {
            self.bots.lock().unwrap().get(&(user_id.to_string(), bot_id.to_string())).cloned()
        }
        async fn save(&self, bot: &Bot) -> Result<(), DomainError> {
            self.bots.lock().unwrap().insert((bot.user_id.clone(), bot.id.clone()), bot.clone());
            Ok(())
        }
        async fn find_by_user_id(&self, user_id: &str) -> Vec<Bot> {
            self.bots.lock().unwrap().values().filter(|b| b.user_id == user_id).cloned().collect()
        }
        async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), String> {
            self.bots.lock().unwrap().remove(&(user_id.to_string(), bot_id.to_string()));
            Ok(())
        }
    }

    /// In-memory BotRuntimeRepository that records the last runtime written.
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
            self.runtimes.lock().unwrap().insert(
                (runtime.user_id.clone(), runtime.bot_id.clone()),
                runtime.clone(),
            );
            Ok(())
        }
    }

    /// In-memory StartLockRepository for the restart path: returns a preset
    /// restart claim and records lifecycle calls. The real CAS is integration-tested.
    struct MockLock {
        restart_claim: StartClaim,
        restart_calls: Mutex<usize>,
        attached: Mutex<Option<String>>,
        released: Mutex<usize>,
    }
    impl MockLock {
        fn new(restart_claim: StartClaim) -> Self {
            Self { restart_claim, restart_calls: Mutex::new(0), attached: Mutex::new(None), released: Mutex::new(0) }
        }
    }
    #[async_trait]
    impl StartLockRepository for MockLock {
        async fn try_acquire_start(&self, _u: &str, _b: &str, _now: i64, _stale: i64) -> Result<StartClaim, DomainError> {
            Ok(StartClaim::Acquired)
        }
        async fn try_acquire_restart(&self, _u: &str, _b: &str, _stopped: &str, _now: i64) -> Result<StartClaim, DomainError> {
            *self.restart_calls.lock().unwrap() += 1;
            Ok(self.restart_claim.clone())
        }
        async fn attach_started_task(&self, _u: &str, _b: &str, task_id: &str) -> Result<(), DomainError> {
            *self.attached.lock().unwrap() = Some(task_id.to_string());
            Ok(())
        }
        async fn release_start(&self, _u: &str, _b: &str, _now: i64) -> Result<(), DomainError> {
            *self.released.lock().unwrap() += 1;
            Ok(())
        }
    }

    /// In-memory TaskController recording StopTask calls; liveness is unused here.
    #[derive(Default)]
    struct MockStopper {
        stops: Mutex<Vec<String>>,
    }
    #[async_trait]
    impl TaskController for MockStopper {
        async fn stop(&self, _cluster_arn: &str, task_id: &str, _reason: &str) -> Result<()> {
            self.stops.lock().unwrap().push(task_id.to_string());
            Ok(())
        }
        async fn liveness(&self, _cluster_arn: &str, _task_id: &str) -> Result<crate::usecase::stop_task::TaskLiveness> {
            Ok(crate::usecase::stop_task::TaskLiveness::Gone)
        }
    }

    /// Build a dummy ECS client that is never actually invoked on the skip paths.
    fn dummy_ecs_client() -> aws_sdk_ecs::Client {
        let creds = aws_sdk_ecs::config::Credentials::new(
            "test", "test", None, None, "pbtb-tests",
        );
        let conf = aws_sdk_ecs::config::Builder::new()
            .behavior_version(aws_sdk_ecs::config::BehaviorVersion::latest())
            .region(aws_sdk_ecs::config::Region::new("us-east-1"))
            .credentials_provider(creds)
            .build();
        aws_sdk_ecs::Client::from_conf(conf)
    }

    fn enabled_bot(enabled: bool) -> Bot {
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

    #[tokio::test]
    async fn disabled_bot_oom_skips_and_records_stopped_without_restart() {
        let bots = Arc::new(InMemoryBots::with(enabled_bot(false)));
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let run_task = Arc::new(RunTaskUseCase::new(dummy_ecs_client()));
        let locks = Arc::new(MockLock::new(StartClaim::Acquired));
        let uc = ReconcileStoppedTaskUseCase::new(
            bots.clone(),
            runtimes.clone(),
            locks,
            run_task,
            Arc::new(MockStopper::default()),
        );

        let outcome = uc
            .execute(
                "user-1",
                "bot-1",
                "stopped-task",
                "cluster",
                "td",
                "container",
                StopInfo { exit_code: 137, stop_code: "TaskFailedToStart".to_string() },
                EVENT_AT,
                EVENT_AT,
            )
            .await
            .unwrap();

        assert_eq!(outcome, ReconcileOutcome::SkippedNotEnabled);
        // A stopped runtime was recorded (run_task never reached, so phase is Stopped).
        let rt = runtimes.find("user-1", "bot-1").await.unwrap().unwrap();
        assert_eq!(rt.phase, RuntimePhase::Stopped);
        assert_eq!(rt.task_id, None);
        assert_eq!(rt.observed_at, EVENT_AT);
    }

    #[tokio::test]
    async fn enabled_bot_non_memory_stop_skips_without_restart() {
        let bots = Arc::new(InMemoryBots::with(enabled_bot(true)));
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let run_task = Arc::new(RunTaskUseCase::new(dummy_ecs_client()));
        let locks = Arc::new(MockLock::new(StartClaim::Acquired));
        let uc = ReconcileStoppedTaskUseCase::new(
            bots,
            runtimes.clone(),
            locks,
            run_task,
            Arc::new(MockStopper::default()),
        );

        // exit 0 => not memory related.
        let outcome = uc
            .execute(
                "user-1",
                "bot-1",
                "stopped-task",
                "cluster",
                "td",
                "container",
                StopInfo { exit_code: 0, stop_code: "EssentialContainerExited".to_string() },
                EVENT_AT,
                EVENT_AT,
            )
            .await
            .unwrap();
        assert_eq!(outcome, ReconcileOutcome::SkippedNotMemoryRelated);

        // 137 but UserInitiated => also not memory related.
        let outcome2 = uc
            .execute(
                "user-1",
                "bot-1",
                "stopped-task",
                "cluster",
                "td",
                "container",
                StopInfo { exit_code: 137, stop_code: "UserInitiated".to_string() },
                EVENT_AT,
                EVENT_AT,
            )
            .await
            .unwrap();
        assert_eq!(outcome2, ReconcileOutcome::SkippedNotMemoryRelated);

        let rt = runtimes.find("user-1", "bot-1").await.unwrap().unwrap();
        assert_eq!(rt.phase, RuntimePhase::Stopped);
    }

    #[tokio::test]
    async fn missing_bot_returns_bot_not_found() {
        let bots = Arc::new(InMemoryBots::default());
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let run_task = Arc::new(RunTaskUseCase::new(dummy_ecs_client()));
        let locks = Arc::new(MockLock::new(StartClaim::Acquired));
        let uc = ReconcileStoppedTaskUseCase::new(bots, runtimes.clone(), locks, run_task, Arc::new(MockStopper::default()));

        let outcome = uc
            .execute(
                "user-1",
                "ghost",
                "stopped-task",
                "cluster",
                "td",
                "container",
                StopInfo { exit_code: 137, stop_code: "TaskFailedToStart".to_string() },
                EVENT_AT,
                EVENT_AT,
            )
            .await
            .unwrap();
        assert_eq!(outcome, ReconcileOutcome::BotNotFound);

        // A bot that no longer exists must be recorded as stopped, not left Running.
        let rt = runtimes.find("user-1", "ghost").await.unwrap().unwrap();
        assert_eq!(rt.phase, RuntimePhase::Stopped);
    }

    /// Mock TaskRunner that returns a fixed task id and counts invocations.
    struct MockTaskRunner {
        task_id: String,
        calls: Mutex<usize>,
    }
    impl MockTaskRunner {
        fn new(task_id: &str) -> Self {
            Self { task_id: task_id.to_string(), calls: Mutex::new(0) }
        }
        fn call_count(&self) -> usize {
            *self.calls.lock().unwrap()
        }
    }
    #[async_trait]
    impl crate::usecase::run_task::TaskRunner for MockTaskRunner {
        async fn run(&self, _user_id: &str, _bot_id: &str, _cluster_arn: &str, _td_arn: &str, _container_name: &str) -> Result<String> {
            *self.calls.lock().unwrap() += 1;
            Ok(self.task_id.clone())
        }
    }

    #[tokio::test]
    async fn enabled_bot_oom_claims_lock_launches_and_attaches() {
        let bots = Arc::new(InMemoryBots::with(enabled_bot(true)));
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let runner = Arc::new(MockTaskRunner::new("task-xyz"));
        let locks = Arc::new(MockLock::new(StartClaim::Acquired));
        let uc = ReconcileStoppedTaskUseCase::new(
            bots,
            runtimes.clone(),
            locks.clone(),
            runner.clone(),
            Arc::new(MockStopper::default()),
        );

        // OOM stop: exit 137, not UserInitiated.
        let outcome = uc
            .execute(
                "user-1",
                "bot-1",
                "old-task",
                "cluster",
                "td",
                "container",
                StopInfo { exit_code: 137, stop_code: "TaskFailedToStart".to_string() },
                EVENT_AT,
                EVENT_AT,
            )
            .await
            .unwrap();

        assert_eq!(outcome, ReconcileOutcome::Restarted { task_id: "task-xyz".to_string() });
        assert_eq!(*locks.restart_calls.lock().unwrap(), 1, "restart claimed through the lock");
        assert_eq!(runner.call_count(), 1, "task runner invoked exactly once");
        assert_eq!(*locks.attached.lock().unwrap(), Some("task-xyz".to_string()), "new task id attached to the lock");
    }

    #[tokio::test]
    async fn duplicate_stopped_event_does_not_relaunch() {
        let bots = Arc::new(InMemoryBots::with(enabled_bot(true)));
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let runner = Arc::new(MockTaskRunner::new("task-xyz"));
        // The stopped task is no longer current -> the lock refuses the claim.
        let locks = Arc::new(MockLock::new(StartClaim::AlreadyRunning));
        let uc = ReconcileStoppedTaskUseCase::new(
            bots,
            runtimes.clone(),
            locks.clone(),
            runner.clone(),
            Arc::new(MockStopper::default()),
        );

        let outcome = uc
            .execute(
                "user-1",
                "bot-1",
                "old-task",
                "cluster",
                "td",
                "container",
                StopInfo { exit_code: 137, stop_code: "TaskFailedToStart".to_string() },
                EVENT_AT,
                EVENT_AT,
            )
            .await
            .unwrap();

        assert_eq!(outcome, ReconcileOutcome::SkippedSuperseded);
        assert_eq!(runner.call_count(), 0, "a superseded stop must not launch a replacement");
        assert_eq!(*locks.attached.lock().unwrap(), None);
    }

    /// Reports enabled on the first read but disabled on the strongly-consistent
    /// re-read, simulating the user disabling the bot inside the restart window.
    struct DisableDuringClaimBots;
    #[async_trait]
    impl BotRepository for DisableDuringClaimBots {
        async fn find(&self, _u: &str, _b: &str) -> Option<Bot> { Some(enabled_bot(true)) }
        async fn find_consistent(&self, _u: &str, _b: &str) -> Option<Bot> { Some(enabled_bot(false)) }
        async fn save(&self, _bot: &Bot) -> Result<(), DomainError> { Ok(()) }
        async fn find_by_user_id(&self, _u: &str) -> Vec<Bot> { vec![] }
        async fn delete(&self, _u: &str, _b: &str) -> Result<(), String> { Ok(()) }
    }

    #[tokio::test]
    async fn disable_during_restart_claim_releases_without_launching() {
        let bots = Arc::new(DisableDuringClaimBots);
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let runner = Arc::new(MockTaskRunner::new("task-xyz"));
        let locks = Arc::new(MockLock::new(StartClaim::Acquired));
        let uc = ReconcileStoppedTaskUseCase::new(bots, runtimes, locks.clone(), runner.clone(), Arc::new(MockStopper::default()));

        let outcome = uc
            .execute(
                "user-1",
                "bot-1",
                "old-task",
                "cluster",
                "td",
                "container",
                StopInfo { exit_code: 137, stop_code: "TaskFailedToStart".to_string() },
                EVENT_AT,
                EVENT_AT,
            )
            .await
            .unwrap();

        assert_eq!(outcome, ReconcileOutcome::SkippedNotEnabled, "a bot disabled mid-claim must not be relaunched");
        assert_eq!(runner.call_count(), 0, "no launch after the bot was disabled");
        assert_eq!(*locks.released.lock().unwrap(), 1, "the lock was rolled back");
    }

    /// Reports enabled on the first strongly-consistent read (the pre-launch gate
    /// passes) and disabled on the second (the post-launch re-check), simulating a
    /// disable that lands during the launch window, after the id is unknowable to StopBot.
    #[derive(Default)]
    struct DisableAfterLaunchBots {
        consistent_reads: Mutex<usize>,
    }
    #[async_trait]
    impl BotRepository for DisableAfterLaunchBots {
        async fn find(&self, _u: &str, _b: &str) -> Option<Bot> { Some(enabled_bot(true)) }
        async fn find_consistent(&self, _u: &str, _b: &str) -> Option<Bot> {
            let mut n = self.consistent_reads.lock().unwrap();
            *n += 1;
            Some(enabled_bot(*n < 2))
        }
        async fn save(&self, _bot: &Bot) -> Result<(), DomainError> { Ok(()) }
        async fn find_by_user_id(&self, _u: &str) -> Vec<Bot> { vec![] }
        async fn delete(&self, _u: &str, _b: &str) -> Result<(), String> { Ok(()) }
    }

    #[tokio::test]
    async fn disable_after_launch_stops_the_replacement() {
        let bots = Arc::new(DisableAfterLaunchBots::default());
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let runner = Arc::new(MockTaskRunner::new("task-xyz"));
        let locks = Arc::new(MockLock::new(StartClaim::Acquired));
        let stopper = Arc::new(MockStopper::default());
        let uc = ReconcileStoppedTaskUseCase::new(bots, runtimes, locks.clone(), runner.clone(), stopper.clone());

        let outcome = uc
            .execute(
                "user-1",
                "bot-1",
                "old-task",
                "cluster",
                "td",
                "container",
                StopInfo { exit_code: 137, stop_code: "TaskFailedToStart".to_string() },
                EVENT_AT,
                EVENT_AT,
            )
            .await
            .unwrap();

        assert_eq!(outcome, ReconcileOutcome::SkippedNotEnabled);
        assert_eq!(runner.call_count(), 1, "the replacement was launched before the disable was visible");
        assert_eq!(*stopper.stops.lock().unwrap(), vec!["task-xyz".to_string()], "and is stopped once the disable is seen");
    }
}
