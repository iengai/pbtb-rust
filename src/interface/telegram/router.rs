// Rust
use crate::interface::telegram::{
    Deps, callbacks, commands, dialogue, middlewares,
    states::{BotContext, DialogueState},
};
use std::collections::HashSet;
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::{dispatching::Dispatcher, prelude::*};

pub async fn run(bot: Bot, deps: Deps, allowed_user_ids: HashSet<String>) -> anyhow::Result<()> {
    // Inject dependencies into DependencyMap for extraction in handlers
    let deps_map = dptree::deps![
        deps,
        InMemStorage::<DialogueState>::new(),
        InMemStorage::<BotContext>::new()
    ];

    // Explicitly annotate schema type as UpdateHandler<DependencyMap>
    let schema: teloxide::dispatching::UpdateHandler<DependencyMap> = dptree::entry()
        .chain(middlewares::install())
        // Must precede the route branches; see `reject_unauthorized`.
        .branch(middlewares::reject_unauthorized(allowed_user_ids))
        .branch(commands::routes())
        .branch(callbacks::routes())
        .branch(dialogue::routes());

    let mut dispatcher = Dispatcher::builder(bot, schema)
        .dependencies(deps_map)
        .enable_ctrlc_handler()
        .build();

    dispatcher.dispatch().await;
    Ok(())
}
