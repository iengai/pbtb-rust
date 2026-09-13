use crate::domain::bot::BotRepository;
use crate::domain::clock::Clock;
use crate::domain::error::DomainError;
use crate::domain::runtime::{BotRuntimeRepository, RuntimePhase};
use crate::usecase::start_bot::{START_LOCK_STALE_AFTER_SECS, StartBotUseCase, StartOutcome};
use crate::usecase::stop_bot::{StopBotUseCase, StopOutcome};
use crate::usecase::stop_task::RESTART_REASON;
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq)]
pub enum RestartOutcome {
    /// The task was told to stop with the restart reason; the reconcile Lambda
    /// launches the replacement once ECS reports it gone.
    Restarting {
        task_id: String,
    },
    /// Nothing was running, so this was a plain start.
    Started {
        task_id: String,
    },
    /// A launch is in flight and its task id is not recorded yet, or a task came
    /// up between the read and the start; a retry restarts it.
    StartInProgress,
    /// The task is winding down from an earlier Stop or Restart; there is nothing
    /// to do until it lands.
    Stopping,
    BotNotFound,
}

/// Turns "Restart bot" into a stop that keeps desired state ON.
///
/// The relaunch is not done here. Launching before the old task is STOPPED
/// would be the double run the start lock exists to prevent, and polling ECS
/// from a request handler for the wind-down is worse. The reconcile Lambda
/// already launches a replacement once a task is really gone (the OOM path,
/// claimed through `try_acquire_restart`); the stop reason is what admits this
/// stop to that path. What the Lambda cannot check is checked here first,
/// before intent or the task is touched: the level ceiling for a bot that is
/// off, and that the bot's config resolves to a launchable target, so a restart
/// never takes a healthy task down behind a config that cannot come back.
pub struct RestartBotUseCase {
    bots: Arc<dyn BotRepository>,
    runtimes: Arc<dyn BotRuntimeRepository>,
    stop: Arc<StopBotUseCase>,
    start: Arc<StartBotUseCase>,
    clock: Arc<dyn Clock>,
}

impl RestartBotUseCase {
    pub fn new(
        bots: Arc<dyn BotRepository>,
        runtimes: Arc<dyn BotRuntimeRepository>,
        stop: Arc<StopBotUseCase>,
        start: Arc<StartBotUseCase>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            bots,
            runtimes,
            stop,
            start,
            clock,
        }
    }

    /// `vip_level` is the caller's, as their account row reads at this request.
    pub async fn execute(
        &self,
        user_id: &str,
        vip_level: u8,
        bot_id: &str,
    ) -> Result<RestartOutcome, DomainError> {
        let mut bot = match self.bots.find(user_id, bot_id).await? {
            Some(b) => b,
            None => return Ok(RestartOutcome::BotNotFound),
        };

        let now = self.clock.now();
        let runtime = self.runtimes.find_consistent(user_id, bot_id).await?;
        match runtime {
            // A fresh `stopping` is an earlier Stop or Restart still winding down;
            // intent stays as that action set it. A stale one (a dropped STOPPED
            // event) is StartBotUseCase's to reclaim behind its liveness check.
            Some(rt)
                if rt.phase == RuntimePhase::Stopping
                    && rt.observed_at > now - START_LOCK_STALE_AFTER_SECS =>
            {
                Ok(RestartOutcome::Stopping)
            }
            Some(rt)
                if matches!(rt.phase, RuntimePhase::Running | RuntimePhase::Starting)
                    && rt.task_id.is_some() =>
            {
                // A bot that is off holds no slot, even while its task is still up.
                if !bot.enabled {
                    self.start.ensure_slot(user_id, vip_level, bot_id).await?;
                }
                self.start.resolve_target(&bot).await?;

                // Intent ON before the stop, so the STOPPED event finds an enabled
                // bot and a crash after this point leaves Enabled · Stopped, which
                // Run and the Lambda both know how to read.
                bot.enable(now);
                self.bots.save(&bot).await?;

                match self.stop.wind_down(user_id, bot_id, RESTART_REASON).await? {
                    StopOutcome::Stopped { task_id } => Ok(RestartOutcome::Restarting { task_id }),
                    // ECS reported the task gone and the row now reads stopped.
                    StopOutcome::NotRunning => self.start_instead(user_id, vip_level, bot_id).await,
                    StopOutcome::StartInProgress => Ok(RestartOutcome::StartInProgress),
                    StopOutcome::AlreadyStopping => Ok(RestartOutcome::Stopping),
                    StopOutcome::BotNotFound => Ok(RestartOutcome::BotNotFound),
                }
            }
            // A launch in flight whose task id is not attached yet. A stale one (a
            // crash between the claim and the attach) falls through to the start,
            // which owns the time-based reclaim of an id-less lock.
            Some(rt)
                if rt.phase == RuntimePhase::Starting
                    && rt.observed_at > now - START_LOCK_STALE_AFTER_SECS =>
            {
                Ok(RestartOutcome::StartInProgress)
            }
            _ => self.start_instead(user_id, vip_level, bot_id).await,
        }
    }

    async fn start_instead(
        &self,
        user_id: &str,
        vip_level: u8,
        bot_id: &str,
    ) -> Result<RestartOutcome, DomainError> {
        Ok(
            match self.start.execute(user_id, vip_level, bot_id).await? {
                StartOutcome::Started { task_id } => RestartOutcome::Started { task_id },
                // A task came up between the read and the claim; a retry restarts it.
                StartOutcome::AlreadyRunning | StartOutcome::AlreadyStarting => {
                    RestartOutcome::StartInProgress
                }
                StartOutcome::Stopping => RestartOutcome::Stopping,
                StartOutcome::BotNotFound => RestartOutcome::BotNotFound,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::bot::Bot;
    use crate::domain::engine::{EngineVersion, Runtime};
    use crate::domain::exchange::Exchange;
    use crate::domain::runtime::{BotRuntime, StartClaim, StartLockRepository};
    use crate::usecase::engine_routing::{LaunchTarget, LaunchTargetResolver};
    use crate::usecase::run_task::TaskRunner;
    use crate::usecase::stop_task::{TaskController, TaskLiveness};
    use anyhow::{Result, anyhow};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;

    const NOW: i64 = 1_700_000_000;
    const TOP: u8 = crate::domain::user::MAX_VIP_LEVEL;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> i64 {
            NOW
        }
    }

    #[derive(Default)]
    struct InMemoryBots {
        bots: Mutex<HashMap<(String, String), Bot>>,
    }
    impl InMemoryBots {
        fn with(bots: impl IntoIterator<Item = Bot>) -> Self {
            let map = bots
                .into_iter()
                .map(|b| ((b.user_id.clone(), b.id.clone()), b))
                .collect();
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
        async fn find(&self, user_id: &str, bot_id: &str) -> Result<Option<Bot>, DomainError> {
            Ok(self.get(user_id, bot_id))
        }
        async fn save(&self, bot: &Bot) -> Result<(), DomainError> {
            self.bots
                .lock()
                .unwrap()
                .insert((bot.user_id.clone(), bot.id.clone()), bot.clone());
            Ok(())
        }
        async fn find_by_user_id(&self, user_id: &str) -> Result<Vec<Bot>, DomainError> {
            Ok(self
                .bots
                .lock()
                .unwrap()
                .values()
                .filter(|b| b.user_id == user_id)
                .cloned()
                .collect())
        }
        async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), DomainError> {
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
        fn get(&self) -> Option<BotRuntime> {
            self.runtimes
                .lock()
                .unwrap()
                .get(&("user-1".to_string(), "bot-1".to_string()))
                .cloned()
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

    /// A lock that always grants; the real CAS is integration-tested.
    struct GrantingLock;
    #[async_trait]
    impl StartLockRepository for GrantingLock {
        async fn try_acquire_start(
            &self,
            _u: &str,
            _b: &str,
            _now: i64,
            _stale: i64,
        ) -> Result<StartClaim, DomainError> {
            Ok(StartClaim::Acquired)
        }
        async fn try_acquire_restart(
            &self,
            _u: &str,
            _b: &str,
            _stopped: &str,
            _now: i64,
        ) -> Result<StartClaim, DomainError> {
            Ok(StartClaim::Acquired)
        }
        async fn attach_started_task(
            &self,
            _u: &str,
            _b: &str,
            _t: &str,
        ) -> Result<(), DomainError> {
            Ok(())
        }
        async fn release_start(&self, _u: &str, _b: &str, _now: i64) -> Result<(), DomainError> {
            Ok(())
        }
    }

    /// Records every RunTask and StopTask, with the stop's reason; liveness is
    /// whatever the test declared for the task.
    #[derive(Default)]
    struct RecordingEcs {
        launches: Mutex<usize>,
        stops: Mutex<Vec<(String, String)>>,
        gone: Mutex<Vec<String>>,
    }
    #[async_trait]
    impl TaskRunner for RecordingEcs {
        async fn run(&self, _u: &str, _b: &str, _c: &str, _t: &str, _n: &str) -> Result<String> {
            let mut n = self.launches.lock().unwrap();
            *n += 1;
            Ok(format!("task-{n}"))
        }
    }
    #[async_trait]
    impl TaskController for RecordingEcs {
        async fn stop(&self, _c: &str, task_id: &str, reason: &str) -> Result<()> {
            if self.gone.lock().unwrap().iter().any(|t| t == task_id) {
                return Err(anyhow!("task not found"));
            }
            self.stops
                .lock()
                .unwrap()
                .push((task_id.to_string(), reason.to_string()));
            Ok(())
        }
        async fn liveness(&self, _c: &str, task_id: &str) -> Result<TaskLiveness> {
            Ok(if self.gone.lock().unwrap().iter().any(|t| t == task_id) {
                TaskLiveness::Gone
            } else {
                TaskLiveness::Alive
            })
        }
    }

    struct Resolver {
        ok: bool,
    }
    #[async_trait]
    impl LaunchTargetResolver for Resolver {
        async fn resolve(&self, bot: &Bot) -> Result<LaunchTarget, DomainError> {
            if self.ok {
                Ok(LaunchTarget {
                    engine: EngineVersion::new(7),
                    runtime: bot.runtime,
                    td_arn: "td".to_string(),
                })
            } else {
                Err(DomainError::InvalidConfig(
                    "no image for engine v9".to_string(),
                ))
            }
        }
    }

    fn bot(id: &str, enabled: bool) -> Bot {
        Bot::new(
            id.to_string(),
            "user-1".to_string(),
            Exchange::Bybit,
            id.to_string(),
            "ak".to_string(),
            "sk".to_string(),
            enabled,
            Runtime::Py,
            1,
            1,
        )
    }

    fn running(task_id: &str) -> BotRuntime {
        BotRuntime::running(
            "user-1".to_string(),
            "bot-1".to_string(),
            task_id.to_string(),
            3,
            NOW - 100,
        )
    }

    fn stopping_at(observed_at: i64) -> BotRuntime {
        BotRuntime::stopping(
            "user-1".to_string(),
            "bot-1".to_string(),
            "task-old".to_string(),
            3,
            observed_at,
        )
    }

    struct World {
        bots: Arc<InMemoryBots>,
        runtimes: Arc<InMemoryRuntimes>,
        ecs: Arc<RecordingEcs>,
        uc: RestartBotUseCase,
    }

    fn world(bots: InMemoryBots, runtimes: InMemoryRuntimes, resolves: bool) -> World {
        let bots = Arc::new(bots);
        let runtimes = Arc::new(runtimes);
        let ecs = Arc::new(RecordingEcs::default());
        let clock = Arc::new(FixedClock);
        let start = Arc::new(StartBotUseCase::new(
            bots.clone(),
            runtimes.clone(),
            Arc::new(GrantingLock),
            ecs.clone(),
            ecs.clone(),
            clock.clone(),
            "cluster".to_string(),
            Arc::new(Resolver { ok: resolves }),
            "container".to_string(),
        ));
        let stop = Arc::new(StopBotUseCase::new(
            bots.clone(),
            runtimes.clone(),
            ecs.clone(),
            clock.clone(),
            "cluster".to_string(),
        ));
        let uc = RestartBotUseCase::new(bots.clone(), runtimes.clone(), stop, start, clock);
        World {
            bots,
            runtimes,
            ecs,
            uc,
        }
    }

    #[tokio::test]
    async fn a_running_bot_is_stopped_with_the_restart_reason_and_stays_enabled() {
        let w = world(
            InMemoryBots::with([bot("bot-1", true)]),
            InMemoryRuntimes::with(running("task-xyz")),
            true,
        );

        let out = w.uc.execute("user-1", TOP, "bot-1").await.unwrap();

        assert_eq!(
            out,
            RestartOutcome::Restarting {
                task_id: "task-xyz".to_string()
            }
        );
        assert_eq!(
            *w.ecs.stops.lock().unwrap(),
            vec![("task-xyz".to_string(), RESTART_REASON.to_string())],
            "one StopTask, carrying the reason the Lambda relaunches on"
        );
        assert_eq!(
            *w.ecs.launches.lock().unwrap(),
            0,
            "the relaunch is the Lambda's"
        );
        assert!(w.bots.get("user-1", "bot-1").unwrap().enabled);
        let rt = w.runtimes.get().unwrap();
        assert_eq!(rt.phase, RuntimePhase::Stopping);
        assert_eq!(rt.task_id.as_deref(), Some("task-xyz"));
    }

    #[tokio::test]
    async fn a_disabled_bot_with_a_live_task_is_enabled_and_restarted() {
        let w = world(
            InMemoryBots::with([bot("bot-1", false)]),
            InMemoryRuntimes::with(running("task-xyz")),
            true,
        );

        let out = w.uc.execute("user-1", TOP, "bot-1").await.unwrap();

        assert!(matches!(out, RestartOutcome::Restarting { .. }));
        assert!(
            w.bots.get("user-1", "bot-1").unwrap().enabled,
            "intent is ON before the stop, so the STOPPED event finds an enabled bot"
        );
    }

    #[tokio::test]
    async fn a_stopped_bot_is_started_instead() {
        let w = world(
            InMemoryBots::with([bot("bot-1", false)]),
            InMemoryRuntimes::default(),
            true,
        );

        let out = w.uc.execute("user-1", TOP, "bot-1").await.unwrap();

        assert_eq!(
            out,
            RestartOutcome::Started {
                task_id: "task-1".to_string()
            }
        );
        assert!(w.ecs.stops.lock().unwrap().is_empty());
        assert_eq!(*w.ecs.launches.lock().unwrap(), 1);
        assert!(w.bots.get("user-1", "bot-1").unwrap().enabled);
    }

    #[tokio::test]
    async fn a_launch_in_flight_reports_start_in_progress() {
        let w = world(
            InMemoryBots::with([bot("bot-1", true)]),
            InMemoryRuntimes::with(BotRuntime {
                user_id: "user-1".to_string(),
                bot_id: "bot-1".to_string(),
                task_id: None,
                phase: RuntimePhase::Starting,
                version: 3,
                observed_at: NOW - 5,
            }),
            true,
        );

        let out = w.uc.execute("user-1", TOP, "bot-1").await.unwrap();

        assert_eq!(out, RestartOutcome::StartInProgress);
        assert!(w.ecs.stops.lock().unwrap().is_empty());
        assert_eq!(*w.ecs.launches.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn a_stale_launch_with_no_task_id_is_started() {
        let w = world(
            InMemoryBots::with([bot("bot-1", true)]),
            InMemoryRuntimes::with(BotRuntime {
                user_id: "user-1".to_string(),
                bot_id: "bot-1".to_string(),
                task_id: None,
                phase: RuntimePhase::Starting,
                version: 3,
                observed_at: NOW - START_LOCK_STALE_AFTER_SECS - 1,
            }),
            true,
        );

        let out = w.uc.execute("user-1", TOP, "bot-1").await.unwrap();

        assert!(matches!(out, RestartOutcome::Started { .. }), "{out:?}");
        assert_eq!(*w.ecs.launches.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn a_stopping_bot_reports_stopping() {
        let w = world(
            InMemoryBots::with([bot("bot-1", false)]),
            InMemoryRuntimes::with(stopping_at(NOW - 5)),
            true,
        );

        let out = w.uc.execute("user-1", TOP, "bot-1").await.unwrap();

        assert_eq!(out, RestartOutcome::Stopping);
        assert!(
            !w.bots.get("user-1", "bot-1").unwrap().enabled,
            "a Stop's wind-down keeps the intent that Stop set"
        );
        assert!(w.ecs.stops.lock().unwrap().is_empty());
        assert_eq!(*w.ecs.launches.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn a_stale_stopping_row_whose_task_is_gone_is_started() {
        let w = world(
            InMemoryBots::with([bot("bot-1", true)]),
            InMemoryRuntimes::with(stopping_at(NOW - START_LOCK_STALE_AFTER_SECS - 1)),
            true,
        );
        w.ecs.gone.lock().unwrap().push("task-old".to_string());

        let out = w.uc.execute("user-1", TOP, "bot-1").await.unwrap();

        assert!(matches!(out, RestartOutcome::Started { .. }), "{out:?}");
        assert_eq!(*w.ecs.launches.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn a_task_ecs_no_longer_knows_is_reconciled_and_a_new_one_started() {
        let w = world(
            InMemoryBots::with([bot("bot-1", true)]),
            InMemoryRuntimes::with(running("task-xyz")),
            true,
        );
        w.ecs.gone.lock().unwrap().push("task-xyz".to_string());

        let out = w.uc.execute("user-1", TOP, "bot-1").await.unwrap();

        assert!(matches!(out, RestartOutcome::Started { .. }), "{out:?}");
        assert!(w.ecs.stops.lock().unwrap().is_empty());
        assert_eq!(*w.ecs.launches.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn an_unresolvable_config_refuses_the_restart_and_stops_nothing() {
        let w = world(
            InMemoryBots::with([bot("bot-1", true)]),
            InMemoryRuntimes::with(running("task-xyz")),
            false,
        );

        let err = w.uc.execute("user-1", TOP, "bot-1").await.unwrap_err();

        assert!(matches!(err, DomainError::InvalidConfig(_)), "{err:?}");
        assert!(
            w.ecs.stops.lock().unwrap().is_empty(),
            "a healthy task is never taken down behind a config that cannot come back"
        );
        assert_eq!(w.runtimes.get().unwrap().phase, RuntimePhase::Running);
    }

    #[tokio::test]
    async fn a_disabled_bot_with_a_live_task_is_held_to_the_level_ceiling() {
        let w = world(
            InMemoryBots::with([bot("bot-1", false), bot("bot-2", true)]),
            InMemoryRuntimes::with(running("task-xyz")),
            true,
        );

        // Level 0 allows one switched-on bot, and bot-2 holds it.
        let err = w.uc.execute("user-1", 0, "bot-1").await.unwrap_err();

        assert!(
            matches!(err, DomainError::QuotaExceeded { limit: 1 }),
            "{err:?}"
        );
        assert!(w.ecs.stops.lock().unwrap().is_empty());
        assert!(!w.bots.get("user-1", "bot-1").unwrap().enabled);
    }

    #[tokio::test]
    async fn an_enabled_bot_with_a_live_task_is_not_counted_against_itself() {
        let w = world(
            InMemoryBots::with([bot("bot-1", true)]),
            InMemoryRuntimes::with(running("task-xyz")),
            true,
        );

        let out = w.uc.execute("user-1", 0, "bot-1").await.unwrap();

        assert!(matches!(out, RestartOutcome::Restarting { .. }), "{out:?}");
    }

    #[tokio::test]
    async fn missing_bot_returns_not_found() {
        let w = world(InMemoryBots::default(), InMemoryRuntimes::default(), true);

        let out = w.uc.execute("user-1", TOP, "bot-1").await.unwrap();

        assert_eq!(out, RestartOutcome::BotNotFound);
    }
}
