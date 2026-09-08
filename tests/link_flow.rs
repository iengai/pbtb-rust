//! Linking a Telegram account to an external identity, end to end.
//!
//! The whole point of the flow is that the browser never gets to say which
//! account it is linking, so most of these are about what happens when it tries
//! — a replayed link, a forged callback, an identity someone else already holds.

mod common;

use bytes::Bytes;
use common::oauth::{FakeIssuer, SUBJECT};
use common::telegram::USER_ID;
use common::{Harness, RESOURCE};
use http::{Request, Response, StatusCode, header};
use pbtb_rust::domain::IdentityRepository;
use pbtb_rust::interface::link::LinkFlow;

const PROVIDER: &str = "workos";
const OTHER_TENANT: &str = "999888777";

macro_rules! harness {
    () => {
        match Harness::start().await {
            Some(h) => h,
            None => return,
        }
    };
}

fn tenant() -> String {
    USER_ID.to_string()
}

fn get(url: &str) -> Request<Bytes> {
    Request::builder()
        .method("GET")
        .uri(url)
        .body(Bytes::new())
        .expect("a valid request")
}

/// The same request from the browser that started the flow, carrying the cookie
/// the redirect set.
fn get_as(url: &str, browser: &Browser) -> Request<Bytes> {
    Request::builder()
        .method("GET")
        .uri(url)
        .header(header::COOKIE, &browser.0)
        .body(Bytes::new())
        .expect("a valid request")
}

/// Whatever the redirect asked this browser to keep.
struct Browser(String);

fn cookie_from(response: &Response<Bytes>) -> Browser {
    let header = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_else(|| panic!("the redirect must bind the flow to a browser"));

    for attribute in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/link"] {
        assert!(
            header.contains(attribute),
            "the cookie needs {attribute}: {header}"
        );
    }
    Browser(
        header
            .split(';')
            .next()
            .expect("a name=value pair")
            .to_string(),
    )
}

fn location(response: &Response<Bytes>) -> String {
    response
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

fn param(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    form_urlencoded::parse(query.as_bytes())
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

/// Walk the first leg and hand back what the browser carries on: the `state` in
/// the URL, and the cookie that says it is the same browser.
async fn authorize(h: &Harness, flow: &LinkFlow) -> (String, Browser) {
    let url = h.given_link_ticket(&tenant(), 4242).await;
    let response = flow.handle(&get(&url)).await.expect("the flow owns /link");

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let redirect = location(&response);

    let token = param(&url, "t").expect("the bot's token");
    assert!(
        !redirect.contains(&token),
        "the bot's token must stop here: it is not carried into the browser's \
         history, or into whatever Referer the authorization server sees"
    );
    assert_eq!(
        param(&redirect, "code_challenge_method").as_deref(),
        Some("S256")
    );

    (
        param(&redirect, "state").expect("a state"),
        cookie_from(&response),
    )
}

#[tokio::test]
async fn a_completed_sign_in_links_the_identity_to_the_ticket_holder() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    let flow = h.link_flow(&issuer.issuer()).await;

    let (state, browser) = authorize(&h, &flow).await;
    let response = flow
        .handle(&get_as(
            &format!("{RESOURCE}link/callback?code=an-auth-code&state={state}"),
            &browser,
        ))
        .await
        .expect("the flow owns /link/callback");

    assert_eq!(response.status(), StatusCode::OK);
    let link = h
        .bots
        .find_link(PROVIDER, SUBJECT)
        .await
        .expect("find")
        .expect("linked");
    assert_eq!(
        link.user_id,
        tenant(),
        "the tenant comes from the ticket the bot minted"
    );

    let exchanges = issuer.exchanges().await;
    assert_eq!(exchanges.len(), 1);
    assert_eq!(
        exchanges[0].get("grant_type").map(String::as_str),
        Some("authorization_code")
    );
    assert!(
        exchanges[0].contains_key("code_verifier"),
        "the code alone must not be enough to spend: {:?}",
        exchanges[0]
    );
}

#[tokio::test]
async fn a_link_url_works_once() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    let flow = h.link_flow(&issuer.issuer()).await;

    let url = h.given_link_ticket(&tenant(), 4242).await;
    assert_eq!(
        flow.handle(&get(&url)).await.expect("first").status(),
        StatusCode::SEE_OTHER
    );

    // What a forwarded message, a browser history entry or a proxy log gets.
    let replay = flow.handle(&get(&url)).await.expect("second");
    assert_eq!(replay.status(), StatusCode::BAD_REQUEST);
    assert!(
        location(&replay).is_empty(),
        "a spent link must not send anyone anywhere"
    );
}

#[tokio::test]
async fn a_callback_with_a_state_nobody_issued_never_spends_the_code() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    let flow = h.link_flow(&issuer.issuer()).await;

    let response = flow
        .handle(&get(&format!(
            "{RESOURCE}link/callback?code=an-auth-code&state=a-state-i-made-up"
        )))
        .await
        .expect("the flow owns /link/callback");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        issuer.exchanges().await.is_empty(),
        "the forgery is refused before the code is spent, not after"
    );
    assert!(
        h.bots
            .find_link(PROVIDER, SUBJECT)
            .await
            .expect("find")
            .is_none(),
        "and nothing is linked"
    );
}

#[tokio::test]
async fn a_callback_cannot_name_the_tenant_it_links_to() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    let flow = h.link_flow(&issuer.issuer()).await;

    let (state, browser) = authorize(&h, &flow).await;
    // The obvious thing to try. Nothing reads these, which is why the tenant is
    // established before the browser is ever involved.
    let response = flow
        .handle(&get_as(
            &format!(
                "{RESOURCE}link/callback?code=an-auth-code&state={state}\
                 &user_id={OTHER_TENANT}&chat_id=1"
            ),
            &browser,
        ))
        .await
        .expect("the flow owns /link/callback");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        h.bots
            .find_link(PROVIDER, SUBJECT)
            .await
            .expect("find")
            .expect("linked")
            .user_id,
        tenant()
    );
}

#[tokio::test]
async fn an_identity_someone_else_holds_is_not_taken_over() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    let flow = h.link_flow(&issuer.issuer()).await;

    h.given_link(PROVIDER, SUBJECT, OTHER_TENANT).await;

    let (state, browser) = authorize(&h, &flow).await;
    let response = flow
        .handle(&get_as(
            &format!("{RESOURCE}link/callback?code=an-auth-code&state={state}"),
            &browser,
        ))
        .await
        .expect("the flow owns /link/callback");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        h.bots
            .find_link(PROVIDER, SUBJECT)
            .await
            .expect("find")
            .expect("still linked")
            .user_id,
        OTHER_TENANT,
        "the first tenant keeps the identity"
    );
}

#[tokio::test]
async fn a_declined_sign_in_says_so_rather_than_failing_silently() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    let flow = h.link_flow(&issuer.issuer()).await;

    // What the authorization server sends back when the user says no: an error
    // and no code.
    let response = flow
        .handle(&get(&format!(
            "{RESOURCE}link/callback?error=access_denied&state=whatever"
        )))
        .await
        .expect("the flow owns /link/callback");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        !String::from_utf8_lossy(response.body()).contains("access_denied"),
        "nothing the caller sent is echoed onto a page reached from a redirect \
         they composed"
    );
}

#[tokio::test]
async fn paths_the_flow_does_not_own_are_handed_back() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    let flow = h.link_flow(&issuer.issuer()).await;

    // The MCP surface shares this host; a 404 here would shadow it.
    assert!(flow.handle(&get(RESOURCE)).await.is_none());
    assert!(
        flow.handle(&get(&format!("{RESOURCE}linkage")))
            .await
            .is_none(),
        "a prefix is not a match"
    );
}

#[tokio::test]
async fn a_callback_from_a_different_browser_is_refused() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    let flow = h.link_flow(&issuer.issuer()).await;

    // A callback that did not come from the browser the redirect went to. The
    // row is keyed by state and the cookie together, so this one addresses
    // nothing.
    let (state, _attackers_browser) = authorize(&h, &flow).await;

    let response = flow
        .handle(&get(&format!(
            "{RESOURCE}link/callback?code=an-auth-code&state={state}"
        )))
        .await
        .expect("the flow owns /link/callback");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        issuer.exchanges().await.is_empty(),
        "and the code is never spent"
    );
    assert!(
        h.bots
            .find_link(PROVIDER, SUBJECT)
            .await
            .expect("find")
            .is_none()
    );
}

#[tokio::test]
async fn a_stolen_state_is_useless_without_the_cookie_that_goes_with_it() {
    let h = harness!();
    let issuer = FakeIssuer::start().await;
    let flow = h.link_flow(&issuer.issuer()).await;

    let (state, _) = authorize(&h, &flow).await;
    // A `state` read out of a Referer header, a proxy log or a browser history,
    // replayed with a cookie of the attacker's own choosing.
    let forged = Browser("pbtb_link=deadbeef".to_string());

    let response = flow
        .handle(&get_as(
            &format!("{RESOURCE}link/callback?code=an-auth-code&state={state}"),
            &forged,
        ))
        .await
        .expect("the flow owns /link/callback");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(issuer.exchanges().await.is_empty());
}
