//! The composition root both MCP binaries share.
//!
//! Outside `src/`, and included by each bin rather than exported from the
//! library, so library code cannot reach it: a composition root is the one place
//! allowed to name concrete infra, and putting it on the public surface would
//! offer every layer a shortcut past the dependency direction.

use anyhow::Context;
use pbtb_rust::config::configs::Configs;
use pbtb_rust::domain::{self, SystemClock};
use pbtb_rust::infra::client::{
    setup_dynamodb_with_configs, setup_ecs_with_configs, setup_s3_with_configs,
};
use pbtb_rust::infra::{
    DynamoBotRepository, S3ApiKeyRepository, S3BotConfigRepository, S3ReturnCurveRepository,
    S3TemplateRepository,
};
use pbtb_rust::interface::{api, mcp};
use pbtb_rust::usecase::*;
use std::sync::Arc;

/// Wire the use cases the tools drive. A strictly smaller set than telebot's:
/// no add-bot dialogue, no whole-config write.
#[allow(dead_code)]
pub async fn mcp_deps(configs: &Configs) -> anyhow::Result<mcp::Deps> {
    Ok(wire(configs).await?.mcp)
}

/// Wire the REST surface: the tool set plus what only the web offers — key
/// entry and signup.
#[allow(dead_code)]
pub async fn api_deps(configs: &Configs) -> anyhow::Result<api::Deps> {
    wire(configs).await
}

async fn wire(configs: &Configs) -> anyhow::Result<api::Deps> {
    let (dynamodb_client, table_name) = setup_dynamodb_with_configs(configs).await;
    let (s3_client, bucket_name) = setup_s3_with_configs(configs).await;
    let (ecs_client, cluster_arn, td_by_engine) = setup_ecs_with_configs(configs).await;
    let engines =
        EngineTaskDefinitions::parse(&td_by_engine).context("APP__ECS__TD_PASSIVBOT_BY_ENGINE")?;
    let container_name = configs.ecs.td_passivbot_container_name.clone();

    let bot_repository = Arc::new(DynamoBotRepository::new(dynamodb_client, table_name));
    let templates: Arc<dyn domain::configtemplate::ConfigTemplateRepository> = Arc::new(
        S3TemplateRepository::new(s3_client.clone(), bucket_name.clone()),
    );
    let bot_configs: Arc<dyn domain::botconfig::BotConfigRepository> = Arc::new(
        S3BotConfigRepository::new(s3_client.clone(), bucket_name.clone()),
    );
    let api_keys: Arc<dyn domain::ApiKeyRepository> =
        Arc::new(S3ApiKeyRepository::new(s3_client.clone(), bucket_name));
    // The chart bucket is in the same account and region as the config bucket,
    // so the one client serves both; only the bucket differs.
    let get_bot_returns_usecase = configs.chart.as_ref().map(|chart| {
        let curves: Arc<dyn domain::ReturnCurveRepository> =
            Arc::new(S3ReturnCurveRepository::new(
                s3_client,
                chart.bucket_name.clone(),
                chart.key_prefix.clone(),
            ));
        Arc::new(GetBotReturnsUseCase::new(curves))
    });

    let clock = Arc::new(SystemClock);
    let bots: Arc<dyn domain::BotRepository> = bot_repository.clone();
    let runtimes: Arc<dyn domain::BotRuntimeRepository> = bot_repository.clone();
    let start_locks: Arc<dyn domain::StartLockRepository> = bot_repository.clone();
    let config_switches: Arc<dyn domain::ConfigSwitchRepository> = bot_repository.clone();

    let launch_targets: Arc<dyn LaunchTargetResolver> = Arc::new(EngineRoutedResolver::new(
        bot_configs.clone(),
        engines.clone(),
    ));
    let task_runner: Arc<dyn TaskRunner> = Arc::new(RunTaskUseCase::new(ecs_client.clone()));
    let task_controller: Arc<dyn TaskController> = Arc::new(EcsTaskController::new(ecs_client));

    let identities: Arc<dyn domain::IdentityRepository> = bot_repository.clone();
    let add_bot_usecase = Arc::new(AddBotUseCase::new(
        bots.clone(),
        api_keys.clone(),
        clock.clone(),
    ));
    let get_template_usecase = Arc::new(GetTemplateUseCase::new(templates.clone()));
    let list_identities_usecase = Arc::new(ListIdentitiesUseCase::new(identities.clone()));
    let users: Arc<dyn domain::UserRepository> = bot_repository.clone();
    let tickets: Arc<dyn domain::LinkTicketRepository> = bot_repository.clone();
    let signup_usecase = Arc::new(SignupUseCase::new(identities.clone(), users, clock.clone()));
    let issue_bind_ticket_usecase =
        Arc::new(IssueTelegramBindTicketUseCase::new(tickets, clock.clone()));
    let unbind_telegram_usecase = Arc::new(UnbindTelegramUseCase::new(identities));
    let bot_username = configs.telegram.bot_username.clone();

    let start_bot_usecase = Arc::new(StartBotUseCase::new(
        bots.clone(),
        runtimes.clone(),
        start_locks,
        task_runner,
        task_controller.clone(),
        clock.clone(),
        cluster_arn.clone(),
        launch_targets,
        container_name,
    ));
    let stop_bot_usecase = Arc::new(StopBotUseCase::new(
        bots.clone(),
        runtimes.clone(),
        task_controller,
        clock.clone(),
        cluster_arn,
    ));
    let restart_bot_usecase = Arc::new(RestartBotUseCase::new(
        bots.clone(),
        runtimes.clone(),
        stop_bot_usecase.clone(),
        start_bot_usecase.clone(),
        clock.clone(),
    ));

    let mcp = mcp::Deps {
        list_bots_usecase: Arc::new(ListBotsUseCase::new(bots.clone())),
        delete_bot_usecase: Arc::new(DeleteBotUseCase::new(bots.clone(), api_keys)),
        list_templates_usecase: Arc::new(ListTemplatesUseCase::new(templates.clone())),
        apply_template_usecase: Arc::new(ApplyTemplateUseCase::new(
            templates,
            bots.clone(),
            bot_configs.clone(),
            config_switches,
            clock.clone(),
            engines.clone(),
        )),
        get_bot_config_usecase: Arc::new(GetBotConfigUseCase::new(bot_configs.clone())),
        update_risk_level_usecase: Arc::new(UpdateRiskLevelUseCase::new(
            bot_configs.clone(),
            clock.clone(),
        )),
        set_strategy_side_usecase: Arc::new(SetStrategySideUseCase::new(
            bot_configs.clone(),
            clock.clone(),
        )),
        set_bot_runtime_usecase: Arc::new(SetBotRuntimeUseCase::new(
            bots.clone(),
            bot_configs,
            engines,
            clock.clone(),
        )),
        get_bot_runtime_usecase: Arc::new(GetBotRuntimeUseCase::new(runtimes.clone())),
        start_bot_usecase,
        stop_bot_usecase,
        restart_bot_usecase,
        get_template_usecase,
        get_bot_returns_usecase,
        list_identities_usecase,
        issue_bind_ticket_usecase,
        unbind_telegram_usecase,
        bot_username,
    };

    Ok(api::Deps {
        mcp,
        add_bot_usecase,
        signup_usecase,
    })
}
