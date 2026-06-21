// Rust
pub mod callbacks;
pub mod commands;
pub mod dialogue;
pub mod keyboards;
pub mod middlewares;
pub mod router;
pub mod states;
pub mod types;
pub mod views;

// Dependencies aggregation for handlers
use crate::usecase::*;
use std::sync::Arc;

#[derive(Clone)]
pub struct Deps {
    // Bot management
    pub list_bots_usecase: Arc<ListBotsUseCase>,
    pub add_bot_usecase: Arc<AddBotUseCase>,
    pub delete_bot_usecase: Arc<DeleteBotUseCase>,

    // Template management
    pub list_templates_usecase: Arc<ListTemplatesUseCase>,

    // Bot config management
    pub apply_template_usecase: Arc<ApplyTemplateUseCase>,
    pub get_bot_config_usecase: Arc<GetBotConfigUseCase>,
    pub update_bot_config_usecase: Arc<UpdateBotConfigUseCase>,
    pub update_risk_level_usecase: Arc<UpdateRiskLevelUseCase>,

    // Runtime / desired-state management
    pub get_bot_runtime_usecase: Arc<GetBotRuntimeUseCase>,

    // ECS actuation (desired state -> real RunTask/StopTask)
    pub start_bot_usecase: Arc<StartBotUseCase>,
    pub stop_bot_usecase: Arc<StopBotUseCase>,
}
