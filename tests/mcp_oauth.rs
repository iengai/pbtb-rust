//! The MCP surface reached with an OAuth token.
//!
//! Everything here is about who a token turns out to be, and what happens when
//! it turns out to be nobody. The tools themselves are covered elsewhere; a
//! passing tool call appears here only as evidence that a good token got through.

mod common;

use std::collections::HashSet;
use std::sync::Arc;

use bytes::Bytes;
use common::oauth::{FakeIssuer, SUBJECT, now};
use common::telegram::USER_ID;
use common::{Harness, NOW, RESOURCE, TD_V7};
use http::{Request, Response, StatusCode, header};
use pbtb_rust::domain::bot::Bot;
use pbtb_rust::domain::botconfig::{BotConfig, BotType};
use pbtb_rust::domain::engine::Runtime;
use pbtb_rust::domain::exchange::Exchange;
use pbtb_rust::interface::mcp::http::Metadata;
use pbtb_rust::interface::mcp::{HttpMcp, OAuthTokens, TokenVerifier};
use serde_json::{Value, json};

const BOT_ID: &str = "alpha";
const PROVIDER: &str = "workos";

/// The tenant, as a string: `USER_ID` is the telegram numeric id and every row
/// is keyed under its text form.
fn tenant() -> String {
    USER_ID.to_string()
}

macro_rules! harness {
    () => {
        match Harness::start().await {
            Some(h) => h,
            None => return,
        }
    };
}

fn a_bot() -> Bot {
    Bot::new(
        BOT_ID.to_string(),
        USER_ID.to_string(),
        Exchange::Bybit,
        BOT_ID.to_string(),
        "ak".to_string(),
        "sk".to_string(),
        false,
        Runtime::Py,
        NOW,
        NOW,
    )
}

fn a_config() -> BotConfig {
    BotConfig {
        user_id: USER_ID.to_string(),
        bot_id: BOT_ID.to_string(),
        bot_type: BotType::default(),
        template_name: "test".to_string(),
        template_version: None,
        config_data: json!({ "config_version": "v7.12.0", "live": {} }),
        created_at: NOW,
        updated_at: NOW,
    }
}

/// The edge with the real OAuth verifier pointed at a stand-in issuer.
///
/// `allowed` is the telegram allowlist, passed separately from the linked tenant
/// so a test can put an identity in the table and still leave its account off
/// the list — which is what revoking someone from the bot looks like from here.
async fn edge(h: &Harness, issuer: &FakeIssuer, allowed: HashSet<String>) -> HttpMcp {
    let verifier = OAuthTokens::discover(issuer.issuer(), RESOURCE, h.bots.clone(), allowed)
        .await
        .expect("the issuer publishes discovery and a key set");

    h.http_mcp_with(
        Arc::new(verifier) as Arc<dyn TokenVerifier>,
        Metadata {
            resource: RESOURCE.to_string(),
            authorization_servers: vec![issuer.issuer()],
        },
    )
}

fn only(user_id: &str) -> HashSet<String> {
    HashSet::from([user_id.to_string()])
}

fn meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

fn call(bearer: &str, body: Value) -> Request<Bytes> {
    let method = body["method"].as_str().unwrap_or_default().to_string();
    let mut request = Request::builder()
        .method("POST")
        .uri(RESOURCE)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", method)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"));
    if let Some(name) = body["params"]["name"].as_str() {
        request = request.header("mcp-name", name);
    }
    request
        .body(Bytes::from(body.to_string()))
        .expect("a valid request")
}

fn tool(name: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments, "_meta": meta() },
    })
}

fn body(response: &Response<Bytes>) -> Value {
    serde_json::from_slice(response.body()).unwrap_or_else(|e| {
        panic!(
            "expected json, got {:?}: {e}",
            String::from_utf8_lossy(response.body())
        )
    })
}

#[tokio::test]
async fn a_linked_subject_reaches_its_own_bots() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    h.given_bot(a_bot()).await;
    h.given_link(PROVIDER, SUBJECT, &tenant()).await;

    let token = issuer.token(issuer.claims(SUBJECT, "bots:read", RESOURCE));
    let response = edge(&h, &issuer, only(&tenant()))
        .await
        .handle(call(&token, tool("list_bots", json!({}))))
        .await;

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(response.body())
    );
    let text = body(&response)["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(text.contains(BOT_ID), "got: {text}");
}

#[tokio::test]
async fn an_unlinked_subject_is_refused_without_being_given_an_account() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;

    let token = issuer.token(issuer.claims("user_nobody_has_linked", "bots:read", RESOURCE));
    let response = edge(&h, &issuer, only(&tenant()))
        .await
        .handle(call(&token, tool("list_bots", json!({}))))
        .await;

    // 403, not 401: the token is fine. Sending 401 would put the client in a
    // refresh loop over a decision no new token changes.
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        !response
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .contains("error="),
        "a 403 that is not about the token must not name a token error"
    );
}

#[tokio::test]
async fn a_linked_account_off_the_allowlist_loses_access() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    h.given_bot(a_bot()).await;
    h.given_link(PROVIDER, SUBJECT, &tenant()).await;

    // The link survives; the account is simply no longer allowed to use the bot.
    // Removing someone from telegram has to take their MCP access with it, or the
    // allowlist is a door with a second key still in circulation.
    let token = issuer.token(issuer.claims(SUBJECT, "bots:read", RESOURCE));
    let response = edge(&h, &issuer, only("someone-else"))
        .await
        .handle(call(&token, tool("list_bots", json!({}))))
        .await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_token_minted_for_another_resource_is_refused() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    h.given_link(PROVIDER, SUBJECT, &tenant()).await;

    // The same user, the same issuer, a token they got for some other API.
    // Honouring it would let any service they also authorized replay it here.
    let token = issuer.token(issuer.claims(SUBJECT, "bots:write", "https://elsewhere.example/"));
    let response = edge(&h, &issuer, only(&tenant()))
        .await
        .handle(call(&token, tool("list_bots", json!({}))))
        .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_expired_token_is_refused() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    h.given_link(PROVIDER, SUBJECT, &tenant()).await;

    let mut claims = issuer.claims(SUBJECT, "bots:read", RESOURCE);
    claims["exp"] = json!(now() - 3600);
    let response = edge(&h, &issuer, only(&tenant()))
        .await
        .handle(call(&issuer.token(claims), tool("list_bots", json!({}))))
        .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_token_signed_by_an_unpublished_key_is_refused() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    h.given_link(PROVIDER, SUBJECT, &tenant()).await;

    // Correctly signed by the fixture key, but naming a `kid` the issuer does not
    // publish — the shape of both a rotated-away key and a forged header.
    let token = issuer.sign(
        "not-a-published-key",
        issuer.claims(SUBJECT, "bots:read", RESOURCE),
    );
    let response = edge(&h, &issuer, only(&tenant()))
        .await
        .handle(call(&token, tool("list_bots", json!({}))))
        .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_read_only_token_cannot_start_a_bot() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    h.given_bot(a_bot()).await;
    h.configs.put(a_config());
    h.given_link(PROVIDER, SUBJECT, &tenant()).await;

    let token = issuer.token(issuer.claims(SUBJECT, "bots:read", RESOURCE));
    let response = edge(&h, &issuer, only(&tenant()))
        .await
        .handle(call(&token, tool("start_bot", json!({ "bot_id": BOT_ID }))))
        .await;

    // The request itself is authentic, so it reaches the tool and is refused
    // there — the scope failure belongs in the protocol, not in the status line.
    assert_eq!(response.status(), StatusCode::OK);
    let refusal = body(&response);
    assert!(
        refusal["error"] != Value::Null || refusal["result"]["isError"] == json!(true),
        "the call should have been refused, not quietly done nothing: {refusal}"
    );
    assert!(
        h.ecs.launches().is_empty(),
        "a token without bots:write must not have launched anything"
    );
}

#[tokio::test]
async fn a_token_that_asked_for_write_can_start_a_bot() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    h.given_bot(a_bot()).await;
    h.configs.put(a_config());
    h.given_link(PROVIDER, SUBJECT, &tenant()).await;

    let token = issuer.token(issuer.claims(SUBJECT, "bots:read bots:write", RESOURCE));
    let response = edge(&h, &issuer, only(&tenant()))
        .await
        .handle(call(&token, tool("start_bot", json!({ "bot_id": BOT_ID }))))
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let launches = h.ecs.launches();
    assert_eq!(launches.len(), 1, "exactly one task, under the start lock");
    assert_eq!(launches[0].td_arn, TD_V7);
}

#[tokio::test]
async fn a_token_signed_with_a_published_secret_is_refused() {
    let h = harness!();
    // The issuer publishes an `oct` key, so its "public" key set contains the
    // secret that signs tokens. Anyone who can GET that document — everyone —
    // can then mint a token for any subject.
    let issuer = FakeIssuer::start_publishing_a_symmetric_key().await;
    h.given_link(PROVIDER, SUBJECT, &tenant()).await;

    let token = issuer.forge_symmetric(issuer.claims(SUBJECT, "bots:write", RESOURCE));
    let response = edge(&h, &issuer, only(&tenant()))
        .await
        .handle(call(&token, tool("list_bots", json!({}))))
        .await;

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "the signature verifies; refusing the algorithm is the only thing that stops it"
    );
    assert!(
        h.ecs.launches().is_empty(),
        "and nothing it asked for happened"
    );
}

#[tokio::test]
async fn a_token_with_no_scopes_cannot_even_list() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    h.given_bot(a_bot()).await;
    h.given_link(PROVIDER, SUBJECT, &tenant()).await;

    // A token issued for some other purpose entirely. Granting it a floor of
    // read access would make the scope claim decorative — this surface lists
    // every bot in the tenant and hands over its full trading config.
    let token = issuer.token(issuer.claims(SUBJECT, "openid profile email", RESOURCE));
    let response = edge(&h, &issuer, only(&tenant()))
        .await
        .handle(call(&token, tool("list_bots", json!({}))))
        .await;

    let refusal = body(&response);
    assert!(
        refusal["error"] != Value::Null || refusal["result"]["isError"] == json!(true),
        "the call should have been refused: {refusal}"
    );
    assert!(
        !refusal.to_string().contains(BOT_ID),
        "and must not have named a bot on the way: {refusal}"
    );
}

#[tokio::test]
async fn a_token_with_no_issuer_claim_is_refused() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    h.given_link(PROVIDER, SUBJECT, &tenant()).await;

    let mut claims = issuer.claims(SUBJECT, "bots:read", RESOURCE);
    claims.as_object_mut().expect("claims").remove("iss");
    let response = edge(&h, &issuer, only(&tenant()))
        .await
        .handle(call(&issuer.token(claims), tool("list_bots", json!({}))))
        .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_token_with_no_audience_is_refused() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    h.given_link(PROVIDER, SUBJECT, &tenant()).await;

    // Absent, not wrong. A verifier that only compares a present `aud` accepts
    // every token the issuer ever minted for anything.
    let mut claims = issuer.claims(SUBJECT, "bots:read", RESOURCE);
    claims.as_object_mut().expect("claims").remove("aud");
    let response = edge(&h, &issuer, only(&tenant()))
        .await
        .handle(call(&issuer.token(claims), tool("list_bots", json!({}))))
        .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_token_that_never_expires_is_refused() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    h.given_link(PROVIDER, SUBJECT, &tenant()).await;

    // A token with no `exp` is a token that is stolen once and useful forever.
    let mut claims = issuer.claims(SUBJECT, "bots:read", RESOURCE);
    claims.as_object_mut().expect("claims").remove("exp");
    let response = edge(&h, &issuer, only(&tenant()))
        .await
        .handle(call(&issuer.token(claims), tool("list_bots", json!({}))))
        .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_token_from_another_issuer_is_refused() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    h.given_link(PROVIDER, SUBJECT, &tenant()).await;

    let mut claims = issuer.claims(SUBJECT, "bots:read", RESOURCE);
    claims["iss"] = json!("https://not-our-issuer.example");
    let response = edge(&h, &issuer, only(&tenant()))
        .await
        .handle(call(&issuer.token(claims), tool("list_bots", json!({}))))
        .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
