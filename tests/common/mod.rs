//! End-to-end harness: a synthetic Telegram update goes through the real
//! router, the real handlers and the real use cases, onto DynamoDB Local.
//!
//! What is real: the handler tree (`router::schema`, so branch order is under
//! test), every use case, and every DynamoDB write — including the exclusive
//! start lock's conditional expressions, which an in-memory mock cannot check.
//! What is faked: object storage, ECS, and the Bot API.
//!
//! `#![allow(dead_code)]` because each test binary compiles the whole module
//! and uses part of it.
#![allow(dead_code)]

pub mod dynamo;
pub mod fakes;
pub mod telegram;

use std::collections::HashSet;
use std::ops::ControlFlow;
use std::sync::Arc;

use pbtb_rust::domain;
use pbtb_rust::infra::DynamoBotRepository;
use pbtb_rust::interface::telegram::{Deps, router};
use pbtb_rust::usecase::*;
use serde_json::Value;
use teloxide::prelude::*;
use teloxide::types::Me;

use fakes::{FixedClock, InMemoryApiKeys, InMemoryBotConfigs, InMemoryTemplates, RecordingEcs};
use telegram::FakeTelegram;

pub const CLUSTER_ARN: &str = "arn:aws:ecs:us-east-1:000000000000:cluster/test";
pub const CONTAINER_NAME: &str = "passivbot-container";
pub const TD_V7: &str = "arn:aws:ecs:us-east-1:000000000000:task-definition/pb-v7:1";
pub const TD_V8: &str = "arn:aws:ecs:us-east-1:000000000000:task-definition/pb-v8:1";
/// The instant `FixedClock` reports, so timestamp assertions are exact.
pub const NOW: i64 = 1_700_000_000;

pub struct Harness {
    /// Holds the fixture's table (and its container, when it started one).
    _db: dynamo::Dynamo,
    pub telegram: FakeTelegram,
    pub bots: Arc<DynamoBotRepository>,
    pub configs: Arc<InMemoryBotConfigs>,
    pub api_keys: Arc<InMemoryApiKeys>,
    pub ecs: Arc<RecordingEcs>,
    schema: teloxide::dispatching::UpdateHandler<DependencyMap>,
    deps_map: DependencyMap,
    bot: Bot,
    me: Me,
}

impl Harness {
    /// Build the whole stack. `None` when no DynamoDB Local is reachable,
    /// matching the skip behaviour of the repository suite.
    pub async fn start() -> Option<Self> {
        Self::start_with_allowlist(HashSet::from([telegram::USER_ID.to_string()])).await
    }

    pub async fn start_with_allowlist(allowed: HashSet<String>) -> Option<Self> {
        let db = dynamo::start().await?;

        let bots = Arc::new(DynamoBotRepository::new(
            db.client.clone(),
            db.table.clone(),
        ));
        let configs = Arc::new(InMemoryBotConfigs::default());
        let api_keys = Arc::new(InMemoryApiKeys::default());
        let ecs = Arc::new(RecordingEcs::default());
        let templates = Arc::new(InMemoryTemplates::default());
        let clock = Arc::new(FixedClock(NOW));

        let deps = build_deps(
            bots.clone(),
            configs.clone(),
            api_keys.clone(),
            ecs.clone(),
            templates,
            clock,
        );

        let telegram = FakeTelegram::start().await;
        let bot = telegram.bot();

        Some(Self {
            _db: db,
            telegram,
            bots,
            configs,
            api_keys,
            ecs,
            schema: router::schema(allowed),
            deps_map: router::deps_map(deps),
            bot,
            me: me_stub(),
        })
    }

    /// Push one update through the router. `true` when a branch claimed it.
    ///
    /// The dispatcher's own loop is skipped deliberately: it adds an update
    /// stream and a shutdown handshake without adding coverage of this crate's
    /// code, and dispatching the schema directly makes "was it handled" an
    /// answer rather than a timeout.
    pub async fn send(&self, update: Value) -> bool {
        // Via a string, not `from_value`: teloxide's `UpdateKind` visitor asks
        // for a borrowed `&str` key, which an owned `serde_json::Value` cannot
        // provide, and its fallback leaves the map consumed. The update then
        // deserializes to `UpdateKind::Error` with no sender and no chat — no
        // error, just an update that matches nothing.
        let update: Update = serde_json::from_str(&update.to_string()).expect("valid update json");
        let mut deps = self.deps_map.clone();
        deps.insert(update);
        deps.insert(self.bot.clone());
        deps.insert(self.me.clone());

        match self.schema.dispatch(deps).await {
            ControlFlow::Break(result) => {
                // A handler that fails maps its error to an empty map, so the
                // update is still "handled"; the reply text is what a test
                // asserts on.
                let _ = result;
                true
            }
            ControlFlow::Continue(_) => false,
        }
    }

    /// Everything the chat was told so far.
    pub async fn transcript(&self) -> String {
        self.telegram.transcript().await
    }

    /// Seed a bot straight into the repository, skipping the add dialogue.
    pub async fn given_bot(&self, bot: domain::bot::Bot) {
        use domain::bot::BotRepository;
        self.bots.save(&bot).await.expect("seed bot");
    }
}

fn build_deps(
    bots: Arc<DynamoBotRepository>,
    configs: Arc<InMemoryBotConfigs>,
    api_keys: Arc<InMemoryApiKeys>,
    ecs: Arc<RecordingEcs>,
    templates: Arc<InMemoryTemplates>,
    clock: Arc<FixedClock>,
) -> Deps {
    let engines = EngineTaskDefinitions::parse(&format!("7={TD_V7},8={TD_V8}"))
        .expect("engine task definition table");

    let bots_dyn: Arc<dyn domain::BotRepository> = bots.clone();
    let runtimes_dyn: Arc<dyn domain::BotRuntimeRepository> = bots.clone();
    let locks: Arc<dyn domain::StartLockRepository> = bots.clone();
    let switches: Arc<dyn domain::ConfigSwitchRepository> = bots.clone();
    let configs_dyn: Arc<dyn domain::botconfig::BotConfigRepository> = configs.clone();

    let targets: Arc<dyn LaunchTargetResolver> = Arc::new(EngineRoutedResolver::new(
        configs_dyn.clone(),
        engines.clone(),
    ));

    Deps {
        list_bots_usecase: Arc::new(ListBotsUseCase::new(bots_dyn.clone())),
        add_bot_usecase: Arc::new(AddBotUseCase::new(
            bots_dyn.clone(),
            api_keys.clone(),
            clock.clone(),
        )),
        delete_bot_usecase: Arc::new(DeleteBotUseCase::new(bots_dyn.clone(), api_keys)),
        list_templates_usecase: Arc::new(ListTemplatesUseCase::new(templates.clone())),
        apply_template_usecase: Arc::new(ApplyTemplateUseCase::new(
            templates,
            bots_dyn.clone(),
            configs_dyn.clone(),
            switches,
            clock.clone(),
            engines.clone(),
        )),
        get_bot_config_usecase: Arc::new(GetBotConfigUseCase::new(configs_dyn.clone())),
        update_bot_config_usecase: Arc::new(UpdateBotConfigUseCase::new(
            configs_dyn.clone(),
            clock.clone(),
        )),
        update_risk_level_usecase: Arc::new(UpdateRiskLevelUseCase::new(
            configs_dyn.clone(),
            clock.clone(),
        )),
        set_strategy_side_usecase: Arc::new(SetStrategySideUseCase::new(
            configs_dyn.clone(),
            clock.clone(),
        )),
        set_bot_runtime_usecase: Arc::new(SetBotRuntimeUseCase::new(
            bots_dyn.clone(),
            configs_dyn,
            engines,
            clock.clone(),
        )),
        get_bot_runtime_usecase: Arc::new(GetBotRuntimeUseCase::new(runtimes_dyn.clone())),
        start_bot_usecase: Arc::new(StartBotUseCase::new(
            bots_dyn.clone(),
            runtimes_dyn.clone(),
            locks,
            ecs.clone(),
            ecs.clone(),
            clock.clone(),
            CLUSTER_ARN.to_string(),
            targets,
            CONTAINER_NAME.to_string(),
        )),
        stop_bot_usecase: Arc::new(StopBotUseCase::new(
            bots_dyn,
            runtimes_dyn,
            ecs,
            clock,
            CLUSTER_ARN.to_string(),
        )),
    }
}

/// The bot's own identity, which `filter_command` needs to strip an
/// `@username` suffix. The real dispatcher fetches this with `getMe`.
fn me_stub() -> Me {
    serde_json::from_value(serde_json::json!({
        "id": 7,
        "is_bot": true,
        "first_name": "pbtb",
        "username": "pbtb_test_bot",
        "can_join_groups": false,
        "can_read_all_group_messages": false,
        "supports_inline_queries": false,
    }))
    .expect("valid Me json")
}
