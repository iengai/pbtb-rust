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
    PROVIDER_TELEGRAM,
};
use pbtb_rust::domain::user::{Role, User, UserRepository, UserStatus};
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

#[tokio::test]
async fn releasing_a_link_lets_its_owner_claim_the_identity() {
    let repo = repo!();

    repo.link(PROVIDER, "sub-release", &an_identity("u-wrong"))
        .await
        .expect("link");
    // The way back from a link that went to the wrong place. Without it the
    // rightful owner is refused forever.
    assert!(
        repo.unlink(PROVIDER, "sub-release", "u-wrong")
            .await
            .expect("unlink")
    );

    assert!(
        repo.find_link(PROVIDER, "sub-release")
            .await
            .expect("find")
            .is_none()
    );
    assert_eq!(
        repo.link(PROVIDER, "sub-release", &an_identity("u-right"))
            .await
            .expect("link"),
        LinkOutcome::Linked
    );
}

#[tokio::test]
async fn a_tenant_can_only_release_their_own_links() {
    let repo = repo!();

    repo.link(PROVIDER, "sub-mine", &an_identity("u-owner"))
        .await
        .expect("link");

    // Otherwise releasing would be a way to take an identity off someone, and
    // then claim it.
    assert!(
        !repo
            .unlink(PROVIDER, "sub-mine", "u-thief")
            .await
            .expect("unlink"),
        "a delete on someone else's link must fail, not succeed quietly"
    );
    assert_eq!(
        repo.find_link(PROVIDER, "sub-mine")
            .await
            .expect("find")
            .expect("still linked")
            .user_id,
        "u-owner"
    );
}

#[tokio::test]
async fn a_tenants_links_are_listed_from_their_own_partition() {
    let repo = repo!();

    for subject in ["sub-a", "sub-b"] {
        repo.link(PROVIDER, subject, &an_identity("u-many"))
            .await
            .expect("link");
    }
    repo.link(PROVIDER, "sub-other", &an_identity("u-elsewhere"))
        .await
        .expect("link");

    let mut links = repo.links_of("u-many").await.expect("list");
    links.sort();
    assert_eq!(
        links,
        vec![
            (PROVIDER.to_string(), "sub-a".to_string()),
            (PROVIDER.to_string(), "sub-b".to_string()),
        ],
        "another tenant's links are not this tenant's to see or release"
    );
}

#[tokio::test]
async fn releasing_leaves_nothing_in_the_tenants_own_listing() {
    let repo = repo!();

    repo.link(PROVIDER, "sub-clean", &an_identity("u-clean"))
        .await
        .expect("link");
    repo.unlink(PROVIDER, "sub-clean", "u-clean")
        .await
        .expect("unlink");

    assert!(repo.links_of("u-clean").await.expect("list").is_empty());
}

// ---------------------------------------------------------------- accounts

#[tokio::test]
async fn an_account_is_created_once_and_read_back_whole() {
    let repo = repo!();

    let user = User::new(
        "acct-1".to_string(),
        Some("one@example.com".to_string()),
        NOW,
    );
    assert!(repo.create_user(&user).await.expect("create"));

    let found = repo
        .find_user("acct-1")
        .await
        .expect("find")
        .expect("the account just created");
    assert_eq!(found, user);
    assert_eq!(found.vip_level, 0, "a fresh account starts at level 0");
    assert!(found.is_active());

    // An id is minted once. A second create with the same id is a bug
    // somewhere upstream; it must not silently reset the level or the status.
    repo.set_vip_level("acct-1", 3, NOW + 1)
        .await
        .expect("set level");
    assert!(
        !repo.create_user(&user).await.expect("create again"),
        "a second create must be refused, not applied"
    );
    assert_eq!(
        repo.find_user("acct-1")
            .await
            .expect("find")
            .unwrap()
            .vip_level,
        3,
        "the refused create must leave the row untouched"
    );
}

#[tokio::test]
async fn role_round_trips() {
    let repo = repo!();

    // A member's row carries no role attribute and reads as member.
    let user = User::new("acct-role".to_string(), None, NOW);
    repo.create_user(&user).await.expect("create");
    let found = repo.find_user("acct-role").await.expect("find").unwrap();
    assert_eq!(found.role, Role::Member);

    assert!(
        repo.set_role("acct-role", Role::Operator, NOW + 1)
            .await
            .expect("set role")
    );
    let found = repo.find_user("acct-role").await.expect("find").unwrap();
    assert_eq!(found.role, Role::Operator);
    assert_eq!(found.updated_at, NOW + 1);

    assert!(
        repo.set_role("acct-role", Role::Member, NOW + 2)
            .await
            .expect("set role back")
    );
    assert_eq!(
        repo.find_user("acct-role")
            .await
            .expect("find")
            .unwrap()
            .role,
        Role::Member
    );

    // An operator created whole reads back whole.
    let mut operator = User::new("acct-op".to_string(), None, NOW);
    operator.role = Role::Operator;
    assert!(repo.create_user(&operator).await.expect("create"));
    assert_eq!(
        repo.find_user("acct-op").await.expect("find").unwrap(),
        operator
    );

    assert!(
        !repo
            .set_role("acct-nobody", Role::Operator, NOW)
            .await
            .expect("set role"),
        "an update must not conjure an account"
    );
}

#[tokio::test]
async fn a_missing_account_is_none_and_cannot_be_updated_into_existence() {
    let repo = repo!();

    assert!(repo.find_user("acct-nobody").await.expect("find").is_none());
    assert!(
        !repo
            .set_vip_level("acct-nobody", 5, NOW)
            .await
            .expect("set level"),
        "an update must not conjure a half-formed account"
    );
    assert!(
        !repo
            .set_status("acct-nobody", UserStatus::Suspended, NOW)
            .await
            .expect("set status")
    );
    assert!(
        repo.find_user("acct-nobody").await.expect("find").is_none(),
        "and nothing was written"
    );
}

#[tokio::test]
async fn level_and_status_changes_stamp_updated_at() {
    let repo = repo!();

    let user = User::new("acct-2".to_string(), None, NOW);
    repo.create_user(&user).await.expect("create");

    assert!(
        repo.set_vip_level("acct-2", 9, NOW + 10)
            .await
            .expect("set")
    );
    assert!(
        repo.set_status("acct-2", UserStatus::Suspended, NOW + 20)
            .await
            .expect("set")
    );

    let found = repo.find_user("acct-2").await.expect("find").unwrap();
    assert_eq!(found.vip_level, 9);
    assert_eq!(found.status, UserStatus::Suspended);
    assert!(!found.is_active());
    assert_eq!(found.created_at, NOW);
    assert_eq!(found.updated_at, NOW + 20);
}

/// The account row lives under its own partition prefix, but it shares the
/// table with the bots and with the two readers that walk it without a
/// sort-key condition. Its id is also the tenant's, so the two prefixes must not
/// collide on the same string.
#[tokio::test]
async fn account_rows_do_not_break_the_bot_readers() {
    let repo = repo!();

    let bot = Bot::new(
        "gamma".to_string(),
        "acct-list".to_string(),
        Exchange::Bybit,
        "gamma".to_string(),
        "ak".to_string(),
        "sk".to_string(),
        false,
        Runtime::Py,
        NOW,
        NOW,
    );
    repo.save(&bot).await.expect("save");
    repo.create_user(&User::new("acct-list".to_string(), None, NOW))
        .await
        .expect("create");
    repo.link(PROVIDER_TELEGRAM, "5351347639", &an_identity("acct-list"))
        .await
        .expect("link");

    let bots = repo
        .find_by_user_id("acct-list")
        .await
        .expect("an account row must not corrupt the tenant's bot list");
    assert_eq!(bots.len(), 1);
    assert_eq!(bots[0].id, "gamma");

    let all = repo
        .find_all()
        .await
        .expect("an account row must not break the table-wide scan");
    assert!(all.iter().any(|b| b.id == "gamma"));
}

/// A Telegram id is an identity like any other: it names exactly one account,
/// and the account can release it to bind another.
#[tokio::test]
async fn a_telegram_id_names_one_account_and_can_be_rebound() {
    let repo = repo!();

    assert_eq!(
        repo.link(PROVIDER_TELEGRAM, "tg-1", &an_identity("acct-a"))
            .await
            .expect("link"),
        LinkOutcome::Linked
    );
    assert_eq!(
        repo.link(PROVIDER_TELEGRAM, "tg-1", &an_identity("acct-b"))
            .await
            .expect("link"),
        LinkOutcome::ClaimedByAnother,
        "a Telegram account cannot be attached to a second tenant"
    );

    // The bot resolves the sender through this lookup, so the Telegram id and
    // the WorkOS subject are separate namespaces even when they collide as
    // strings.
    assert!(
        repo.find_link(PROVIDER, "tg-1")
            .await
            .expect("find")
            .is_none(),
        "a subject is only unique within its provider"
    );

    assert!(
        repo.unlink(PROVIDER_TELEGRAM, "tg-1", "acct-a")
            .await
            .expect("unlink")
    );
    assert_eq!(
        repo.link(PROVIDER_TELEGRAM, "tg-1", &an_identity("acct-b"))
            .await
            .expect("link"),
        LinkOutcome::Linked,
        "released, the Telegram account can be bound elsewhere"
    );
}
