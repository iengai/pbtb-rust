//! The MCP server over stdio: bot management as tools, for a client running on
//! the operator's own machine.
//!
//! stdio is the transport where the process itself is the credential — the
//! client spawns this binary as a child of a shell the operator already
//! controls — so the principal is fixed at startup rather than presented per
//! request. The HTTP transport, where that reasoning does not hold, takes its
//! own `Authenticator`; nothing in the tools changes between the two.
//!
//! Nothing is logged to stdout: stdout IS the protocol stream. The subscriber
//! writes to stderr.

#[path = "../shared/mcp_deps.rs"]
mod composition;

use composition::mcp_deps;

use anyhow::{Context, bail};
use pbtb_rust::config::configs::{Configs, load_config};
use pbtb_rust::interface::mcp::{Authenticator, BotTools, LocalOperator};
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let configs: Configs = load_config().context("Failed to load config")?;

    let user_id = configs.mcp.user_id.trim().to_string();
    if user_id.is_empty() {
        bail!("APP__MCP__USER_ID: set it to the Telegram user id this server acts as");
    }
    // The same allowlist the bot enforces, checked here too: an operator removed
    // from telebot loses MCP access with them, rather than keeping a second door.
    let allowed = configs
        .telegram
        .allowlist()
        .context("APP__TELEGRAM__ALLOWED_USER_IDS")?;
    if !allowed.contains(&user_id) {
        bail!("APP__MCP__USER_ID={user_id} is not on APP__TELEGRAM__ALLOWED_USER_IDS");
    }

    let deps = mcp_deps(&configs).await?;
    let auth: Arc<dyn Authenticator> = Arc::new(LocalOperator::new(user_id));

    let service = BotTools::new(deps, auth)
        .serve(stdio())
        .await
        .context("Failed to start the MCP stdio server")?;
    service.waiting().await.context("MCP server stopped")?;
    Ok(())
}
