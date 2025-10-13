// Rust
pub mod router;
pub mod middlewares;
pub mod keyboards;
pub mod views;
pub mod commands;
pub mod callbacks;
pub mod dialogue;
pub mod types;
pub mod states;

// Dependencies aggregation for handlers
use std::sync::Arc;
use crate::usecase::*;

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
}