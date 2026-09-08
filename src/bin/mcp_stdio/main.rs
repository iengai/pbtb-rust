//! The MCP server over stdio: bot management as tools, for a client running on
//! the operator's own machine.
//!
//! stdio is the transport where the process itself is the credential — the
//! client spawns this binary as a child of a shell the operator already
//! controls — so the principal is fixed at startup rather than presented per
//! request. The HTTP transport, where that reasoning does not hold, takes its
//! own `Authenticator`; nothing in the tools changes between the two.
//!
//! Nothing is logged to stdout: stdout IS the protocol stream. The subscriber
//! writes to stderr.

use anyhow::{Context, bail};
use pbtb_rust::config::configs::{Configs, load_config};
use pbtb_rust::domain::{self, SystemClock};
use pbtb_rust::infra::client::{
    setup_dynamodb_with_configs, setup_ecs_with_configs, setup_s3_with_configs,
};
use pbtb_rust::infra::{
    DynamoBotRepository, S3ApiKeyRepository, S3BotConfigRepository, S3TemplateRepository,
};
use pbtb_rust::interface::mcp::{self, Authenticator, BotTools, LocalOperator};
use pbtb_rust::usecase::*;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let configs: Configs = load_config().context("Failed to load config")?;

    let user_id = configs.mcp.user_id.trim().to_string();
    if user_id.is_empty() {
        bail!("APP__MCP__USER_ID: set it to the Telegram user id this server acts as");
    }
    // The same allowlist the bot enforces, checked here too: an operator removed
    // from telebot loses MCP access with them, rather than keeping a second door.
    let allowed = configs
        .telegram
        .allowlist()
        .context("APP__TELEGRAM__ALLOWED_USER_IDS")?;
    if !allowed.contains(&user_id) {
        bail!("APP__MCP__USER_ID={user_id} is not on APP__TELEGRAM__ALLOWED_USER_IDS");
    }

    let deps = build_deps(&configs).await?;
    let auth: Arc<dyn Authenticator> = Arc::new(LocalOperator::new(user_id));

    let service = BotTools::new(deps, auth)
        .serve(stdio())
        .await
        .context("Failed to start the MCP stdio server")?;
    service.waiting().await.context("MCP server stopped")?;
    Ok(())
}

/// Wire the use cases the tools drive. A separate composition root from
/// `main.rs`: the two adapters expose different surfaces, and the tools reach
/// for a strictly smaller set (no add-bot dialogue, no whole-config write).
async fn build_deps(configs: &Configs) -> anyhow::Result<mcp::Deps> {
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
        Arc::new(S3ApiKeyRepository::new(s3_client, bucket_name));

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

    Ok(mcp::Deps {
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
        start_bot_usecase: Arc::new(StartBotUseCase::new(
            bots.clone(),
            runtimes.clone(),
            start_locks,
            task_runner,
            task_controller.clone(),
            clock.clone(),
            cluster_arn.clone(),
            launch_targets,
            container_name,
        )),
        stop_bot_usecase: Arc::new(StopBotUseCase::new(
            bots,
            runtimes,
            task_controller,
            clock,
            cluster_arn,
        )),
    })
}
