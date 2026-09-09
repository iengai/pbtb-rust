//! End-to-end tests: a Telegram update in, a reply and a DynamoDB row out.
//!
//! These cover the paths no unit test reaches — the router's branch order, the
//! dialogue's keyboard vocabulary, and the launch path's interaction with the
//! real start lock.

mod common;

use common::telegram::{STRANGER_ID, USER_ID, callback, group_message, senderless, text_message};
use common::{CONTAINER_NAME, Harness, NOW, SITE_URL, TD_V7};
use pbtb_rust::domain::IdentityRepository;
use pbtb_rust::domain::bot::Bot;
use pbtb_rust::domain::botconfig::{BotConfig, BotType};
use pbtb_rust::domain::configtemplate::ConfigTemplate;
use pbtb_rust::domain::engine::Runtime;
use pbtb_rust::domain::exchange::Exchange;
use pbtb_rust::domain::identity::PROVIDER_TELEGRAM;
use pbtb_rust::domain::runtime::{BotRuntimeRepository, RuntimePhase};
use serde_json::json;

const BOT_ID: &str = "alpha";

fn a_bot() -> Bot {
    named_bot(BOT_ID)
}

fn named_bot(id: &str) -> Bot {
    Bot::new(
        id.to_string(),
        USER_ID.to_string(),
        Exchange::Bybit,
        id.to_string(),
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
    config_of(BOT_ID)
}

fn config_of(bot_id: &str) -> BotConfig {
    BotConfig {
        user_id: USER_ID.to_string(),
        bot_id: bot_id.to_string(),
        bot_type: BotType::default(),
        template_name: "test".to_string(),
        template_version: None,
        config_data: json!({ "config_version": "v7.12.0", "live": {} }),
        created_at: NOW,
        updated_at: NOW,
    }
}

/// A v7 template that asks for `min_vip_level`.
fn a_template(name: &str, min_vip_level: u8) -> ConfigTemplate {
    ConfigTemplate {
        name: name.to_string(),
        description: None,
        version: None,
        config_data: json!({
            "config_version": "v7.12.0",
            "pbtb": { "min_vip_level": min_vip_level },
            "live": {},
        }),
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
async fn stranger_is_pointed_at_the_web_and_learns_nothing() {
    let h = harness!();
    h.given_bot(a_bot()).await;

    let handled = h.send(text_message(STRANGER_ID, "/list")).await;

    assert!(
        handled,
        "the gate must claim the update, not let it fall through"
    );
    let wire = h.telegram.wire().await;
    assert!(
        h.transcript().await.contains(SITE_URL),
        "a stranger should be told where to sign up, got: {wire}"
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
        "a senderless update resolves to nobody and must stop at the gate"
    );
    assert!(
        !h.transcript().await.contains("Your bots"),
        "no tenant may be reached without a sender"
    );
}

#[tokio::test]
async fn a_suspended_account_is_turned_away() {
    let h = harness!();
    h.given_bot(a_bot()).await;
    h.given_suspended(&USER_ID.to_string()).await;

    assert!(h.send(text_message(USER_ID, "/list")).await);

    let wire = h.telegram.wire().await;
    assert!(h.transcript().await.contains("suspended"), "got: {wire}");
    assert!(!wire.contains(BOT_ID), "got: {wire}");
}

#[tokio::test]
async fn a_bind_link_from_the_web_binds_the_sender_who_opens_it() {
    let h = harness!();
    h.given_account("acct-new", 0).await;
    let token = h.given_bind_ticket("acct-new").await;

    assert!(
        h.send(text_message(STRANGER_ID, &format!("/start {token}")))
            .await
    );
    assert!(
        h.transcript().await.contains("bound"),
        "got: {}",
        h.transcript().await
    );

    // Bound to the ticket's account, which nothing in the message named.
    let identities: &dyn IdentityRepository = h.bots.as_ref();
    let link = identities
        .find_link(PROVIDER_TELEGRAM, &STRANGER_ID.to_string())
        .await
        .expect("find")
        .expect("the sender is now bound");
    assert_eq!(link.user_id, "acct-new");

    // And the next update from that sender acts as that account.
    h.given_bot(Bot::new(
        "theirs".to_string(),
        "acct-new".to_string(),
        Exchange::Bybit,
        "theirs".to_string(),
        "ak".to_string(),
        "sk".to_string(),
        false,
        Runtime::Py,
        NOW,
        NOW,
    ))
    .await;
    assert!(h.send(text_message(STRANGER_ID, "/list")).await);
    assert!(h.telegram.wire().await.contains("theirs"));

    // The link is spent.
    assert!(
        h.send(text_message(999_000_111, &format!("/start {token}")))
            .await
    );
    assert!(
        h.transcript()
            .await
            .contains("invalid, expired or already used"),
        "got: {}",
        h.transcript().await
    );
}

#[tokio::test]
async fn a_bind_link_refuses_a_group_and_a_telegram_id_already_bound_elsewhere() {
    let h = harness!();
    h.given_account("acct-new", 0).await;

    let token = h.given_bind_ticket("acct-new").await;
    assert!(
        h.send(group_message(STRANGER_ID, &format!("/start {token}")))
            .await
    );
    assert!(h.transcript().await.contains("private chat"));

    // The operator's Telegram id belongs to the operator's account; a ticket
    // for another account cannot take it.
    let token = h.given_bind_ticket("acct-new").await;
    assert!(
        h.send(text_message(USER_ID, &format!("/start {token}")))
            .await
    );
    assert!(
        h.transcript()
            .await
            .contains("bound to a different account"),
        "got: {}",
        h.transcript().await
    );
    let identities: &dyn IdentityRepository = h.bots.as_ref();
    assert_eq!(
        identities
            .find_link(PROVIDER_TELEGRAM, &USER_ID.to_string())
            .await
            .expect("find")
            .expect("still bound")
            .user_id,
        USER_ID.to_string()
    );
}

#[tokio::test]
async fn an_account_speaks_through_one_telegram_id_until_it_unbinds() {
    let h = harness!();

    // The operator already has a Telegram id; a second cannot be added.
    let token = h.given_bind_ticket(&USER_ID.to_string()).await;
    assert!(
        h.send(text_message(STRANGER_ID, &format!("/start {token}")))
            .await
    );
    assert!(
        h.transcript()
            .await
            .contains("already has a Telegram account bound"),
        "got: {}",
        h.transcript().await
    );

    // Unbinding from the bot releases it; the sender is a stranger afterwards.
    assert!(h.send(text_message(USER_ID, "/unlink")).await);
    assert!(h.transcript().await.contains("Unbound"));
    assert!(h.send(text_message(USER_ID, "/list")).await);
    assert!(h.transcript().await.contains(SITE_URL));

    // And a fresh ticket binds the new id.
    let token = h.given_bind_ticket(&USER_ID.to_string()).await;
    assert!(
        h.send(text_message(STRANGER_ID, &format!("/start {token}")))
            .await
    );
    let identities: &dyn IdentityRepository = h.bots.as_ref();
    assert_eq!(
        identities
            .find_link(PROVIDER_TELEGRAM, &STRANGER_ID.to_string())
            .await
            .expect("find")
            .expect("bound")
            .user_id,
        USER_ID.to_string()
    );
}

#[tokio::test]
async fn a_bound_user_sees_their_own_bots() {
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

// The harness seeds the operator at VIP 0: one running bot, open templates only.

#[tokio::test]
async fn a_second_bot_is_refused_at_level_zero_until_the_first_is_stopped() {
    let h = harness!();
    for id in [BOT_ID, "beta"] {
        h.given_bot(named_bot(id)).await;
        h.configs.put(config_of(id));
    }

    assert!(
        h.send(callback(USER_ID, &format!("select_bot:{BOT_ID}")))
            .await
    );
    assert!(h.send(text_message(USER_ID, "Run bot")).await);
    assert!(h.send(callback(USER_ID, "select_bot:beta")).await);
    assert!(h.send(text_message(USER_ID, "Run bot")).await);

    let transcript = h.transcript().await;
    assert!(
        transcript.contains("stop one first"),
        "the refusal says what to do: {transcript}"
    );
    assert_eq!(
        h.ecs.launches().len(),
        1,
        "the ceiling holds before ECS is reached"
    );

    assert!(
        h.send(callback(USER_ID, &format!("select_bot:{BOT_ID}")))
            .await
    );
    assert!(h.send(text_message(USER_ID, "Stop bot")).await);
    assert!(h.send(callback(USER_ID, "select_bot:beta")).await);
    assert!(h.send(text_message(USER_ID, "Run bot")).await);
    assert_eq!(h.ecs.launches().len(), 2, "the freed slot is usable");
}

#[tokio::test]
async fn a_locked_template_is_marked_in_the_list_and_refused_on_tap() {
    let h = harness!();
    h.given_bot(a_bot()).await;
    h.templates.add(a_template("open", 0));
    h.templates.add(a_template("gated", 3));

    assert!(
        h.send(callback(USER_ID, &format!("select_bot:{BOT_ID}")))
            .await
    );
    assert!(h.send(text_message(USER_ID, "Choose config...")).await);

    let wire = h.telegram.wire().await;
    assert!(
        wire.contains("📄 open") && wire.contains("🔒 gated (VIP 3)"),
        "both are listed, one locked: {wire}"
    );

    assert!(h.send(callback(USER_ID, "select_template:gated")).await);
    let transcript = h.transcript().await;
    assert!(
        transcript.contains("needs VIP 3"),
        "the tap says which level unlocks it: {transcript}"
    );
    assert!(
        h.configs.get_saved(&USER_ID.to_string(), BOT_ID).is_none(),
        "nothing was applied"
    );
}
