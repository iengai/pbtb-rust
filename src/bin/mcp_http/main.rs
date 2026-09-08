//! The MCP server behind a Lambda Function URL.
//!
//! Public ingress, so the credential is presented per request rather than
//! assumed from the transport the way stdio's is.
//!
//! Which credential depends on whether an issuer is configured. With one, tokens
//! are verified against its published keys and the subject is resolved through
//! the identity table, so each caller is a person. Without one, a single shared
//! bearer stands for the whole deployment — enough to close the door, not enough
//! to tell two callers apart.
//!
//! Both are read from SSM at cold start and held for the life of the execution
//! environment, so rotating either takes effect as environments recycle rather
//! than instantly.

#[path = "../shared/mcp_deps.rs"]
mod composition;

use composition::mcp_deps;

use anyhow::{Context, bail};
use lambda_http::{Body, Error, Request, Response, http, run, service_fn};
use pbtb_rust::config::configs::{Configs, load_config};
use pbtb_rust::domain::IdentityRepository;
use pbtb_rust::infra::DynamoBotRepository;
use pbtb_rust::infra::client::setup_dynamodb_with_configs;
use pbtb_rust::interface::mcp::http::Metadata;
use pbtb_rust::interface::mcp::{HttpMcp, OAuthTokens, StaticToken, TokenVerifier};
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

    let ssm = aws_sdk_ssm::Client::new(
        &aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await,
    );
    let resource = read_param(
        &ssm,
        &configs.mcp.resource_url_param,
        "APP__MCP__RESOURCE_URL_PARAM",
    )
    .await?;

    let issuer = configs.mcp.issuer.trim().to_string();
    let (tokens, metadata): (Arc<dyn TokenVerifier>, Metadata) = if issuer.is_empty() {
        let token = read_param(&ssm, &configs.mcp.token_param, "APP__MCP__TOKEN_PARAM").await?;
        (
            Arc::new(StaticToken::new(token, user_id)),
            Metadata::unissued(&resource),
        )
    } else {
        let (client, table) = setup_dynamodb_with_configs(&configs).await;
        let identities: Arc<dyn IdentityRepository> =
            Arc::new(DynamoBotRepository::new(client, table));
        let verifier =
            OAuthTokens::discover(&issuer, &resource, identities, allowed.clone()).await?;
        (
            Arc::new(verifier),
            Metadata {
                resource: resource.clone(),
                authorization_servers: vec![issuer],
            },
        )
    };

    Ok(HttpMcp::new(mcp_deps(&configs).await?, tokens, metadata))
}

/// Read one parameter from SSM. A failure here aborts the cold start rather than
/// letting the server come up with a missing credential or an audience it cannot
/// check tokens against.
async fn read_param(
    client: &aws_sdk_ssm::Client,
    param: &str,
    env_var: &str,
) -> anyhow::Result<String> {
    if param.trim().is_empty() {
        bail!("{env_var}: set it to the SSM parameter to read");
    }
    let value = client
        .get_parameter()
        .name(param)
        .with_decryption(true)
        .send()
        .await
        .with_context(|| format!("Failed to read {param}"))?
        .parameter
        .and_then(|p| p.value)
        .unwrap_or_default();

    if value.trim().is_empty() {
        bail!("{param} has no value; set it with `aws ssm put-parameter`");
    }
    Ok(value.trim().to_string())
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
