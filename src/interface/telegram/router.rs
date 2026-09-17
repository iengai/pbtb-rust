// Rust
use crate::interface::telegram::{
    Deps, bind, callbacks, commands, dialogue, middlewares,
    states::{BotContext, DialogueState},
};
use teloxide::dispatching::{DefaultKey, dialogue::InMemStorage};
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

/// The dispatcher `run` drives.
///
/// Building it type-checks the handler tree against the dependencies and panics
/// on a value some handler extracts but nothing provides, so a test builds it
/// to catch at test time what would otherwise crash the telebot at startup.
pub fn dispatcher(
    bot: Bot,
    deps: DependencyMap,
    site_url: String,
) -> Dispatcher<Bot, DependencyMap, DefaultKey> {
    Dispatcher::builder(bot, schema(site_url))
        .dependencies(deps)
        .enable_ctrlc_handler()
        .build()
}

pub async fn run(bot: Bot, deps: Deps, site_url: String) -> anyhow::Result<()> {
    let mut dispatcher = dispatcher(bot, deps_map(deps), site_url);
    shutdown_on_sigterm(dispatcher.shutdown_token());

    dispatcher.dispatch().await;
    Ok(())
}

/// Shut the dispatcher down on SIGTERM, the signal systemd and `docker stop` end
/// the container with. teloxide's own handler listens for SIGINT only, and the
/// binary runs as PID 1 in its container, where an unhandled SIGTERM is ignored:
/// without this a stop waits out its timeout and ends in SIGKILL, cutting off any
/// update mid-handler, a Run between its start-lock claim and the task-id attach
/// included. The shutdown lets in-flight handlers finish first.
#[cfg(unix)]
fn shutdown_on_sigterm(token: teloxide::dispatching::ShutdownToken) {
    use tokio::signal::unix::{SignalKind, signal};
    // Registered before the spawn, so a SIGTERM that lands before the task is
    // first polled is queued rather than lost.
    let mut term = match signal(SignalKind::terminate()) {
        Ok(term) => term,
        Err(e) => {
            tracing::warn!(error = %e, "cannot listen for SIGTERM; a stop will end in SIGKILL");
            return;
        }
    };
    tokio::spawn(async move {
        term.recv().await;
        tracing::info!("SIGTERM received, shutting the dispatcher down");
        // A SIGTERM during startup (the webhook deletion and `get_me` before
        // dispatching begins) finds the dispatcher idle, and shutdown refuses an
        // idle dispatcher; the request is held until dispatching has begun.
        loop {
            match token.shutdown() {
                Ok(done) => {
                    done.await;
                    return;
                }
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(200)).await,
            }
        }
    });
}

#[cfg(not(unix))]
fn shutdown_on_sigterm(_token: teloxide::dispatching::ShutdownToken) {}
