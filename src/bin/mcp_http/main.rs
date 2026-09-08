//! The MCP server behind a Lambda Function URL.
//!
//! Public ingress, so the credential is presented per request rather than
//! assumed from the transport the way stdio's is. The token is a single shared
//! bearer standing for one tenant — enough to close the door, not enough to tell
//! two callers apart; per-user identity is what OAuth is for.
//!
//! The bearer is fetched from SSM at cold start and kept for the life of the
//! execution environment, so rotating it takes effect as environments recycle
//! rather than instantly.

#[path = "../shared/mcp_deps.rs"]
mod composition;

use composition::mcp_deps;

use anyhow::{Context, bail};
use lambda_http::{Body, Error, Request, Response, http, run, service_fn};
use pbtb_rust::config::configs::{Configs, load_config};
use pbtb_rust::interface::mcp::{HttpMcp, StaticToken};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let mcp = Arc::new(build().await?);
    run(service_fn(move |request: Request| {
        let mcp = mcp.clone();
        async move { serve(&mcp, request).await }
    }))
    .await
}

async fn build() -> anyhow::Result<HttpMcp> {
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

    let token = fetch_token(&configs.mcp.token_param).await?;
    Ok(HttpMcp::new(
        mcp_deps(&configs).await?,
        StaticToken::new(token, user_id),
    ))
}

/// Read the bearer from SSM. A failure here aborts the cold start: a server that
/// came up without its token would have to either refuse everyone or, far worse,
/// accept them.
async fn fetch_token(param: &str) -> anyhow::Result<String> {
    if param.trim().is_empty() {
        bail!("APP__MCP__TOKEN_PARAM: set it to the SSM parameter holding the bearer token");
    }
    let aws = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .load()
        .await;
    let client = aws_sdk_ssm::Client::new(&aws);
    let token = client
        .get_parameter()
        .name(param)
        .with_decryption(true)
        .send()
        .await
        .with_context(|| format!("Failed to read {param}"))?
        .parameter
        .and_then(|p| p.value)
        .unwrap_or_default();

    if token.trim().is_empty() {
        bail!("{param} has no value; set the bearer token with `aws ssm put-parameter`");
    }
    Ok(token)
}

async fn serve(mcp: &HttpMcp, request: Request) -> Result<Response<Body>, Error> {
    let (parts, body) = request.into_parts();
    let bytes = match body {
        Body::Empty => Default::default(),
        Body::Text(text) => text.into(),
        Body::Binary(bytes) => bytes.into(),
    };
    let response = mcp.handle(http::Request::from_parts(parts, bytes)).await;
    Ok(response.map(|bytes| Body::from(bytes.to_vec())))
}
