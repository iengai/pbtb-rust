// Rust
use teloxide::prelude::*;

// 如需多步对话/状态机，在此定义状态并注册路由
#[derive(Clone, Default)]
pub enum State {
    #[default]
    Idle,
    // Add more states here...
}

pub fn routes() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry()
}
