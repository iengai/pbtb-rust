//! The MCP surface over HTTP.
//!
//! The protocol itself is rmcp's: `StreamableHttpService` already is an
//! `http::Request` handler, so this module is only the parts rmcp cannot decide
//! — who the caller is, what a refusal looks like, and where a client goes to
//! get a token.
//!
//! Authentication happens before the request reaches the protocol, and the tool
//! surface is then built around the principal it resolved. That ordering is the
//! point: a tool is never constructed for an unauthenticated caller, so there is
//! no path where a missing check leaves a tool reachable.

use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderValue, Method, Request, Response, StatusCode, header};
use http_body_util::{BodyExt, Full};
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde_json::json;

use super::auth::{AuthError, SCOPE_READ, SCOPE_WRITE, TokenVerifier, Verified};
use super::{BotTools, Deps};

/// RFC 9728 publishes protected-resource metadata at this path, and clients also
/// probe the path-suffix form for a resource that lives under a path. Both are
/// served so a client finds the authorization server either way.
const METADATA_PATH: &str = "/.well-known/oauth-protected-resource";

pub struct HttpMcp {
    deps: Deps,
    tokens: Arc<dyn TokenVerifier>,
    metadata: Metadata,
}

/// What an unauthenticated client is told, so it can go and get a token.
///
/// `resource` has to be the identifier the authorization server mints tokens
/// for; it is what ties a token to this server and not to some other API the
/// same user authorized.
#[derive(Clone)]
pub struct Metadata {
    pub resource: String,
    pub authorization_servers: Vec<String>,
}

impl Metadata {
    /// Metadata for a deployment with no authorization server — the shared-token
    /// transport. Discovery still answers, naming the resource and no issuer, so
    /// a client learns there is nowhere to go for a token rather than hanging on
    /// a 404.
    pub fn unissued(resource: impl Into<String>) -> Self {
        Self {
            resource: resource.into(),
            authorization_servers: Vec::new(),
        }
    }
}

impl HttpMcp {
    pub fn new(deps: Deps, tokens: Arc<dyn TokenVerifier>, metadata: Metadata) -> Self {
        Self {
            deps,
            tokens,
            metadata,
        }
    }

    /// Serve one request. The body arrives already collected because the caller
    /// is a Lambda, which is handed the whole payload up front.
    pub async fn handle(&self, request: Request<Bytes>) -> Response<Bytes> {
        if is_metadata_probe(&request) {
            return self.metadata_response();
        }

        let principal = match bearer(&request) {
            Some(token) => self.tokens.verify(token).await,
            None => Err(AuthError::Unauthenticated("no bearer presented".into())),
        };
        let principal = match principal {
            Ok(principal) => principal,
            Err(refusal) => {
                // The reason is logged, never returned: telling a caller whether
                // a token failed verification or merely belongs to nobody here
                // is a free probe.
                tracing::info!(%refusal, "refused an MCP request");
                return self.refuse(&refusal);
            }
        };

        let tools = BotTools::new(self.deps.clone(), Arc::new(Verified(principal)));
        let service = StreamableHttpService::new(
            move || Ok(tools.clone()),
            Arc::new(NeverSessionManager::default()),
            {
                // No session survives a Lambda invocation, so none is minted.
                // The `2026-07-28` revision has no sessions at all; older clients
                // are served statelessly rather than handed an id that would stop
                // resolving the moment this execution environment goes.
                let mut config = StreamableHttpServerConfig::default();
                config.legacy_session_mode = false;
                config.json_response = true;
                // Host validation defends a server bound to loopback against a
                // browser being steered at it, which is not this deployment: the
                // authority is AWS's and fixed, and the bearer is the control. It
                // is also unwireable here — the function's env cannot name the URL
                // of the function it belongs to.
                config.disable_allowed_hosts()
            },
        );

        let response = service.handle(request.map(Full::new)).await;
        let (parts, body) = response.into_parts();
        let bytes = match body.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(never) => match never {},
        };
        Response::from_parts(parts, bytes)
    }

    fn metadata_response(&self) -> Response<Bytes> {
        let body = json!({
            "resource": self.metadata.resource,
            "authorization_servers": self.metadata.authorization_servers,
            "bearer_methods_supported": ["header"],
            "scopes_supported": [SCOPE_READ, SCOPE_WRITE],
        });

        let mut response = Response::new(Bytes::from(body.to_string()));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        response
    }

    /// A refusal that points at the metadata document, per RFC 9728 §5.1, so a
    /// client can discover where to authenticate from the failure itself.
    ///
    /// The status carries the whole distinction: 401 says get a better token,
    /// 403 says the token is fine and the answer is still no. A client that
    /// cannot tell them apart refreshes forever against a decision no token
    /// changes.
    fn refuse(&self, refusal: &AuthError) -> Response<Bytes> {
        // No `error=` on the 403. RFC 6750 defines the codes for token problems,
        // and this token has none — the identity behind it simply has no account
        // here. Borrowing `insufficient_scope` would send the client off to ask
        // for scopes that would change nothing.
        let (status, error) = match refusal {
            AuthError::Unauthenticated(_) => {
                (StatusCode::UNAUTHORIZED, "error=\"invalid_token\", ")
            }
            AuthError::Forbidden(_) => (StatusCode::FORBIDDEN, ""),
        };

        let mut response = Response::new(Bytes::from_static(b"{}"));
        *response.status_mut() = status;
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        let challenge = format!(
            "Bearer {error}resource_metadata=\"{}{METADATA_PATH}\"",
            self.metadata.resource.trim_end_matches('/')
        );
        if let Ok(value) = HeaderValue::from_str(&challenge) {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, value);
        }
        response
    }
}

/// Whether this is a client asking where to authenticate. Unauthenticated by
/// design — a caller with no token is exactly who needs the answer.
fn is_metadata_probe<T>(request: &Request<T>) -> bool {
    let path = request.uri().path();
    request.method() == Method::GET
        && (path == METADATA_PATH || path.starts_with(&format!("{METADATA_PATH}/")))
}

/// The bearer token presented, if the header is well formed.
fn bearer<T>(request: &Request<T>) -> Option<&str> {
    let value = request
        .headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim())
        .filter(|t| !t.is_empty())
}
