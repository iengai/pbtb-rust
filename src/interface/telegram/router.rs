// Rust
use crate::interface::telegram::{
    Deps, callbacks, commands, dialogue, middlewares,
    states::{BotContext, DialogueState},
};
use std::collections::HashSet;
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::{dispatching::Dispatcher, prelude::*};

/// The handler tree every update is routed through.
///
/// Separate from `run` so a test can push a synthetic update through the real
/// branch order: which branch claims an update is behaviour, and a schema
/// reassembled inside a test would no longer be the one production runs.
pub fn schema(
    allowed_user_ids: HashSet<String>,
) -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry()
        .chain(middlewares::install())
        // Must precede the route branches; see `reject_unauthorized`.
        .branch(middlewares::reject_unauthorized(allowed_user_ids))
        .branch(commands::routes())
        .branch(callbacks::routes())
        .branch(dialogue::routes())
}

/// What handlers extract from: the use cases plus the two dialogue storages.
pub fn deps_map(deps: Deps) -> DependencyMap {
    dptree::deps![
        deps,
        InMemStorage::<DialogueState>::new(),
        InMemStorage::<BotContext>::new()
    ]
}

pub async fn run(bot: Bot, deps: Deps, allowed_user_ids: HashSet<String>) -> anyhow::Result<()> {
    let mut dispatcher = Dispatcher::builder(bot, schema(allowed_user_ids))
        .dependencies(deps_map(deps))
        .enable_ctrlc_handler()
        .build();

    dispatcher.dispatch().await;
    Ok(())
}
