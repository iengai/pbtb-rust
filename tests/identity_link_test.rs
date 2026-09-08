//! The link flow's conditional writes, against a real DynamoDB.
//!
//! Every assertion here is about a condition expression, which an in-memory
//! stand-in cannot check: it would accept an expression DynamoDB rejects, and a
//! silent `ValidationException` in production is exactly what this suite exists
//! to stop. Two of them are also the flow's security properties — a ticket is
//! redeemable once, and an identity names one tenant.

mod common;

use pbtb_rust::domain::bot::{Bot, BotRepository};
use pbtb_rust::domain::engine::Runtime;
use pbtb_rust::domain::exchange::Exchange;
use pbtb_rust::domain::identity::{
    IdentityRepository, LinkOutcome, LinkTicket, LinkTicketRepository, LinkedIdentity,
};
use pbtb_rust::infra::botrepository::DynamoBotRepository;

const NOW: i64 = 1_700_000_000;
const PROVIDER: &str = "workos";
const START: &str = "start";

fn a_ticket(user_id: &str) -> LinkTicket {
    LinkTicket {
        user_id: user_id.to_string(),
        chat_id: 4242,
        code_verifier: None,
    }
}

fn an_identity(user_id: &str) -> LinkedIdentity {
    LinkedIdentity {
        user_id: user_id.to_string(),
        email: Some("someone@example.com".to_string()),
        linked_at: NOW,
    }
}

macro_rules! repo {
    () => {
        match common::dynamo::start().await {
            Some(db) => DynamoBotRepository::new(db.client.clone(), db.table.clone()),
            None => return,
        }
    };
}

#[tokio::test]
async fn a_ticket_is_redeemable_exactly_once() {
    let repo = repo!();

    repo.issue(START, "hash-1", &a_ticket("u-1"), NOW, NOW + 600)
        .await
        .expect("issue");

    let first = repo.redeem(START, "hash-1", NOW).await.expect("redeem");
    assert_eq!(first.as_ref().map(|t| t.user_id.as_str()), Some("u-1"));
    assert_eq!(first.expect("a ticket").chat_id, 4242);

    // The second arrival is what a replayed link URL looks like — from a browser
    // history, a proxy log, a forwarded message.
    assert!(
        repo.redeem(START, "hash-1", NOW)
            .await
            .expect("redeem")
            .is_none(),
        "a redeemed ticket must not be redeemable again"
    );
}

#[tokio::test]
async fn an_expired_ticket_is_not_redeemable() {
    let repo = repo!();

    repo.issue(START, "hash-2", &a_ticket("u-1"), NOW, NOW + 600)
        .await
        .expect("issue");

    // TTL deletion lags by hours, so the row is very likely still there. The
    // expiry has to be enforced by the read, not by the row's absence.
    assert!(
        repo.redeem(START, "hash-2", NOW + 601)
            .await
            .expect("redeem")
            .is_none()
    );
}

#[tokio::test]
async fn a_ticket_cannot_be_redeemed_under_another_purpose() {
    let repo = repo!();

    repo.issue(START, "hash-3", &a_ticket("u-1"), NOW, NOW + 600)
        .await
        .expect("issue");

    assert!(
        repo.redeem("callback", "hash-3", NOW)
            .await
            .expect("redeem")
            .is_none(),
        "the browser's first ticket is not an authorization it was never part of"
    );
}

#[tokio::test]
async fn the_pkce_verifier_survives_the_redirect() {
    let repo = repo!();

    let ticket = LinkTicket {
        code_verifier: Some("the-verifier".to_string()),
        ..a_ticket("u-1")
    };
    repo.issue("callback", "hash-4", &ticket, NOW, NOW + 600)
        .await
        .expect("issue");

    let redeemed = repo
        .redeem("callback", "hash-4", NOW)
        .await
        .expect("redeem")
        .expect("a ticket");
    assert_eq!(redeemed.code_verifier.as_deref(), Some("the-verifier"));
}

#[tokio::test]
async fn an_identity_can_only_be_claimed_by_one_tenant() {
    let repo = repo!();

    assert_eq!(
        repo.link(PROVIDER, "sub-1", &an_identity("u-1"))
            .await
            .expect("link"),
        LinkOutcome::Linked
    );

    // The takeover: someone authenticates as the same provider identity and asks
    // for it to point at their own tenant instead.
    assert_eq!(
        repo.link(PROVIDER, "sub-1", &an_identity("u-2"))
            .await
            .expect("link"),
        LinkOutcome::ClaimedByAnother
    );

    let link = repo
        .find_link(PROVIDER, "sub-1")
        .await
        .expect("find")
        .expect("still linked");
    assert_eq!(link.user_id, "u-1", "the first tenant keeps the identity");
}

#[tokio::test]
async fn relinking_the_same_identity_is_not_a_failure() {
    let repo = repo!();

    repo.link(PROVIDER, "sub-2", &an_identity("u-1"))
        .await
        .expect("link");

    // How someone recovers from a flow that died after the write.
    assert_eq!(
        repo.link(PROVIDER, "sub-2", &an_identity("u-1"))
            .await
            .expect("link"),
        LinkOutcome::AlreadyLinked
    );
}

#[tokio::test]
async fn one_tenant_may_hold_several_identities() {
    let repo = repo!();

    for subject in ["sub-3", "sub-4"] {
        assert_eq!(
            repo.link(PROVIDER, subject, &an_identity("u-1"))
                .await
                .expect("link"),
            LinkOutcome::Linked,
            "the asymmetry runs one way only"
        );
    }

    assert_eq!(
        repo.find_link(PROVIDER, "sub-3")
            .await
            .expect("find")
            .expect("linked")
            .user_id,
        "u-1"
    );
    assert_eq!(
        repo.find_link(PROVIDER, "sub-4")
            .await
            .expect("find")
            .expect("linked")
            .user_id,
        "u-1"
    );
}

/// The rows this flow writes share a table with the bots, and two readers walk
/// that table without a sort-key condition. A row shape they do not recognise is
/// not skipped — it is parsed as a bot, fails, and takes the whole read with it.
/// These rows have no TTL, so that failure would be permanent.
#[tokio::test]
async fn a_link_does_not_break_the_tenants_bot_list() {
    let repo = repo!();

    let bot = Bot::new(
        "alpha".to_string(),
        "u-list".to_string(),
        Exchange::Bybit,
        "alpha".to_string(),
        "ak".to_string(),
        "sk".to_string(),
        false,
        Runtime::Py,
        NOW,
        NOW,
    );
    repo.save(&bot).await.expect("save");
    repo.link(PROVIDER, "sub-list", &an_identity("u-list"))
        .await
        .expect("link");

    let bots = repo
        .find_by_user_id("u-list")
        .await
        .expect("the tenant's own linked identity must not corrupt their bot list");
    assert_eq!(bots.len(), 1);
    assert_eq!(bots[0].id, "alpha");
}

#[tokio::test]
async fn link_rows_do_not_break_the_table_wide_scan() {
    let repo = repo!();

    let bot = Bot::new(
        "beta".to_string(),
        "u-scan".to_string(),
        Exchange::Bybit,
        "beta".to_string(),
        "ak".to_string(),
        "sk".to_string(),
        false,
        Runtime::Py,
        NOW,
        NOW,
    );
    repo.save(&bot).await.expect("save");

    // A ticket exists from the moment anyone taps the button, before any browser
    // is involved — so this is the first thing the scan meets, not the last.
    repo.issue(START, "hash-scan", &a_ticket("u-scan"), NOW, NOW + 600)
        .await
        .expect("issue");
    repo.link(PROVIDER, "sub-scan", &an_identity("u-scan"))
        .await
        .expect("link");

    let all = repo
        .find_all()
        .await
        .expect("a link must not break the daily snapshot for every tenant");
    assert!(all.iter().any(|b| b.id == "beta"));
}
