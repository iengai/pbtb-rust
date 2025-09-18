// Rust
use teloxide::prelude::*;

// 可在此添加鉴权、节流、统一错误拦截、日志上下文等横切逻辑。
// 返回一个可链式组合的 Handler。当前先返回一个空的入口节点。
pub fn install() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry()
}
