// Rust
use teloxide::{dispatching::Dispatcher, prelude::*};
use crate::interface::telegram::{commands, callbacks, dialogue, middlewares, Deps};

pub async fn run(bot: Bot, deps: Deps) -> anyhow::Result<()> {
    // 将依赖注入到 DependencyMap，便于在 handlers 中提取
    let deps_map = dptree::deps![deps];

    // 显式标注 schema 的类型为 UpdateHandler<DependencyMap>
    let schema: teloxide::dispatching::UpdateHandler<DependencyMap> = dptree::entry()
        .chain(middlewares::install())
        .branch(dialogue::routes())
        .branch(commands::routes())
        .branch(callbacks::routes());

    let mut dispatcher = Dispatcher::builder(bot, schema)
        .dependencies(deps_map)
        .enable_ctrlc_handler()
        .build();

    dispatcher.dispatch().await;
    Ok(())
}