use std::sync::Arc;
use anyhow::Result;
use crate::domain::bot::BotRepository;
use crate::domain::runtime::{BotRuntime, BotRuntimeRepository};
use crate::domain::clock::Clock;
use crate::usecase::run_task::RunTaskUseCase;

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
    BotNotFound,
}

pub struct ReconcileStoppedTaskUseCase {
    bots: Arc<dyn BotRepository>,
    runtimes: Arc<dyn BotRuntimeRepository>,
    run_task: Arc<RunTaskUseCase>,
    clock: Arc<dyn Clock>,
}

impl ReconcileStoppedTaskUseCase {
    pub fn new(bots: Arc<dyn BotRepository>, runtimes: Arc<dyn BotRuntimeRepository>, run_task: Arc<RunTaskUseCase>, clock: Arc<dyn Clock>) -> Self {
        Self { bots, runtimes, run_task, clock }
    }

    pub async fn execute(&self, user_id: &str, bot_id: &str, cluster_arn: &str, td_arn: &str, container_name: &str, stop: StopInfo) -> Result<ReconcileOutcome> {
        let now = self.clock.now();
        let bot = match self.bots.find(user_id, bot_id).await {
            Some(b) => b,
            None => return Ok(ReconcileOutcome::BotNotFound),
        };
        let prev_version = self.runtimes.find(user_id, bot_id).await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .map(|r| r.version).unwrap_or(0);

        // Desired state OFF (user manually stopped) -> reflect stopped, never restart. THIS is the rule the old Lambda was missing.
        if !bot.enabled {
            self.runtimes.record(&BotRuntime::stopped(user_id.to_string(), bot_id.to_string(), prev_version, now)).await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            return Ok(ReconcileOutcome::SkippedNotEnabled);
        }
        if !stop.is_memory_related() {
            self.runtimes.record(&BotRuntime::stopped(user_id.to_string(), bot_id.to_string(), prev_version, now)).await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            return Ok(ReconcileOutcome::SkippedNotMemoryRelated);
        }
        let task_id = self.run_task.execute(user_id, bot_id, cluster_arn, td_arn, container_name).await?;
        self.runtimes.record(&BotRuntime::running(user_id.to_string(), bot_id.to_string(), task_id.clone(), prev_version + 1, now)).await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(ReconcileOutcome::Restarted { task_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::runtime::RuntimePhase;

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

    /// Fixed-time clock for deterministic tests (MockClock is not exported).
    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> i64 { 1_700_000_000 }
    }

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
        let uc = ReconcileStoppedTaskUseCase::new(
            bots.clone(),
            runtimes.clone(),
            run_task,
            Arc::new(FixedClock),
        );

        let outcome = uc
            .execute(
                "user-1",
                "bot-1",
                "cluster",
                "td",
                "container",
                StopInfo { exit_code: 137, stop_code: "TaskFailedToStart".to_string() },
            )
            .await
            .unwrap();

        assert_eq!(outcome, ReconcileOutcome::SkippedNotEnabled);
        // A stopped runtime was recorded (run_task never reached, so phase is Stopped).
        let rt = runtimes.find("user-1", "bot-1").await.unwrap().unwrap();
        assert_eq!(rt.phase, RuntimePhase::Stopped);
        assert_eq!(rt.task_id, None);
        assert_eq!(rt.observed_at, 1_700_000_000);
    }

    #[tokio::test]
    async fn enabled_bot_non_memory_stop_skips_without_restart() {
        let bots = Arc::new(InMemoryBots::with(enabled_bot(true)));
        let runtimes = Arc::new(InMemoryRuntimes::default());
        let run_task = Arc::new(RunTaskUseCase::new(dummy_ecs_client()));
        let uc = ReconcileStoppedTaskUseCase::new(
            bots,
            runtimes.clone(),
            run_task,
            Arc::new(FixedClock),
        );

        // exit 0 => not memory related.
        let outcome = uc
            .execute(
                "user-1",
                "bot-1",
                "cluster",
                "td",
                "container",
                StopInfo { exit_code: 0, stop_code: "EssentialContainerExited".to_string() },
            )
            .await
            .unwrap();
        assert_eq!(outcome, ReconcileOutcome::SkippedNotMemoryRelated);

        // 137 but UserInitiated => also not memory related.
        let outcome2 = uc
            .execute(
                "user-1",
                "bot-1",
                "cluster",
                "td",
                "container",
                StopInfo { exit_code: 137, stop_code: "UserInitiated".to_string() },
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
        let uc = ReconcileStoppedTaskUseCase::new(bots, runtimes, run_task, Arc::new(FixedClock));

        let outcome = uc
            .execute(
                "user-1",
                "ghost",
                "cluster",
                "td",
                "container",
                StopInfo { exit_code: 137, stop_code: "TaskFailedToStart".to_string() },
            )
            .await
            .unwrap();
        assert_eq!(outcome, ReconcileOutcome::BotNotFound);
    }

    // The Restarted path requires a real ECS run_task call (RunTaskUseCase holds a
    // concrete aws_sdk_ecs::Client, not a trait), so it cannot be cleanly faked
    // without a live/mock ECS endpoint. It is covered by the integration layer,
    // not here. Intentionally skipped.
}
