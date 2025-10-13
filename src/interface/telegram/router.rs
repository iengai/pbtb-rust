// Rust
use teloxide::{dispatching::Dispatcher, prelude::*};
use teloxide::dispatching::dialogue::InMemStorage;
use crate::interface::telegram::{commands, callbacks, dialogue, middlewares, Deps, states::{DialogueState, BotContext}};

pub async fn run(bot: Bot, deps: Deps) -> anyhow::Result<()> {
    // Inject dependencies into DependencyMap for extraction in handlers
    let deps_map = dptree::deps![
        deps,
        InMemStorage::<DialogueState>::new(),
        InMemStorage::<BotContext>::new()
    ];

    // Explicitly annotate schema type as UpdateHandler<DependencyMap>
    let schema: teloxide::dispatching::UpdateHandler<DependencyMap> = dptree::entry()
        .chain(middlewares::install())
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