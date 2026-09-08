//! The MCP surface over HTTP.
//!
//! The protocol itself is rmcp's: `StreamableHttpService` already is an
//! `http::Request` handler, so this module is only the two things rmcp cannot
//! decide — who the caller is, and what a refusal looks like.
//!
//! Authentication happens before the request reaches the protocol, and the tool
//! surface is then built around the principal it resolved. That ordering is the
//! point: a tool is never constructed for an unauthenticated caller, so there is
//! no path where a missing check leaves a tool reachable.

use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderValue, Request, Response, StatusCode, header};
use http_body_util::{BodyExt, Full};
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

use super::auth::{StaticToken, Verified};
use super::{BotTools, Deps};

pub struct HttpMcp {
    deps: Deps,
    tokens: StaticToken,
}

impl HttpMcp {
    pub fn new(deps: Deps, tokens: StaticToken) -> Self {
        Self { deps, tokens }
    }

    /// Serve one request. The body arrives already collected because the caller
    /// is a Lambda, which is handed the whole payload up front.
    pub async fn handle(&self, request: Request<Bytes>) -> Response<Bytes> {
        let Some(principal) = bearer(&request).and_then(|t| self.tokens.verify(t)) else {
            return unauthorized();
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

/// A refusal that tells the client how to authenticate, per RFC 6750. The body
/// says nothing about why: distinguishing "no token" from "wrong token" is a
/// free probe for anyone guessing.
fn unauthorized() -> Response<Bytes> {
    let mut response = Response::new(Bytes::from_static(b"unauthorized"));
    *response.status_mut() = StatusCode::UNAUTHORIZED;
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Bearer error=\"invalid_token\""),
    );
    response
}
