//! The MCP surface over HTTP, through the same edge the Lambda runs.
//!
//! The tools themselves are covered by `mcp_tools.rs`; what is asserted here is
//! the part the HTTP transport adds and the part a public endpoint gets wrong:
//! that nothing at all is reachable without the bearer, and that a request is
//! answered without a session the next invocation would not have.

mod common;

use bytes::Bytes;
use common::{Harness, RESOURCE};
use http::{Request, Response, StatusCode, header};
use serde_json::{Value, json};

const TOKEN: &str = "a-shared-bearer";

macro_rules! harness {
    () => {
        match Harness::start().await {
            Some(h) => h,
            None => return,
        }
    };
}

/// A `2026-07-28` request. That revision carries the body's method in headers so
/// a proxy can route and authorize without parsing JSON-RPC, and the server
/// refuses a request whose headers and body disagree — so they are built from
/// the body here rather than written out twice.
fn post(auth: Option<&str>, body: Value) -> Request<Bytes> {
    let method = body["method"].as_str().unwrap_or_default().to_string();
    let mut request = Request::builder()
        .method("POST")
        .uri(RESOURCE)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", method);
    if let Some(name) = body["params"]["name"].as_str() {
        request = request.header("mcp-name", name);
    }
    if let Some(token) = auth {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    request
        .body(Bytes::from(body.to_string()))
        .expect("a valid request")
}

fn json_body(response: &Response<Bytes>) -> Value {
    serde_json::from_slice(response.body()).unwrap_or_else(|e| {
        panic!(
            "expected a json body, got {:?}: {e}",
            String::from_utf8_lossy(response.body())
        )
    })
}

/// The per-request `_meta` the `2026-07-28` revision carries in place of the
/// `initialize` handshake it removed. Without a session there is nowhere to
/// remember the client's protocol version and capabilities, so every request
/// restates them.
fn meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

fn list_tools() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": { "_meta": meta() },
    })
}

#[tokio::test]
async fn a_request_without_a_bearer_is_refused_and_told_how_to_authenticate() {
    let h = harness!();

    let response = h.http_mcp(TOKEN).handle(post(None, list_tools())).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(
        response.headers().contains_key(header::WWW_AUTHENTICATE),
        "a 401 that does not name the scheme leaves the client guessing"
    );
}

#[tokio::test]
async fn a_wrong_bearer_learns_nothing_a_missing_one_would_not() {
    let h = harness!();
    let mcp = h.http_mcp(TOKEN);

    let wrong = mcp.handle(post(Some("not-the-token"), list_tools())).await;
    let missing = mcp.handle(post(None, list_tools())).await;

    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    // Telling the two apart turns a guess into a signal about how far off it was.
    assert_eq!(wrong.body(), missing.body());
    assert_eq!(
        wrong.headers().get(header::WWW_AUTHENTICATE),
        missing.headers().get(header::WWW_AUTHENTICATE)
    );
}

#[tokio::test]
async fn the_bearer_reaches_the_tools() {
    let h = harness!();

    let response = h
        .http_mcp(TOKEN)
        .handle(post(Some(TOKEN), list_tools()))
        .await;

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(response.body())
    );
    let names: Vec<String> = json_body(&response)["result"]["tools"]
        .as_array()
        .expect("a tool list")
        .iter()
        .map(|t| t["name"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(names.contains(&"list_bots".to_string()), "got {names:?}");
    assert!(names.contains(&"start_bot".to_string()), "got {names:?}");
}

#[tokio::test]
async fn a_call_is_answered_without_minting_a_session() {
    let h = harness!();

    let response = h
        .http_mcp(TOKEN)
        .handle(post(
            Some(TOKEN),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": { "name": "list_bots", "arguments": {}, "_meta": meta() },
            }),
        ))
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    // A session id would be a promise this deployment cannot keep: the next
    // request may land in a different execution environment, which has never
    // heard of it.
    assert!(
        !response.headers().contains_key("mcp-session-id"),
        "headers: {:?}",
        response.headers()
    );
    assert!(
        json_body(&response)["result"]["isError"] != json!(true),
        "the call should have run: {}",
        String::from_utf8_lossy(response.body())
    );
}

#[tokio::test]
async fn an_unauthenticated_get_never_reaches_the_protocol() {
    let h = harness!();

    let response = h
        .http_mcp(TOKEN)
        .handle(
            Request::builder()
                .method("GET")
                .uri(RESOURCE)
                .body(Bytes::new())
                .expect("a valid request"),
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn discovery_answers_without_a_token() {
    let h = harness!();

    let response = h
        .http_mcp(TOKEN)
        .handle(
            Request::builder()
                .method("GET")
                .uri(format!("{RESOURCE}.well-known/oauth-protected-resource"))
                .body(Bytes::new())
                .expect("a valid request"),
        )
        .await;

    // A caller with no token is exactly who needs this answer, so requiring one
    // would make the document unreachable to everyone it is for.
    assert_eq!(response.status(), StatusCode::OK);
    let document = json_body(&response);
    assert_eq!(document["resource"], json!(RESOURCE));
    assert_eq!(document["bearer_methods_supported"], json!(["header"]));
    let scopes = document["scopes_supported"]
        .as_array()
        .expect("scopes")
        .clone();
    assert!(scopes.contains(&json!("bots:read")));
    assert!(scopes.contains(&json!("bots:write")));
}

#[tokio::test]
async fn a_refusal_points_at_the_discovery_document() {
    let h = harness!();

    let response = h.http_mcp(TOKEN).handle(post(None, list_tools())).await;

    let challenge = response
        .headers()
        .get(header::WWW_AUTHENTICATE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        challenge.contains("resource_metadata=\""),
        "a client should be able to find the issuer from the failure alone: {challenge}"
    );
    assert!(
        challenge.contains("/.well-known/oauth-protected-resource"),
        "got: {challenge}"
    );
}
