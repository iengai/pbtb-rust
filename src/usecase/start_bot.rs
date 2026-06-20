use std::sync::Arc;
use crate::domain::bot::BotRepository;
use crate::domain::clock::Clock;
use crate::domain::runtime::{BotRuntimeRepository, RuntimePhase, StartClaim, StartLockRepository};
use crate::usecase::run_task::TaskRunner;
use crate::usecase::stop_task::{TaskController, TaskLiveness};

/// Seconds after which a `starting` lock is treated as abandoned and may be
/// re-claimed. Deliberately generous — longer than any real task-start latency —
/// so a legitimately in-flight launch is never stolen, which would double-run a
/// live-trading task. A stale lock that still carries a task id is only reclaimed
/// after ECS confirms the task is gone (see the liveness guard in `execute`); the
/// residual time-based reclaim applies only when no task id was ever recorded.
pub const START_LOCK_STALE_AFTER_SECS: i64 = 600;

#[derive(Debug, PartialEq, Eq)]
pub enum StartOutcome {
    Started { task_id: String },
    AlreadyRunning,
    AlreadyStarting,
    BotNotFound,
}

/// Turns the user's "Run bot" intent into a single ECS task launch.
///
/// Order is deliberate and money-critical:
/// 1. flip desired state ON (so the reconcile Lambda will keep it up),
/// 2. claim the exclusive start lock (CAS) — only the winner launches,
/// 3. launch, then record the task id so a stop during startup can find it.
pub struct StartBotUseCase {
    bots: Arc<dyn BotRepository>,
    runtimes: Arc<dyn BotRuntimeRepository>,
    locks: Arc<dyn StartLockRepository>,
    runner: Arc<dyn TaskRunner>,
    controller: Arc<dyn TaskController>,
    clock: Arc<dyn Clock>,
    cluster_arn: String,
    td_arn: String,
    container_name: String,
}

impl StartBotUseCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bots: Arc<dyn BotRepository>,
        runtimes: Arc<dyn BotRuntimeRepository>,
        locks: Arc<dyn StartLockRepository>,
        runner: Arc<dyn TaskRunner>,
        controller: Arc<dyn TaskController>,
        clock: Arc<dyn Clock>,
        cluster_arn: String,
        td_arn: String,
        container_name: String,
    ) -> Self {
        Self { bots, runtimes, locks, runner, controller, clock, cluster_arn, td_arn, container_name }
    }

    pub async fn execute(&self, user_id: &str, bot_id: &str) -> Result<StartOutcome, String> {
        let mut bot = match self.bots.find(user_id, bot_id).await {
            Some(b) => b,
            None => return Ok(StartOutcome::BotNotFound),
        };

        // Desired state ON first: intent is recorded even if the launch fails,
        // and auto-restart keys off it.
        let now = self.clock.now();
        bot.enable(now);
        self.bots.save(&bot).await.map_err(|e| e.to_string())?;

        // Guard the lock's time-based stale reclaim: a `starting` lock older than
        // the stale window whose task is actually alive (its RUNNING event was
        // lost) must NOT be reclaimed — relaunching would double-run. Confirm with
        // ECS that the carried task is gone before allowing the reclaim below.
        let runtime = self.runtimes.find_consistent(user_id, bot_id).await.map_err(|e| e.to_string())?;
        if let Some(rt) = &runtime {
            let stale = rt.phase == RuntimePhase::Starting && rt.observed_at <= now - START_LOCK_STALE_AFTER_SECS;
            if stale {
                if let Some(task_id) = rt.task_id.as_deref() {
                    match self.controller.liveness(&self.cluster_arn, task_id).await {
                        Ok(TaskLiveness::Alive) => return Ok(StartOutcome::AlreadyRunning),
                        Ok(TaskLiveness::Gone) => {}
                        Err(e) => return Err(format!("could not verify the in-flight task is stopped: {e}")),
                    }
                }
                // No task id was recorded (a crash before it could be attached):
                // there is no id to verify, so the time-based reclaim below is the
                // accepted residual edge.
            }
        }

        // Claim the exclusive right to launch exactly one task.
        match self.locks.try_acquire_start(user_id, bot_id, now, START_LOCK_STALE_AFTER_SECS).await.map_err(|e| e.to_string())? {
            StartClaim::AlreadyRunning => return Ok(StartOutcome::AlreadyRunning),
            StartClaim::AlreadyStarting => return Ok(StartOutcome::AlreadyStarting),
            StartClaim::Acquired => {}
        }

        // Lock held: launch, then attach the task id. On failure release the lock
        // so the user can retry (desired stays ON, observed returns to stopped).
        match self.runner.run(user_id, bot_id, &self.cluster_arn, &self.td_arn, &self.container_name).await {
            Ok(task_id) => {
                self.locks.attach_started_task(user_id, bot_id, &task_id).await.map_err(|e| e.to_string())?;
                Ok(StartOutcome::Started { task_id })
            }
            Err(e) => {
                let _ = self.locks.release_start(user_id, bot_id, self.clock.now()).await;
                Err(e.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use anyhow::{anyhow, Result};
    use async_trait::async_trait;
    use crate::domain::bot::Bot;
    use crate::domain::error::DomainError;
    use crate::domain::exchange::Exchange;
    use crate::domain::runtime::BotRuntime;

    const NOW: i64 = 1_700_000_000;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> i64 { NOW }
    }

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
        fn get(&self, user_id: &str, bot_id: &str) -> Option<Bot> {
            self.bots.lock().unwrap().get(&(user_id.to_string(), bot_id.to_string())).cloned()
        }
    }
    #[async_trait]
    impl BotRepository for InMemoryBots {
        async fn find(&self, user_id: &str, bot_id: &str) -> Option<Bot> {
            self.get(user_id, bot_id)
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

    #[derive(Default)]
    struct InMemoryRuntimes {
        runtimes: Mutex<HashMap<(String, String), BotRuntime>>,
    }
    impl InMemoryRuntimes {
        fn with(rt: BotRuntime) -> Self {
            let mut map = HashMap::new();
            map.insert((rt.user_id.clone(), rt.bot_id.clone()), rt);
            Self { runtimes: Mutex::new(map) }
        }
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

    /// Mock lock that returns a preset start claim and records which lifecycle
    /// calls the use case made. The real CAS is exercised by the integration test.
    struct MockLock {
        claim: StartClaim,
        acquire_calls: Mutex<usize>,
        attached: Mutex<Option<String>>,
        released: Mutex<usize>,
    }
    impl MockLock {
        fn new(claim: StartClaim) -> Self {
            Self { claim, acquire_calls: Mutex::new(0), attached: Mutex::new(None), released: Mutex::new(0) }
        }
    }
    #[async_trait]
    impl StartLockRepository for MockLock {
        async fn try_acquire_start(&self, _u: &str, _b: &str, _now: i64, _stale: i64) -> Result<StartClaim, DomainError> {
            *self.acquire_calls.lock().unwrap() += 1;
            Ok(self.claim.clone())
        }
        async fn try_acquire_restart(&self, _u: &str, _b: &str, _stopped: &str, _now: i64) -> Result<StartClaim, DomainError> {
            Ok(StartClaim::Acquired)
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

    /// Mock TaskRunner that either returns a fixed id or fails, counting calls.
    struct MockRunner {
        result: std::result::Result<String, String>,
        calls: Mutex<usize>,
    }
    impl MockRunner {
        fn ok(task_id: &str) -> Self { Self { result: Ok(task_id.to_string()), calls: Mutex::new(0) } }
        fn fail(msg: &str) -> Self { Self { result: Err(msg.to_string()), calls: Mutex::new(0) } }
        fn call_count(&self) -> usize { *self.calls.lock().unwrap() }
    }
    #[async_trait]
    impl TaskRunner for MockRunner {
        async fn run(&self, _u: &str, _b: &str, _c: &str, _t: &str, _n: &str) -> Result<String> {
            *self.calls.lock().unwrap() += 1;
            self.result.clone().map_err(|e| anyhow!(e))
        }
    }

    /// Mock TaskController whose liveness answer is preset; records call count.
    struct MockController {
        liveness: TaskLiveness,
        liveness_calls: Mutex<usize>,
    }
    impl MockController {
        fn new(liveness: TaskLiveness) -> Self {
            Self { liveness, liveness_calls: Mutex::new(0) }
        }
    }
    #[async_trait]
    impl TaskController for MockController {
        async fn stop(&self, _c: &str, _t: &str, _r: &str) -> Result<()> { Ok(()) }
        async fn liveness(&self, _c: &str, _t: &str) -> Result<TaskLiveness> {
            *self.liveness_calls.lock().unwrap() += 1;
            Ok(self.liveness)
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

    fn stale_starting(task_id: Option<String>) -> BotRuntime {
        BotRuntime {
            user_id: "user-1".to_string(),
            bot_id: "bot-1".to_string(),
            task_id,
            phase: RuntimePhase::Starting,
            version: 1,
            observed_at: NOW - START_LOCK_STALE_AFTER_SECS - 1,
        }
    }

    fn use_case(
        bots: Arc<InMemoryBots>,
        runtimes: Arc<InMemoryRuntimes>,
        locks: Arc<MockLock>,
        runner: Arc<MockRunner>,
        controller: Arc<MockController>,
    ) -> StartBotUseCase {
        StartBotUseCase::new(
            bots,
            runtimes,
            locks,
            runner,
            controller,
            Arc::new(FixedClock),
            "cluster".to_string(),
            "td".to_string(),
            "container".to_string(),
        )
    }

    #[tokio::test]
    async fn acquired_launches_enables_and_attaches() {
        let bots = Arc::new(InMemoryBots::with(bot(false)));
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let locks = Arc::new(MockLock::new(StartClaim::Acquired));
        let runner = Arc::new(MockRunner::ok("task-xyz"));
        let controller = Arc::new(MockController::new(TaskLiveness::Gone));
        let uc = use_case(bots.clone(), runtimes, locks.clone(), runner.clone(), controller);

        let out = uc.execute("user-1", "bot-1").await.unwrap();
        assert_eq!(out, StartOutcome::Started { task_id: "task-xyz".to_string() });
        assert!(bots.get("user-1", "bot-1").unwrap().enabled, "desired state flipped on");
        assert_eq!(runner.call_count(), 1, "exactly one launch");
        assert_eq!(*locks.attached.lock().unwrap(), Some("task-xyz".to_string()), "task id attached to lock");
        assert_eq!(*locks.released.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn already_running_does_not_launch_but_still_enables() {
        let bots = Arc::new(InMemoryBots::with(bot(false)));
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let locks = Arc::new(MockLock::new(StartClaim::AlreadyRunning));
        let runner = Arc::new(MockRunner::ok("task-xyz"));
        let controller = Arc::new(MockController::new(TaskLiveness::Gone));
        let uc = use_case(bots.clone(), runtimes, locks, runner.clone(), controller);

        let out = uc.execute("user-1", "bot-1").await.unwrap();
        assert_eq!(out, StartOutcome::AlreadyRunning);
        assert!(bots.get("user-1", "bot-1").unwrap().enabled);
        assert_eq!(runner.call_count(), 0, "must not launch when already running");
    }

    #[tokio::test]
    async fn already_starting_does_not_launch() {
        let bots = Arc::new(InMemoryBots::with(bot(false)));
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let locks = Arc::new(MockLock::new(StartClaim::AlreadyStarting));
        let runner = Arc::new(MockRunner::ok("task-xyz"));
        let controller = Arc::new(MockController::new(TaskLiveness::Gone));
        let uc = use_case(bots, runtimes, locks, runner.clone(), controller);

        let out = uc.execute("user-1", "bot-1").await.unwrap();
        assert_eq!(out, StartOutcome::AlreadyStarting);
        assert_eq!(runner.call_count(), 0, "must not launch a second task");
    }

    #[tokio::test]
    async fn launch_failure_releases_lock_and_errors() {
        let bots = Arc::new(InMemoryBots::with(bot(false)));
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let locks = Arc::new(MockLock::new(StartClaim::Acquired));
        let runner = Arc::new(MockRunner::fail("ecs run_task failed"));
        let controller = Arc::new(MockController::new(TaskLiveness::Gone));
        let uc = use_case(bots.clone(), runtimes, locks.clone(), runner.clone(), controller);

        let err = uc.execute("user-1", "bot-1").await.unwrap_err();
        assert!(err.contains("run_task"));
        assert_eq!(runner.call_count(), 1);
        assert_eq!(*locks.released.lock().unwrap(), 1, "lock released after failed launch");
        assert!(bots.get("user-1", "bot-1").unwrap().enabled, "desired stays on for retry");
    }

    #[tokio::test]
    async fn missing_bot_returns_not_found_without_claiming() {
        let bots = Arc::new(InMemoryBots::default());
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let locks = Arc::new(MockLock::new(StartClaim::Acquired));
        let runner = Arc::new(MockRunner::ok("task-xyz"));
        let controller = Arc::new(MockController::new(TaskLiveness::Gone));
        let uc = use_case(bots, runtimes, locks.clone(), runner.clone(), controller);

        let out = uc.execute("user-1", "ghost").await.unwrap();
        assert_eq!(out, StartOutcome::BotNotFound);
        assert_eq!(*locks.acquire_calls.lock().unwrap(), 0, "no lock attempt for a missing bot");
        assert_eq!(runner.call_count(), 0);
    }

    #[tokio::test]
    async fn stale_lock_with_live_task_does_not_relaunch() {
        let bots = Arc::new(InMemoryBots::with(bot(true)));
        let runtimes = Arc::new(InMemoryRuntimes::with(stale_starting(Some("old-task".to_string()))));
        let locks = Arc::new(MockLock::new(StartClaim::Acquired));
        let runner = Arc::new(MockRunner::ok("task-new"));
        let controller = Arc::new(MockController::new(TaskLiveness::Alive));
        let uc = use_case(bots, runtimes, locks.clone(), runner.clone(), controller.clone());

        let out = uc.execute("user-1", "bot-1").await.unwrap();
        assert_eq!(out, StartOutcome::AlreadyRunning, "a live task must not be double-launched");
        assert_eq!(*controller.liveness_calls.lock().unwrap(), 1, "liveness checked before reclaim");
        assert_eq!(runner.call_count(), 0, "no relaunch while the task is alive");
        assert_eq!(*locks.acquire_calls.lock().unwrap(), 0, "lock not claimed when task is alive");
    }

    #[tokio::test]
    async fn stale_lock_with_dead_task_reclaims_and_launches() {
        let bots = Arc::new(InMemoryBots::with(bot(true)));
        let runtimes = Arc::new(InMemoryRuntimes::with(stale_starting(Some("old-task".to_string()))));
        let locks = Arc::new(MockLock::new(StartClaim::Acquired));
        let runner = Arc::new(MockRunner::ok("task-new"));
        let controller = Arc::new(MockController::new(TaskLiveness::Gone));
        let uc = use_case(bots, runtimes, locks.clone(), runner.clone(), controller.clone());

        let out = uc.execute("user-1", "bot-1").await.unwrap();
        assert_eq!(out, StartOutcome::Started { task_id: "task-new".to_string() });
        assert_eq!(*controller.liveness_calls.lock().unwrap(), 1, "liveness checked");
        assert_eq!(runner.call_count(), 1, "relaunch once the task is confirmed gone");
    }
}
