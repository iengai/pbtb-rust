// Rust
use std::sync::Arc;
use crate::usecase::{ListBotsUseCase, AddBotUseCase};

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
#[derive(Clone)]
pub struct Deps {
    pub list_bots_usecase: Arc<ListBotsUseCase>,
    pub add_bot_usecase: Arc<AddBotUseCase>,
}