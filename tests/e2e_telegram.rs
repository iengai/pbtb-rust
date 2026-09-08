//! End-to-end tests: a Telegram update in, a reply and a DynamoDB row out.
//!
//! These cover the paths no unit test reaches — the router's branch order, the
//! dialogue's keyboard vocabulary, and the launch path's interaction with the
//! real start lock.

mod common;

use common::telegram::{STRANGER_ID, USER_ID, callback, senderless, text_message};
use common::{CONTAINER_NAME, Harness, NOW, TD_V7};
use pbtb_rust::domain::bot::Bot;
use pbtb_rust::domain::botconfig::{BotConfig, BotType};
use pbtb_rust::domain::engine::Runtime;
use pbtb_rust::domain::exchange::Exchange;
use pbtb_rust::domain::runtime::{BotRuntimeRepository, RuntimePhase};
use serde_json::json;

const BOT_ID: &str = "alpha";

fn a_bot() -> Bot {
    Bot::new(
        BOT_ID.to_string(),
        USER_ID.to_string(),
        Exchange::Bybit,
        BOT_ID.to_string(),
        "KEY-DO-NOT-LEAK".to_string(),
        "SECRET-DO-NOT-LEAK".to_string(),
        false,
        Runtime::Py,
        NOW,
        NOW,
    )
}

/// A v7 config, which routes the launch to the v7 task definition.
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

/// Skips (green) when Docker is unavailable; CI asserts the daemon is up.
macro_rules! harness {
    () => {
        match Harness::start().await {
            Some(h) => h,
            None => return,
        }
    };
}

#[tokio::test]
async fn stranger_is_refused_and_learns_nothing() {
    let h = harness!();
    h.given_bot(a_bot()).await;

    let handled = h.send(text_message(STRANGER_ID, "/list")).await;

    assert!(
        handled,
        "the guard must claim the update, not let it fall through"
    );
    let wire = h.telegram.wire().await;
    assert!(
        h.transcript().await.contains("authorized"),
        "stranger should get a refusal, got: {wire}"
    );
    assert!(
        !wire.contains(BOT_ID),
        "a refusal must not disclose the bots, got: {wire}"
    );
}

#[tokio::test]
async fn update_without_a_sender_is_refused() {
    let h = harness!();

    let handled = h.send(senderless()).await;

    assert!(
        handled,
        "a senderless update matches no allowlist entry and must stop at the guard"
    );
    assert!(
        !h.transcript().await.contains("Your bots"),
        "the 'unknown' tenant bucket must stay unreachable"
    );
}

#[tokio::test]
async fn allowlisted_user_sees_their_own_bots() {
    let h = harness!();
    h.given_bot(a_bot()).await;

    assert!(h.send(text_message(USER_ID, "/list")).await);

    // The list is rendered as inline buttons, so the bot name is in the
    // keyboard rather than in the message text.
    let wire = h.telegram.wire().await;
    assert!(
        wire.contains(BOT_ID),
        "the bot should be listed, got: {wire}"
    );
    assert!(
        !wire.contains("DO-NOT-LEAK"),
        "exchange credentials must never reach a reply, got: {wire}"
    );
}

#[tokio::test]
async fn run_claims_the_lock_and_launches_exactly_one_task() {
    let h = harness!();
    h.given_bot(a_bot()).await;
    h.configs.put(a_config());

    assert!(
        h.send(callback(USER_ID, &format!("select_bot:{BOT_ID}")))
            .await
    );
    assert!(h.send(text_message(USER_ID, "Run bot")).await);

    let launches = h.ecs.launches();
    assert_eq!(launches.len(), 1, "exactly one RunTask: {launches:?}");
    assert_eq!(launches[0].bot_id, BOT_ID);
    assert_eq!(
        launches[0].td_arn, TD_V7,
        "a v7 config must launch on the v7 task definition"
    );
    assert_eq!(launches[0].container_name, CONTAINER_NAME);

    let runtime = h
        .bots
        .find_consistent(&USER_ID.to_string(), BOT_ID)
        .await
        .expect("read runtime")
        .expect("a runtime row after a launch");
    assert_eq!(runtime.phase, RuntimePhase::Starting);
    assert_eq!(runtime.task_id.as_deref(), Some("task-1"));
}

#[tokio::test]
async fn a_second_run_does_not_launch_a_second_task() {
    let h = harness!();
    h.given_bot(a_bot()).await;
    h.configs.put(a_config());

    assert!(
        h.send(callback(USER_ID, &format!("select_bot:{BOT_ID}")))
            .await
    );
    assert!(h.send(text_message(USER_ID, "Run bot")).await);
    assert!(h.send(text_message(USER_ID, "Run bot")).await);

    assert_eq!(
        h.ecs.launches().len(),
        1,
        "the held start lock must reject the second launch; two live tasks for one bot is the \
         failure this system must never have"
    );
}

#[tokio::test]
async fn stop_issues_a_stoptask_for_the_launched_task() {
    let h = harness!();
    h.given_bot(a_bot()).await;
    h.configs.put(a_config());

    assert!(
        h.send(callback(USER_ID, &format!("select_bot:{BOT_ID}")))
            .await
    );
    assert!(h.send(text_message(USER_ID, "Run bot")).await);
    assert!(h.send(text_message(USER_ID, "Stop bot")).await);

    let stops = h.ecs.stops();
    assert_eq!(stops.len(), 1, "exactly one StopTask: {stops:?}");
    assert_eq!(stops[0].1, "task-1", "stopping the task that was launched");
}
