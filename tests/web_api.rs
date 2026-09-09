//! The REST surface the web console drives, through the same edge the Lambda
//! runs.
//!
//! What is asserted here is what a browser-facing API on a trading account gets
//! wrong: that nothing answers without the bearer, that a token's scope is
//! honoured, that a bot in another tenant is indistinguishable from none, that
//! the one route which accepts exchange keys stores them and never returns them,
//! and that no route hands out a strategy's parameters.

mod common;

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use common::oauth::FakeIssuer;
use common::telegram::STRANGER_ID;
use common::{BOT_USERNAME, Harness, NOW, RESOURCE};
use http::{Request, Response, StatusCode, header};
use pbtb_rust::domain::bot::Bot;
use pbtb_rust::domain::configtemplate::ConfigTemplate;
use pbtb_rust::domain::identity::{IdentityRepository, PROVIDER_TELEGRAM};
use pbtb_rust::interface::mcp::{AuthError, Principal, TokenVerifier};
use pbtb_rust::usecase::{BindOutcome, BindTelegramUseCase};
use serde_json::{Value, json};

const TOKEN: &str = "a-shared-bearer";
const USER: &str = "5351347639";
const SOMEONE_ELSE: &str = "4000000001";

macro_rules! harness {
    () => {
        match Harness::start().await {
            Some(h) => h,
            None => return,
        }
    };
}

fn request(method: &str, path: &str, auth: Option<&str>, body: Option<Value>) -> Request<Bytes> {
    let mut request = Request::builder()
        .method(method)
        .uri(format!("{}api/v1{path}", RESOURCE))
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = auth {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    request
        .body(body.map(|b| Bytes::from(b.to_string())).unwrap_or_default())
        .expect("a valid request")
}

fn get(path: &str) -> Request<Bytes> {
    request("GET", path, Some(TOKEN), None)
}

fn body(response: &Response<Bytes>) -> Value {
    serde_json::from_slice(response.body()).unwrap_or_else(|e| {
        panic!(
            "expected a json body, got {:?}: {e}",
            String::from_utf8_lossy(response.body())
        )
    })
}

fn a_bot(user_id: &str, name: &str) -> Bot {
    Bot::create(
        user_id.to_string(),
        name.to_string(),
        "key-of-the-bot".to_string(),
        "secret-of-the-bot".to_string(),
        NOW,
    )
}

/// A verifier handing out one fixed principal: the way to test what a token
/// with fewer scopes, or another tenant, can reach.
struct Fixed(Principal);

#[async_trait]
impl TokenVerifier for Fixed {
    async fn verify(&self, bearer: &str) -> Result<Principal, AuthError> {
        if bearer == TOKEN {
            Ok(self.0.clone())
        } else {
            Err(AuthError::Unauthenticated("wrong".into()))
        }
    }
}

fn read_only(user_id: &str) -> Arc<dyn TokenVerifier> {
    Arc::new(Fixed(Principal {
        user_id: user_id.to_string(),
        scopes: HashSet::from(["bots:read".to_string()]),
        vip_level: 0,
    }))
}

/// Both scopes at a given level: the way to test what a level may do.
fn at_level(user_id: &str, vip_level: u8) -> Arc<dyn TokenVerifier> {
    Arc::new(Fixed(Principal {
        user_id: user_id.to_string(),
        scopes: HashSet::from(["bots:read".to_string(), "bots:write".to_string()]),
        vip_level,
    }))
}

// ---------------------------------------------------------------- the edge

#[tokio::test]
async fn nothing_answers_without_a_bearer() {
    let h = harness!();
    let api = h.http_api(TOKEN);

    for path in ["/me", "/bots", "/templates", "/bots/x"] {
        let response = api.handle(request("GET", path, None, None)).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        assert!(
            response.headers().contains_key(header::WWW_AUTHENTICATE),
            "{path}: a 401 that does not name the scheme leaves the client guessing"
        );
    }
}

#[tokio::test]
async fn an_unknown_route_is_a_404_only_once_authenticated() {
    let h = harness!();
    let api = h.http_api(TOKEN);

    let anonymous = api.handle(request("GET", "/nothing", None, None)).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let known = api.handle(get("/nothing")).await;
    assert_eq!(known.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_read_only_token_can_look_but_not_touch() {
    let h = harness!();
    h.given_bot(a_bot(USER, "abot")).await;
    let api = h.http_api_with(read_only(USER));

    let listing = api.handle(get("/bots")).await;
    assert_eq!(listing.status(), StatusCode::OK);
    let id = body(&listing)["bots"][0]["bot_id"]
        .as_str()
        .expect("the seeded bot")
        .to_string();

    let start = api
        .handle(request(
            "POST",
            &format!("/bots/{id}/start"),
            Some(TOKEN),
            None,
        ))
        .await;
    assert_eq!(start.status(), StatusCode::FORBIDDEN);
    let challenge = start
        .headers()
        .get(header::WWW_AUTHENTICATE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        challenge.contains("insufficient_scope") && challenge.contains("bots:write"),
        "the client should learn which scope to ask for: {challenge}"
    );
    assert!(
        h.ecs.launches().is_empty(),
        "a refused start must not have reached ECS"
    );
}

// ---------------------------------------------------------------- tenancy

#[tokio::test]
async fn another_tenants_bot_is_indistinguishable_from_none() {
    let h = harness!();
    h.given_bot(a_bot(SOMEONE_ELSE, "theirs")).await;
    let their_id = a_bot(SOMEONE_ELSE, "theirs").id;
    let api = h.http_api(TOKEN);

    let listing = api.handle(get("/bots")).await;
    assert_eq!(body(&listing)["bots"], json!([]));

    let detail = api.handle(get(&format!("/bots/{their_id}"))).await;
    let missing = api.handle(get("/bots/no-such-bot")).await;
    assert_eq!(detail.status(), StatusCode::NOT_FOUND);
    assert_eq!(detail.body(), missing.body());

    let stop = api
        .handle(request(
            "POST",
            &format!("/bots/{their_id}/stop"),
            Some(TOKEN),
            None,
        ))
        .await;
    assert_eq!(stop.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------- bots

#[tokio::test]
async fn adding_a_bot_stores_the_keys_and_returns_none_of_them() {
    let h = harness!();
    let api = h.http_api(TOKEN);

    let created = api
        .handle(request(
            "POST",
            "/bots",
            Some(TOKEN),
            Some(json!({
                "name": "newbot",
                "api_key": "AKIA-the-key",
                "secret_key": "the-secret",
            })),
        ))
        .await;
    assert_eq!(
        created.status(),
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(created.body())
    );
    let created = body(&created);
    assert_eq!(created["status"], json!("added"));
    let id = created["bot"]["bot_id"]
        .as_str()
        .expect("an id")
        .to_string();

    let stored = h
        .api_keys
        .get(USER, &id)
        .expect("the keys took the storage path");
    assert_eq!(
        stored,
        ("AKIA-the-key".to_string(), "the-secret".to_string())
    );

    // The keys are in the store and nowhere in any answer.
    for path in ["/bots", &format!("/bots/{id}")] {
        let text = String::from_utf8_lossy(api.handle(get(path)).await.body()).to_string();
        assert!(!text.contains("AKIA-the-key"), "{path} leaked the api key");
        assert!(!text.contains("the-secret"), "{path} leaked the secret");
    }
}

#[tokio::test]
async fn a_same_name_add_is_refused_until_the_caller_says_overwrite() {
    let h = harness!();
    h.given_bot(a_bot(USER, "abot")).await;
    let api = h.http_api(TOKEN);
    let add = |overwrite: bool| {
        request(
            "POST",
            "/bots",
            Some(TOKEN),
            Some(json!({
                "name": "abot",
                "api_key": "rotated-key",
                "secret_key": "rotated-secret",
                "overwrite": overwrite,
            })),
        )
    };

    let refused = api.handle(add(false)).await;
    assert_eq!(refused.status(), StatusCode::CONFLICT);
    assert_eq!(body(&refused)["status"], json!("already_exists"));
    let id = a_bot(USER, "abot").id;
    assert_eq!(
        h.api_keys.get(USER, &id),
        None,
        "a refused add must write nothing"
    );

    let overwritten = api.handle(add(true)).await;
    assert_eq!(overwritten.status(), StatusCode::OK);
    assert_eq!(body(&overwritten)["status"], json!("overwritten"));
    assert_eq!(
        h.api_keys.get(USER, &id),
        Some(("rotated-key".to_string(), "rotated-secret".to_string()))
    );
}

#[tokio::test]
async fn deleting_needs_the_id_named_twice() {
    let h = harness!();
    let bot = a_bot(USER, "abot");
    h.given_bot(bot.clone()).await;
    let api = h.http_api(TOKEN);
    let path = format!("/bots/{}", bot.id);

    let unconfirmed = api
        .handle(request("DELETE", &path, Some(TOKEN), Some(json!({}))))
        .await;
    assert_eq!(unconfirmed.status(), StatusCode::BAD_REQUEST);
    assert_eq!(api.handle(get(&path)).await.status(), StatusCode::OK);

    let confirmed = api
        .handle(request(
            "DELETE",
            &path,
            Some(TOKEN),
            Some(json!({ "confirm": bot.id })),
        ))
        .await;
    assert_eq!(confirmed.status(), StatusCode::OK);
    assert_eq!(api.handle(get(&path)).await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_start_claims_the_same_lock_a_button_does() {
    let h = harness!();
    let bot = a_bot(USER, "abot");
    h.given_bot(bot.clone()).await;
    h.configs.put(
        pbtb_rust::domain::botconfig::BotConfig::from_template(
            USER.to_string(),
            bot.id.clone(),
            &a_template("v7-template"),
            NOW,
        )
        .expect("a config from the template"),
    );
    let api = h.http_api(TOKEN);
    let start = || {
        request(
            "POST",
            &format!("/bots/{}/start", bot.id),
            Some(TOKEN),
            None,
        )
    };

    let first = api.handle(start()).await;
    assert_eq!(
        first.status(),
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(first.body())
    );
    assert_eq!(body(&first)["status"], json!("started"));

    let second = api.handle(start()).await;
    assert_eq!(second.status(), StatusCode::OK);
    assert_ne!(
        body(&second)["status"],
        json!("started"),
        "a second start while a task is up must launch nothing"
    );
    assert_eq!(h.ecs.launches().len(), 1);
}

#[tokio::test]
async fn the_bot_detail_describes_the_config_without_dumping_it() {
    let h = harness!();
    let bot = a_bot(USER, "abot");
    h.given_bot(bot.clone()).await;
    h.configs.put(
        pbtb_rust::domain::botconfig::BotConfig::from_template(
            USER.to_string(),
            bot.id.clone(),
            &a_template("v7-template"),
            NOW,
        )
        .expect("a config from the template"),
    );
    let api = h.http_api(TOKEN);

    let detail = api.handle(get(&format!("/bots/{}", bot.id))).await;
    assert_eq!(detail.status(), StatusCode::OK);
    let detail = body(&detail);
    assert_eq!(detail["config"]["template_name"], json!("v7-template"));
    assert_eq!(detail["config"]["sides"]["long"], json!(true));
    assert_eq!(detail["config"]["coins"]["long"], json!(["XRP"]));
    // The strategy's parameters are the whole value of a template and never
    // leave the server: no `bot` section, no grid or trailing values.
    let text = detail.to_string();
    assert!(
        !text.contains("entry_grid_spacing") && !text.contains("\"bot\""),
        "the detail leaked strategy parameters: {text}"
    );
}

// ---------------------------------------------------------------- templates

#[tokio::test]
async fn a_template_is_described_never_dumped() {
    let h = harness!();
    h.templates.add(a_template("v7-template"));
    let api = h.http_api(TOKEN);

    let listing = api.handle(get("/templates")).await;
    assert_eq!(
        body(&listing)["templates"],
        json!([{ "name": "v7-template", "min_vip_level": 0 }])
    );

    let one = api.handle(get("/templates/v7-template")).await;
    assert_eq!(
        one.status(),
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(one.body())
    );
    let one = body(&one);
    assert_eq!(one["name"], json!("v7-template"));
    assert_eq!(one["description"], json!("a strategy"));
    assert_eq!(one["min_vip_level"], json!(0));
    assert_eq!(one["coins"]["long"], json!(["XRP"]));
    let text = one.to_string();
    assert!(
        !text.contains("entry_grid_spacing") && !text.contains("\"bot\""),
        "the template answer leaked strategy parameters: {text}"
    );

    let missing = api.handle(get("/templates/no-such")).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_template_above_the_callers_level_is_refused_but_never_hidden() {
    let h = harness!();
    let mut gated = a_template("gated");
    gated.config_data["pbtb"]["min_vip_level"] = json!(3);
    h.templates.add(gated);
    h.given_bot(a_bot(USER, "abot")).await;
    let api = h.http_api_with(at_level(USER, 2));

    let listing = api.handle(get("/templates")).await;
    assert_eq!(
        body(&listing)["templates"],
        json!([{ "name": "gated", "min_vip_level": 3 }]),
        "the catalogue shows what a higher level unlocks"
    );
    let described = api.handle(get("/templates/gated")).await;
    assert_eq!(described.status(), StatusCode::OK, "reading is never gated");
    assert_eq!(body(&described)["min_vip_level"], json!(3));

    let refused = api
        .handle(request(
            "POST",
            "/bots/abot/template",
            Some(TOKEN),
            Some(json!({ "name": "gated" })),
        ))
        .await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    let refused = body(&refused);
    assert_eq!(refused["error"], json!("insufficient_level"));
    assert_eq!(refused["required"], json!(3));
    assert_eq!(refused["current"], json!(2));
    assert!(
        h.configs.get_saved(USER, "abot").is_none(),
        "a refused apply leaves no config behind"
    );

    let allowed = h.http_api_with(at_level(USER, 3));
    let applied = allowed
        .handle(request(
            "POST",
            "/bots/abot/template",
            Some(TOKEN),
            Some(json!({ "name": "gated" })),
        ))
        .await;
    assert_eq!(
        applied.status(),
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(applied.body())
    );
}

#[tokio::test]
async fn a_level_zero_account_runs_one_bot_at_a_time() {
    let h = harness!();
    for name in ["first", "second"] {
        let bot = a_bot(USER, name);
        h.given_bot(bot.clone()).await;
        h.configs.put(
            pbtb_rust::domain::botconfig::BotConfig::from_template(
                USER.to_string(),
                bot.id.clone(),
                &a_template("v7-template"),
                NOW,
            )
            .expect("a config from the template"),
        );
    }
    let api = h.http_api_with(at_level(USER, 0));
    let start = |id: &str| request("POST", &format!("/bots/{id}/start"), Some(TOKEN), None);

    let first = api.handle(start("first")).await;
    assert_eq!(
        first.status(),
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(first.body())
    );

    let second = api.handle(start("second")).await;
    assert_eq!(second.status(), StatusCode::FORBIDDEN);
    let refused = body(&second);
    assert_eq!(refused["error"], json!("quota_exceeded"));
    assert_eq!(refused["limit"], json!(1));
    assert_eq!(h.ecs.launches().len(), 1, "the ceiling never reached ECS");

    // The bot that is on may be run again: its slot is its own.
    let again = api.handle(start("first")).await;
    assert_eq!(again.status(), StatusCode::OK);
}

// ---------------------------------------------------------------- account

#[tokio::test]
async fn me_describes_the_account_and_its_telegram_binding() {
    let h = harness!();
    h.given_link("workos", "user_01SUBJECT", USER).await;
    let api = h.http_api(TOKEN);

    let me = api.handle(get("/me")).await;
    assert_eq!(me.status(), StatusCode::OK);
    let me = body(&me);
    assert_eq!(me["user_id"], json!(USER));
    assert_eq!(
        me["telegram"],
        json!(USER),
        "the harness binds the operator's own id"
    );
    assert!(me["vip_level"].is_number());
    assert!(
        me["identities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|it| it == &json!({ "provider": "workos", "subject": "user_01SUBJECT" }))
    );
}

#[tokio::test]
async fn a_bind_ticket_is_a_deep_link_the_bot_redeems_for_the_caller() {
    let h = harness!();
    let api = h.http_api(TOKEN);

    // The operator already speaks through a Telegram id; release it first, as
    // the account page does before offering a new bind.
    let released = api
        .handle(request("DELETE", "/me/telegram", Some(TOKEN), None))
        .await;
    assert_eq!(released.status(), StatusCode::OK);
    assert_eq!(body(&released)["released"], json!(1));
    assert_eq!(body(&api.handle(get("/me")).await)["telegram"], Value::Null);

    let ticket = api
        .handle(request(
            "POST",
            "/me/telegram/bind-ticket",
            Some(TOKEN),
            None,
        ))
        .await;
    assert_eq!(ticket.status(), StatusCode::OK);
    let ticket = body(&ticket);
    let token = ticket["token"].as_str().expect("a token").to_string();
    assert_eq!(token.len(), 64);
    assert_eq!(
        ticket["url"],
        json!(format!("https://t.me/{BOT_USERNAME}?start={token}"))
    );

    // Whoever opens it in the bot is bound to the caller — the ticket named the
    // account; nothing in the bot's message does.
    let bind = BindTelegramUseCase::new(
        h.bots.clone(),
        h.bots.clone(),
        Arc::new(common::fakes::FixedClock(NOW)),
    );
    assert_eq!(
        bind.execute(&token, &STRANGER_ID.to_string())
            .await
            .expect("bind"),
        BindOutcome::Bound {
            user_id: USER.to_string()
        }
    );
    let identities: &dyn IdentityRepository = h.bots.as_ref();
    assert_eq!(
        identities
            .find_link(PROVIDER_TELEGRAM, &STRANGER_ID.to_string())
            .await
            .expect("find")
            .expect("bound")
            .user_id,
        USER
    );
    assert_eq!(
        body(&api.handle(get("/me")).await)["telegram"],
        json!(STRANGER_ID.to_string())
    );

    // A read-only token can look at the account but not change what it speaks
    // through.
    let api = h.http_api_with(read_only(USER));
    assert_eq!(
        api.handle(request(
            "POST",
            "/me/telegram/bind-ticket",
            Some(TOKEN),
            None
        ))
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn signup_creates_the_account_a_verified_subject_lacks_and_only_that() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    let api = h.http_api_with(h.oauth_verifier(&issuer.issuer()).await);

    let mut claims = issuer.claims("user_01NEWCOMER", "bots:read bots:write", RESOURCE);
    claims["email"] = json!("newcomer@example.com");
    let token = issuer.token(claims);

    // Before signing up, a verified subject is nobody here.
    let refused = api
        .handle(request("GET", "/bots", Some(&token), None))
        .await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    let created = api
        .handle(request("POST", "/signup", Some(&token), None))
        .await;
    assert_eq!(
        created.status(),
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(created.body())
    );
    let created = body(&created);
    assert_eq!(created["status"], json!("created"));
    assert_eq!(created["vip_level"], json!(0));
    let user_id = created["user_id"].as_str().expect("an id").to_string();
    assert_ne!(
        user_id, "user_01NEWCOMER",
        "the account id is not the subject"
    );

    // The same token now reaches an empty tenant of its own.
    let bots = api
        .handle(request("GET", "/bots", Some(&token), None))
        .await;
    assert_eq!(bots.status(), StatusCode::OK);
    assert_eq!(body(&bots)["bots"], json!([]));
    let me = body(&api.handle(request("GET", "/me", Some(&token), None)).await);
    assert_eq!(me["user_id"], json!(user_id));
    assert_eq!(me["telegram"], Value::Null);

    // Signing up again is the same account, not a second one.
    let again = api
        .handle(request("POST", "/signup", Some(&token), None))
        .await;
    assert_eq!(again.status(), StatusCode::OK);
    assert_eq!(body(&again)["status"], json!("existing"));
    assert_eq!(body(&again)["user_id"], json!(user_id));

    // Signup needs a verified token like everything else.
    assert_eq!(
        api.handle(request("POST", "/signup", None, None))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let stale =
        issuer.token(issuer.claims("user_01OTHER", "bots:read", "https://elsewhere.example/"));
    assert_eq!(
        api.handle(request("POST", "/signup", Some(&stale), None))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn the_shared_bearer_cannot_sign_anyone_up() {
    let h = harness!();
    let api = h.http_api(TOKEN);
    // A deployment-wide token names no subject; there is nobody to create an
    // account for.
    assert_eq!(
        api.handle(request("POST", "/signup", Some(TOKEN), None))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn the_placeholders_say_so() {
    let h = harness!();
    let api = h.http_api(TOKEN);

    let balance = api.handle(get("/bots/any/balance")).await;
    assert_eq!(balance.status(), StatusCode::NOT_IMPLEMENTED);
}

/// A v7-shaped template with one long coin and the grid parameters a real one
/// carries, so a test can assert those never come back.
fn a_template(name: &str) -> ConfigTemplate {
    ConfigTemplate {
        name: name.to_string(),
        description: Some("a strategy".to_string()),
        version: Some("1".to_string()),
        config_data: json!({
            "config_version": "v7.12.0",
            "pbtb": {
                "name": name,
                "exchange": "bybit",
                "description": "a strategy",
                "strategies": [{ "name": name, "side": "long" }],
            },
            "live": {
                "user": "",
                "leverage": 3.0,
                "approved_coins": { "long": ["XRP"], "short": [] },
                "forced_mode_long": "",
                "forced_mode_short": "graceful_stop",
            },
            "bot": {
                "long": {
                    "total_wallet_exposure_limit": 1.5,
                    "entry_grid_spacing_pct": 0.06,
                    "n_positions": 1,
                },
                "short": {
                    "total_wallet_exposure_limit": 0.0,
                    "entry_grid_spacing_pct": 0.06,
                    "n_positions": 0,
                },
            },
        }),
    }
}
