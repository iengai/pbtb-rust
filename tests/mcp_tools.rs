//! The MCP tool surface, over the same repositories the Telegram adapter drives.
//!
//! Several of these assert the boundaries rather than the behaviour: that no
//! tool can name another tenant, that no tool takes exchange credentials, that
//! no tool overwrites a whole config. Those are the properties that make it safe
//! to hand this surface to a model, and they are one careless `#[tool]` away
//! from being untrue, so they are asserted against the registry itself.

mod common;

use std::collections::HashSet;
use std::sync::Arc;

use common::telegram::USER_ID;
use common::{Harness, NOW, TD_V7};
use pbtb_rust::domain::bot::Bot;
use pbtb_rust::domain::botconfig::{BotConfig, BotType};
use pbtb_rust::domain::engine::Runtime;
use pbtb_rust::domain::exchange::Exchange;
use pbtb_rust::interface::mcp::{
    Authenticator, BotTools, LocalOperator, Principal, SCOPE_CONFIG_READ, SCOPE_READ, SCOPE_WRITE,
};
use rmcp::model::CallToolResult;
use serde_json::{Value, json};

const BOT_ID: &str = "alpha";
const OTHER_TENANT: &str = "999888777";

macro_rules! harness {
    () => {
        match Harness::start().await {
            Some(h) => h,
            None => return,
        }
    };
}

fn a_bot(user_id: &str, bot_id: &str) -> Bot {
    Bot::new(
        bot_id.to_string(),
        user_id.to_string(),
        Exchange::Bybit,
        bot_id.to_string(),
        "KEY-DO-NOT-LEAK".to_string(),
        "SECRET-DO-NOT-LEAK".to_string(),
        false,
        Runtime::Py,
        NOW,
        NOW,
    )
}

fn a_config(user_id: &str, bot_id: &str) -> BotConfig {
    BotConfig {
        user_id: user_id.to_string(),
        bot_id: bot_id.to_string(),
        bot_type: BotType::default(),
        template_name: "test".to_string(),
        template_version: None,
        // A `bot` section, so a walk of the read-only tools has real strategy
        // parameters to leak if one of them dumps a config.
        config_data: json!({
            "config_version": "v7.12.0",
            "live": {},
            "bot": { "long": { "entry_grid_spacing_pct": 0.06 } },
        }),
        created_at: NOW,
        updated_at: NOW,
    }
}

fn operator() -> Arc<dyn Authenticator> {
    Arc::new(LocalOperator::new(USER_ID.to_string()))
}

/// A principal with only `bots:read`.
struct ReadOnly;

impl Authenticator for ReadOnly {
    fn authenticate(&self) -> Option<Principal> {
        Some(Principal {
            user_id: USER_ID.to_string(),
            scopes: HashSet::from([SCOPE_READ.to_string()]),
            vip_level: 0,
        })
    }
}

/// A principal with both bot scopes and no `config:read`: what a signed-up
/// user holds.
struct BotsOnly;

impl Authenticator for BotsOnly {
    fn authenticate(&self) -> Option<Principal> {
        Some(Principal {
            user_id: USER_ID.to_string(),
            scopes: HashSet::from([SCOPE_READ.to_string(), SCOPE_WRITE.to_string()]),
            vip_level: 0,
        })
    }
}

/// A transport that could not identify its caller.
struct Anonymous;

impl Authenticator for Anonymous {
    fn authenticate(&self) -> Option<Principal> {
        None
    }
}

/// The JSON a tool answered with. Tool results carry text content, so the
/// payload is parsed back rather than asserted as a string.
fn payload(result: CallToolResult) -> Value {
    let text = result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("");
    serde_json::from_str(&text).expect("tool result is json")
}

#[tokio::test]
async fn list_bots_sees_only_the_callers_own_and_never_their_keys() {
    let h = harness!();
    h.given_bot(a_bot(&USER_ID.to_string(), BOT_ID)).await;
    h.given_bot(a_bot(OTHER_TENANT, "someone-elses")).await;

    let tools = h.mcp_tools(operator());
    let out = payload(tools.list_bots().await.expect("list_bots"));

    let bots = out["bots"].as_array().expect("bots array");
    assert_eq!(bots.len(), 1, "another tenant's bot leaked: {out}");
    assert_eq!(bots[0]["bot_id"], BOT_ID);
    assert!(
        !out.to_string().contains("DO-NOT-LEAK"),
        "exchange credentials must never appear in a tool result: {out}"
    );
}

#[tokio::test]
async fn a_read_only_principal_cannot_start_a_bot() {
    let h = harness!();
    h.given_bot(a_bot(&USER_ID.to_string(), BOT_ID)).await;
    h.configs.put(a_config(&USER_ID.to_string(), BOT_ID));

    let tools = h.mcp_tools(Arc::new(ReadOnly));
    let err = tools
        .start_bot(params(json!({ "bot_id": BOT_ID })))
        .await
        .expect_err("a read-only token must not launch a live task");

    assert!(
        err.message.contains(SCOPE_WRITE),
        "the refusal should name the missing scope: {}",
        err.message
    );
    assert!(
        h.ecs.launches().is_empty(),
        "nothing may launch on a refused call"
    );
}

#[tokio::test]
async fn an_unidentified_caller_is_refused() {
    let h = harness!();
    let tools = h.mcp_tools(Arc::new(Anonymous));

    let err = tools
        .list_bots()
        .await
        .expect_err("an unidentified caller has no tenant to read");
    assert!(err.message.contains("not authenticated"), "{}", err.message);
}

#[tokio::test]
async fn start_is_idempotent_because_it_claims_the_same_lock() {
    let h = harness!();
    h.given_bot(a_bot(&USER_ID.to_string(), BOT_ID)).await;
    h.configs.put(a_config(&USER_ID.to_string(), BOT_ID));
    let tools = h.mcp_tools(operator());

    let first = payload(
        tools
            .start_bot(params(json!({ "bot_id": BOT_ID })))
            .await
            .expect("first start"),
    );
    let second = payload(
        tools
            .start_bot(params(json!({ "bot_id": BOT_ID })))
            .await
            .expect("second start"),
    );

    assert_eq!(first["status"], "started");
    assert_eq!(second["status"], "already_starting");
    let launches = h.ecs.launches();
    assert_eq!(
        launches.len(),
        1,
        "a tool call must claim the same exclusive lock a button press does; two live \
         tasks for one bot is the failure this system must never have: {launches:?}"
    );
    assert_eq!(launches[0].td_arn, TD_V7);
}

#[tokio::test]
async fn stop_finds_the_task_the_start_launched() {
    let h = harness!();
    h.given_bot(a_bot(&USER_ID.to_string(), BOT_ID)).await;
    h.configs.put(a_config(&USER_ID.to_string(), BOT_ID));
    let tools = h.mcp_tools(operator());

    tools
        .start_bot(params(json!({ "bot_id": BOT_ID })))
        .await
        .expect("start");
    let out = payload(
        tools
            .stop_bot(params(json!({ "bot_id": BOT_ID })))
            .await
            .expect("stop"),
    );

    assert_eq!(out["status"], "stopped");
    assert_eq!(h.ecs.stops().len(), 1);
    assert_eq!(h.ecs.stops()[0].1, "task-1");
}

#[tokio::test]
async fn deleting_needs_the_confirmation_to_name_the_bot() {
    let h = harness!();
    h.given_bot(a_bot(&USER_ID.to_string(), BOT_ID)).await;
    let tools = h.mcp_tools(operator());

    let err = tools
        .delete_bot(params(
            json!({ "bot_id": BOT_ID, "confirm": "something else" }),
        ))
        .await
        .expect_err("a mismatched confirmation must not delete");
    assert!(err.message.contains("confirm"), "{}", err.message);

    let still_there = payload(tools.list_bots().await.expect("list"));
    assert_eq!(still_there["bots"].as_array().expect("bots").len(), 1);
}

#[tokio::test]
async fn an_unknown_runtime_is_rejected_before_any_write() {
    let h = harness!();
    h.given_bot(a_bot(&USER_ID.to_string(), BOT_ID)).await;
    let tools = h.mcp_tools(operator());

    let err = tools
        .set_bot_runtime(params(json!({ "bot_id": BOT_ID, "runtime": "wasm" })))
        .await
        .expect_err("only py and rs exist");
    assert!(err.message.contains("py"), "{}", err.message);
}

#[tokio::test]
async fn reading_a_config_takes_more_than_bots_read() {
    let h = harness!();
    h.given_bot(a_bot(&USER_ID.to_string(), BOT_ID)).await;
    h.configs.put(a_config(&USER_ID.to_string(), BOT_ID));

    let err = h
        .mcp_tools(Arc::new(BotsOnly))
        .get_bot_config(params(json!({ "bot_id": BOT_ID })))
        .await
        .expect_err("the parameters are the strategy; bots:read does not buy them");
    assert!(
        err.message.contains(SCOPE_CONFIG_READ),
        "the refusal should name the missing scope: {}",
        err.message
    );

    let out = payload(
        h.mcp_tools(operator())
            .get_bot_config(params(json!({ "bot_id": BOT_ID })))
            .await
            .expect("a principal holding config:read reads the config"),
    );
    assert_eq!(out["bot_id"], BOT_ID);
    assert!(
        out["config"]["bot"]["long"]["entry_grid_spacing_pct"].is_number(),
        "the whole config is what this tool is for: {out}"
    );
}

#[tokio::test]
async fn no_read_only_tool_hands_a_config_to_a_caller_without_config_read() {
    let h = harness!();
    h.given_bot(a_bot(&USER_ID.to_string(), BOT_ID)).await;
    h.configs.put(a_config(&USER_ID.to_string(), BOT_ID));
    let tools = h.mcp_tools(Arc::new(BotsOnly));

    let answers = vec![
        tools.list_bots().await.expect("list_bots"),
        tools
            .get_bot_status(params(json!({ "bot_id": BOT_ID })))
            .await
            .expect("get_bot_status"),
        tools.list_templates().await.expect("list_templates"),
    ];

    for answer in answers {
        let text = payload(answer).to_string();
        assert!(
            !text.contains("\"bot\"") && !text.contains("entry_grid_spacing"),
            "a tool outside config:read leaked strategy parameters: {text}"
        );
    }
}

// --- The registry's own boundaries -----------------------------------------

/// Every tool's declared input schema.
fn tool_schemas() -> Vec<(String, Value)> {
    BotTools::tool_router()
        .list_all()
        .into_iter()
        .map(|t| {
            (
                t.name.to_string(),
                serde_json::to_value(&t.input_schema).expect("schema is json"),
            )
        })
        .collect()
}

#[tokio::test]
async fn no_tool_accepts_a_user_id() {
    for (name, schema) in tool_schemas() {
        assert!(
            !schema.to_string().contains("user_id"),
            "{name} takes a user_id; the tenant must come from the authenticated \
             principal, or a caller can name someone else's bots"
        );
    }
}

#[tokio::test]
async fn no_tool_accepts_exchange_credentials() {
    for (name, schema) in tool_schemas() {
        let text = schema.to_string();
        for forbidden in ["api_key", "secret_key", "apiKey", "secretKey"] {
            assert!(
                !text.contains(forbidden),
                "{name} takes {forbidden}; a tool argument lands in the model's context \
                 and in the transcript, so key entry stays on the Telegram path"
            );
        }
    }
}

#[tokio::test]
async fn the_registry_exposes_no_whole_config_overwrite() {
    let names: Vec<String> = tool_schemas().into_iter().map(|(n, _)| n).collect();
    assert!(
        !names.iter().any(|n| n == "update_bot_config"),
        "exposing a whole-config write lets a model rewrite a live bot's position \
         parameters in one call; only named fields are exposed: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n == "add_bot"),
        "add_bot would have to take exchange credentials to be useful: {names:?}"
    );
    // The surface that should exist, so a rename or a dropped `#[tool]` is loud.
    for expected in [
        "list_bots",
        "get_bot_status",
        "get_bot_config",
        "list_templates",
        "start_bot",
        "stop_bot",
        "apply_template",
        "set_risk_level",
        "set_strategy_side",
        "set_bot_runtime",
        "delete_bot",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "{expected} is missing from the tool registry: {names:?}"
        );
    }
}

/// Build a tool's typed parameters from the JSON a client would send, so the
/// test exercises the same deserialization a real call does.
fn params<T: serde::de::DeserializeOwned>(
    value: Value,
) -> rmcp::handler::server::wrapper::Parameters<T> {
    rmcp::handler::server::wrapper::Parameters(
        serde_json::from_value(value).expect("valid tool arguments"),
    )
}
