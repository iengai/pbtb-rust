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

use composition::api_deps;

use anyhow::{Context, bail};
use lambda_http::{Body, Error, Request, Response, http, run, service_fn};
use pbtb_rust::config::configs::{Configs, load_config};
use pbtb_rust::domain::identity::LinkTicketRepository;
use pbtb_rust::domain::{IdentityRepository, SystemClock};
use pbtb_rust::infra::DynamoBotRepository;
use pbtb_rust::infra::client::setup_dynamodb_with_configs;
use pbtb_rust::interface::api::WebApi;
use pbtb_rust::interface::link::{LinkFlow, OAuthClient, PATH_CALLBACK};
use pbtb_rust::interface::mcp::http::Metadata;
use pbtb_rust::interface::mcp::{HttpMcp, OAuthTokens, StaticToken, TokenVerifier};
use std::sync::Arc;

/// The provider half of an identity key. One authorization server, so one name.
const PROVIDER: &str = "workos";

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let server = Arc::new(build().await?);
    run(service_fn(move |request: Request| {
        let server = server.clone();
        async move { serve(&server, request).await }
    }))
    .await
}

/// The surfaces this function serves. They share a host so that the resource a
/// token is minted for, the redirect that mints the link, and the API the web
/// console calls are the same origin, which is one fewer thing to register and
/// one fewer thing to get wrong.
struct Server {
    mcp: HttpMcp,
    api: WebApi,
    link: Option<LinkFlow>,
}

async fn build() -> anyhow::Result<Server> {
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
                authorization_servers: vec![issuer.clone()],
            },
        )
    };

    let link = build_link(&configs, &ssm, &issuer, &resource).await?;

    let deps = api_deps(&configs).await?;
    Ok(Server {
        mcp: HttpMcp::new(deps.mcp.clone(), tokens.clone(), metadata.clone()),
        api: WebApi::new(deps, tokens, metadata),
        link,
    })
}

/// Wire the link flow, or leave it off.
///
/// Off is the default and off means unreachable: without an issuer to send
/// people to, or a client registered with it, the routes are not served at all
/// rather than served and failing.
async fn build_link(
    configs: &Configs,
    ssm: &aws_sdk_ssm::Client,
    issuer: &str,
    resource: &str,
) -> anyhow::Result<Option<LinkFlow>> {
    let client_id = configs.link.client_id.trim();
    if issuer.is_empty() || client_id.is_empty() {
        return Ok(None);
    }

    let secret = read_param(
        ssm,
        &configs.link.client_secret_param,
        "APP__LINK__CLIENT_SECRET_PARAM",
    )
    .await?;
    let redirect_uri = format!("{}{}", resource.trim_end_matches('/'), PATH_CALLBACK);

    let (client, table) = setup_dynamodb_with_configs(configs).await;
    let repository = Arc::new(DynamoBotRepository::new(client, table));
    let tickets: Arc<dyn LinkTicketRepository> = repository.clone();
    let identities: Arc<dyn IdentityRepository> = repository;

    Ok(Some(LinkFlow::new(
        tickets,
        identities,
        Arc::new(SystemClock),
        OAuthClient::discover(issuer, client_id, secret, redirect_uri).await?,
        PROVIDER,
    )))
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

async fn serve(server: &Server, request: Request) -> Result<Response<Body>, Error> {
    let (parts, body) = request.into_parts();
    let bytes = match body {
        Body::Empty => Default::default(),
        Body::Text(text) => text.into(),
        Body::Binary(bytes) => bytes.into(),
    };
    let request = http::Request::from_parts(parts, bytes);

    let response = match &server.link {
        Some(link) => match link.handle(&request).await {
            Some(response) => response,
            None => serve_authenticated(server, request).await,
        },
        None => serve_authenticated(server, request).await,
    };
    Ok(response.map(|bytes| Body::from(bytes.to_vec())))
}

/// The two bearer-protected surfaces, told apart by path prefix.
async fn serve_authenticated(
    server: &Server,
    request: http::Request<bytes::Bytes>,
) -> http::Response<bytes::Bytes> {
    if WebApi::owns(&request) {
        server.api.handle(request).await
    } else {
        server.mcp.handle(request).await
    }
}
