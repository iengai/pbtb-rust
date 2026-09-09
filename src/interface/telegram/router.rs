// Rust
use crate::interface::telegram::{
    Deps, bind, callbacks, commands, dialogue, middlewares,
    states::{BotContext, DialogueState},
};
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::{dispatching::Dispatcher, prelude::*};

/// The handler tree every update is routed through.
///
/// Separate from `run` so a test can push a synthetic update through the real
/// branch order: which branch claims an update is behaviour, and a schema
/// reassembled inside a test would no longer be the one production runs.
///
/// Order: the bind command first, because its sender is by definition not yet
/// known; then every other branch behind sender resolution; then the refusal
/// for whatever resolution did not admit.
pub fn schema(site_url: String) -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry()
        .chain(middlewares::install())
        .branch(bind::routes())
        .chain(middlewares::resolve_sender())
        .branch(
            middlewares::known_sender()
                .branch(commands::routes())
                .branch(callbacks::routes())
                .branch(dialogue::routes()),
        )
        .branch(middlewares::refuse_unresolved(site_url))
}

/// What handlers extract from: the use cases plus the two dialogue storages.
pub fn deps_map(deps: Deps) -> DependencyMap {
    dptree::deps![
        deps,
        InMemStorage::<DialogueState>::new(),
        InMemStorage::<BotContext>::new()
    ]
}

pub async fn run(bot: Bot, deps: Deps, site_url: String) -> anyhow::Result<()> {
    let mut dispatcher = Dispatcher::builder(bot, schema(site_url))
        .dependencies(deps_map(deps))
        .enable_ctrlc_handler()
        .build();

    dispatcher.dispatch().await;
    Ok(())
}
