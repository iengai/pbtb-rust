//! The keyboard dialogue, end to end.
//!
//! `dialogue.rs` is the largest file in the interface layer and had no tests: it
//! is a hand-rolled state machine over button labels, where a typo in a match arm
//! is a silently dead button. These push the real updates through the real
//! router and assert on what the chat is told and what lands in the stores.

mod common;

use common::telegram::{USER_ID, callback, group_message, text_message};
use common::{Harness, NOW};
use pbtb_rust::domain::IdentityRepository;
use pbtb_rust::domain::bot::Bot;
use pbtb_rust::domain::botconfig::{BotConfig, BotType};
use pbtb_rust::domain::configtemplate::ConfigTemplate;
use pbtb_rust::domain::engine::Runtime;
use pbtb_rust::domain::exchange::Exchange;
use pbtb_rust::domain::identity::LinkTicketRepository;
use pbtb_rust::domain::secret::token_digest;
use serde_json::json;

const BOT_ID: &str = "alpha";

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

/// A v7 config: exposure lives under the flat `bot.<side>` key.
fn a_config() -> BotConfig {
    BotConfig {
        user_id: USER_ID.to_string(),
        bot_id: BOT_ID.to_string(),
        bot_type: BotType::default(),
        template_name: "test".to_string(),
        template_version: None,
        config_data: json!({
            "config_version": "v7.12.0",
            "live": { "user": BOT_ID },
            "bot": {
                "long": { "total_wallet_exposure_limit": 1.0, "n_positions": 2.0 },
                "short": { "total_wallet_exposure_limit": 0.5, "n_positions": 1.0 },
            },
        }),
        created_at: NOW,
        updated_at: NOW,
    }
}

/// Pick the bot, so the per-bot buttons have a subject.
async fn select_bot(h: &Harness) {
    assert!(
        h.send(callback(USER_ID, &format!("select_bot:{BOT_ID}")))
            .await
    );
}

#[tokio::test]
async fn a_per_bot_button_with_no_bot_selected_says_so() {
    let h = harness!();
    h.given_bot(a_bot()).await;

    // Every per-bot button shares this guard; "Run bot" is the one where acting
    // without a subject would launch a live task.
    assert!(h.send(text_message(USER_ID, "Run bot")).await);

    // Asserted on the button the reply points at, not on the sentence: the
    // per-bot guards are duplicated across handlers with six different wordings,
    // and pinning one of them here would make a copy edit elsewhere fail this.
    let transcript = h.transcript().await;
    assert!(
        transcript.contains("List"),
        "the reply should point at the button that fixes it: {transcript}"
    );
    assert!(
        h.ecs.launches().is_empty(),
        "nothing may launch without a selected bot"
    );
}

#[tokio::test]
async fn the_risk_prompt_shows_what_is_configured_now() {
    let h = harness!();
    h.given_bot(a_bot()).await;
    h.configs.put(a_config());
    select_bot(&h).await;

    assert!(h.send(text_message(USER_ID, "Risk level")).await);

    let transcript = h.transcript().await;
    assert!(
        transcript.contains("Long: 1.00, Short: 0.50"),
        "the prompt should show the current exposure: {transcript}"
    );
}

#[tokio::test]
async fn entering_a_risk_level_writes_it_and_derives_leverage() {
    let h = harness!();
    h.given_bot(a_bot()).await;
    h.configs.put(a_config());
    select_bot(&h).await;

    assert!(h.send(text_message(USER_ID, "Risk level")).await);
    assert!(h.send(text_message(USER_ID, "3.0/1.5")).await);

    let saved = h
        .configs
        .get_saved(&USER_ID.to_string(), BOT_ID)
        .expect("config");
    let risk = saved.risk_level().expect("risk level");
    assert_eq!(risk.long, 3.0);
    assert_eq!(risk.short, 1.5);
    // Leverage is the domain's to derive from the exposure, not the dialogue's.
    let leverage = saved.leverage().expect("leverage");
    assert_eq!(leverage.long, 4.0, "max(3.0, 1.5) + 1");
}

#[tokio::test]
async fn a_risk_level_in_the_wrong_shape_changes_nothing() {
    let h = harness!();
    h.given_bot(a_bot()).await;
    h.configs.put(a_config());
    select_bot(&h).await;

    assert!(h.send(text_message(USER_ID, "Risk level")).await);
    assert!(h.send(text_message(USER_ID, "three")).await);

    assert!(
        h.transcript().await.contains("Invalid format"),
        "the user should be told what shape to use"
    );
    let saved = h
        .configs
        .get_saved(&USER_ID.to_string(), BOT_ID)
        .expect("config");
    assert_eq!(saved.risk_level().expect("risk").long, 1.0, "unchanged");
}

#[tokio::test]
async fn cancelling_the_risk_prompt_leaves_the_config_alone() {
    let h = harness!();
    h.given_bot(a_bot()).await;
    h.configs.put(a_config());
    select_bot(&h).await;

    assert!(h.send(text_message(USER_ID, "Risk level")).await);
    assert!(h.send(text_message(USER_ID, "cancel")).await);

    assert!(h.transcript().await.contains("cancelled"));
    let saved = h
        .configs
        .get_saved(&USER_ID.to_string(), BOT_ID)
        .expect("config");
    assert_eq!(saved.risk_level().expect("risk").long, 1.0);
}

#[tokio::test]
async fn asking_for_risk_on_a_bot_with_no_config_points_at_the_template_button() {
    let h = harness!();
    h.given_bot(a_bot()).await;
    select_bot(&h).await;

    assert!(h.send(text_message(USER_ID, "Risk level")).await);

    let transcript = h.transcript().await;
    assert!(
        transcript.contains("No configuration found"),
        "got: {transcript}"
    );
    assert!(
        transcript.contains("Choose config"),
        "the reply should name the button that fixes it: {transcript}"
    );
}

#[tokio::test]
async fn the_sides_panel_reflects_the_config_and_a_tap_flips_one() {
    let h = harness!();
    h.given_bot(a_bot()).await;
    h.configs.put(a_config());
    select_bot(&h).await;

    assert!(h.send(text_message(USER_ID, "Sides")).await);
    let before = h.telegram.wire().await;
    assert!(
        before.contains("Long: \u{1f7e2} ON"),
        "long has exposure, so it starts on: {before}"
    );

    assert!(h.send(callback(USER_ID, "toggle_side:long")).await);

    let saved = h
        .configs
        .get_saved(&USER_ID.to_string(), BOT_ID)
        .expect("config");
    assert!(
        !saved.side_enabled("long"),
        "tapping an ON side must turn it off"
    );
    assert!(
        saved.side_enabled("short"),
        "the other side is untouched by the tap"
    );
}

#[tokio::test]
async fn choosing_a_template_previews_it_before_applying() {
    let h = harness!();
    h.given_bot(a_bot()).await;
    h.templates.add(ConfigTemplate {
        name: "steady".to_string(),
        description: Some("a steady preset".to_string()),
        config_data: json!({
            "config_version": "v7.12.0",
            "live": {},
            "bot": {
                "long": { "total_wallet_exposure_limit": 2.0 },
                "short": { "total_wallet_exposure_limit": 0.25 },
            },
        }),
        version: Some("3".to_string()),
    });
    select_bot(&h).await;

    assert!(h.send(text_message(USER_ID, "Choose config...")).await);
    assert!(
        h.telegram.wire().await.contains("steady"),
        "the template should be offered"
    );

    assert!(h.send(callback(USER_ID, "select_template:steady")).await);
    let preview = h.transcript().await;
    assert!(preview.contains("Apply this config?"), "got: {preview}");
    assert!(
        preview.contains("2.00"),
        "the preview should show the exposure being accepted: {preview}"
    );
    assert!(
        h.configs.get_saved(&USER_ID.to_string(), BOT_ID).is_none(),
        "a preview must not have applied anything yet"
    );

    assert!(h.send(callback(USER_ID, "confirm_template:steady")).await);
    let saved = h
        .configs
        .get_saved(&USER_ID.to_string(), BOT_ID)
        .expect("the confirmation applies it");
    assert_eq!(saved.template_name, "steady");
    assert_eq!(
        saved.config_data["live"]["user"], BOT_ID,
        "the applied config must name the bot, which is how passivbot finds its keys"
    );
}

#[tokio::test]
async fn the_runtime_command_moves_a_bot_between_images() {
    let h = harness!();
    h.given_bot(a_bot()).await;
    h.configs.put(a_config());

    assert!(
        h.send(text_message(USER_ID, &format!("/runtime {BOT_ID} rs")))
            .await
    );

    let transcript = h.transcript().await;
    assert!(
        transcript.contains("rs"),
        "the reply should name the new runtime: {transcript}"
    );
}

#[tokio::test]
async fn the_link_button_hands_out_a_single_use_url_bound_to_the_caller() {
    let h = harness!();

    assert!(h.send(text_message(USER_ID, "Link account")).await);

    // The URL lives in an inline button, not in the message text.
    let wire = h.telegram.wire().await;
    let token = wire
        .split("?t=")
        .nth(1)
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_hexdigit()).next())
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("no link token in: {wire}"));
    assert_eq!(token.len(), 64, "32 bytes of entropy, hex encoded");

    // Redeemed under the caller's own id, which the browser never gets to name.
    let tickets: &dyn LinkTicketRepository = h.bots.as_ref();
    let ticket = tickets
        .redeem("start", &token_digest(&token), NOW)
        .await
        .expect("redeem")
        .expect("the button issued a ticket");
    assert_eq!(ticket.user_id, USER_ID.to_string());

    assert!(
        tickets
            .redeem("start", &token_digest(&token), NOW)
            .await
            .expect("redeem")
            .is_none(),
        "and it is spent"
    );
}

#[tokio::test]
async fn the_link_button_refuses_to_post_a_personal_url_into_a_group() {
    let h = harness!();

    assert!(h.send(group_message(USER_ID, "Link account")).await);

    // An inline button renders for everyone in the chat, and the URL behind it
    // is a bearer credential for one account: the first member to tap it would
    // link their own identity to the sender's tenant.
    let wire = h.telegram.wire().await;
    assert!(
        !wire.contains("?t="),
        "no link token may reach a group: {wire}"
    );
    assert!(
        wire.contains("private"),
        "and the reply should say where to ask instead: {wire}"
    );
}

#[tokio::test]
async fn unlink_releases_the_callers_own_links_and_nobody_elses() {
    let h = harness!();
    h.given_link("workos", "sub-mine", &USER_ID.to_string())
        .await;
    h.given_link("workos", "sub-theirs", "999888777").await;

    assert!(h.send(text_message(USER_ID, "/unlink")).await);

    assert!(
        h.transcript().await.contains("Released 1"),
        "got: {}",
        h.transcript().await
    );
    let identities: &dyn IdentityRepository = h.bots.as_ref();
    assert!(
        identities
            .find_link("workos", "sub-mine")
            .await
            .expect("find")
            .is_none()
    );
    assert!(
        identities
            .find_link("workos", "sub-theirs")
            .await
            .expect("find")
            .is_some(),
        "another tenant's link is not the caller's to release"
    );
}
