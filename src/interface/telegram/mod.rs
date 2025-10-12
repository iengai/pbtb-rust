// Rust
use std::sync::Arc;
use crate::usecase::ListBotsUseCase;

pub mod router;
pub mod middlewares;
pub mod keyboards;
pub mod views;
pub mod commands;
pub mod callbacks;
pub mod dialogue;
pub mod types;

// 你的依赖聚合（示例：把 domain/service/仓储注入到 handlers）
// 实际项目里，用 Arc<T> 包装具体服务，或使用构造函数传入
#[derive(Clone)]
pub struct Deps {
    // pub bot_repo: Arc<dyn BotRepository>,
    // pub user_service: Arc<UserService>,
    pub list_bots_usecase: Arc<ListBotsUseCase>,
}