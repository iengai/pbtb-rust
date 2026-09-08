//! Linking a Telegram account to an external identity.
//!
//! A third adapter beside `telegram` and `mcp`, and the only place this crate is
//! an OAuth *client* rather than a resource server. The two roles share an
//! authorization server and nothing else: here we send a user to authenticate
//! and record who came back, there we check a token someone presents.
//!
//! The browser leg cannot be trusted to say who it is, so it never gets to. The
//! bot mints a ticket for the user it has already authenticated, the ticket is
//! redeemed server-side, and the tenant that ends up in the identity row comes
//! from that ticket — never from a parameter, a cookie or a claim.

mod oauth_client;
mod page;

use std::sync::Arc;

use bytes::Bytes;
use http::{Method, Request, Response, StatusCode, header};

use crate::domain::clock::Clock;
use crate::domain::identity::{
    IdentityRepository, LINK_TICKET_TTL, LinkOutcome, LinkTicket, LinkTicketRepository,
    LinkedIdentity,
};
use crate::domain::secret::{random_token, token_digest};
use crate::usecase::PURPOSE_START;

pub use oauth_client::OAuthClient;

/// The second leg's key. The first is `PURPOSE_START`, owned by the use case
/// that mints it; they are kept apart so a link URL cannot be presented as the
/// authorization it was never part of.
const PURPOSE_CALLBACK: &str = "callback";

/// Names the cookie that ties a callback to the browser the flow started in.
/// Scoped to this flow's own path so it is not sent anywhere else on the host.
const BROWSER_COOKIE: &str = "pbtb_link";

pub const PATH_START: &str = "/link";
pub const PATH_CALLBACK: &str = "/link/callback";

pub struct LinkFlow {
    tickets: Arc<dyn LinkTicketRepository>,
    identities: Arc<dyn IdentityRepository>,
    clock: Arc<dyn Clock>,
    client: OAuthClient,
    provider: String,
}

impl LinkFlow {
    pub fn new(
        tickets: Arc<dyn LinkTicketRepository>,
        identities: Arc<dyn IdentityRepository>,
        clock: Arc<dyn Clock>,
        client: OAuthClient,
        provider: impl Into<String>,
    ) -> Self {
        Self {
            tickets,
            identities,
            clock,
            client,
            provider: provider.into(),
        }
    }

    /// Serve a request if it belongs to this flow, or hand it back.
    ///
    /// Returning `None` rather than a 404 keeps the routing decision with the
    /// caller: this shares a host with the MCP surface, and a path this flow does
    /// not own is not necessarily a path nobody owns.
    pub async fn handle(&self, request: &Request<Bytes>) -> Option<Response<Bytes>> {
        if request.method() != Method::GET {
            return None;
        }
        match request.uri().path() {
            PATH_CALLBACK => Some(
                self.finish(
                    query(request, "code"),
                    query(request, "state"),
                    cookie(request, BROWSER_COOKIE),
                )
                .await,
            ),
            PATH_START => Some(self.start(query(request, "t")).await),
            _ => None,
        }
    }

    /// Redeem the bot's ticket and send the user on to authenticate.
    async fn start(&self, token: Option<String>) -> Response<Bytes> {
        let Some(token) = token else {
            return page::failed("This link is not valid.");
        };

        let now = self.clock.now();
        let ticket = match self
            .tickets
            .redeem(PURPOSE_START, &token_digest(&token), now)
            .await
        {
            Ok(Some(ticket)) => ticket,
            Ok(None) => return page::failed("This link has expired or has already been used."),
            Err(error) => {
                tracing::warn!(%error, "could not redeem a link ticket");
                return page::failed("Something went wrong. Try the button again.");
            }
        };

        let state = random_token();
        let verifier = random_token();
        // Held only by the browser that started this. The row is keyed by state
        // and this together, so a `state` read out of a Referer header, a proxy
        // log or a browser history addresses nothing on its own. It does not
        // speak to a flow run start-to-finish in one browser; what limits who can
        // start one is that the URL only ever goes to a private chat.
        let browser = random_token();
        // The bot's token stops here. Everything downstream — the redirect URL,
        // the browser's history, whatever Referer the authorization server sees —
        // carries `state` instead, which is worth nothing without the code that
        // comes back with it.
        let pending = LinkTicket {
            code_verifier: Some(verifier.clone()),
            ..ticket
        };
        if let Err(error) = self
            .tickets
            .issue(
                PURPOSE_CALLBACK,
                &token_digest(&(state.clone() + &browser)),
                &pending,
                now,
                now + LINK_TICKET_TTL,
            )
            .await
        {
            tracing::warn!(%error, "could not record a pending authorization");
            return page::failed("Something went wrong. Try the button again.");
        }

        page::redirect(&self.client.authorize_url(&state, &verifier), &browser)
    }

    /// Turn the authorization back into a tenant and record the link.
    async fn finish(
        &self,
        code: Option<String>,
        state: Option<String>,
        browser: Option<String>,
    ) -> Response<Bytes> {
        let (Some(code), Some(state), Some(browser)) = (code, state, browser) else {
            // Also the shape of a user who declined: the authorization server
            // sends them back with an error and no code.
            return page::failed("Sign-in did not complete.");
        };

        let now = self.clock.now();
        let ticket = match self
            .tickets
            .redeem(PURPOSE_CALLBACK, &token_digest(&(state + &browser)), now)
            .await
        {
            Ok(Some(ticket)) => ticket,
            // No pending authorization under this state: a forged callback, a
            // replayed one, or one that sat too long. This is what CSRF looks
            // like from here, and it is refused before the code is ever spent.
            Ok(None) => return page::failed("This sign-in has expired. Try the button again."),
            Err(error) => {
                tracing::warn!(%error, "could not redeem a pending authorization");
                return page::failed("Something went wrong. Try the button again.");
            }
        };

        // No verifier means no proof this callback belongs to the authorization
        // that started it. Sending an empty one would hand that decision to the
        // authorization server, which is the one place this must not be decided.
        let Some(verifier) = ticket.code_verifier else {
            return page::failed("Sign-in did not complete.");
        };
        let account = match self.client.exchange(&code, &verifier).await {
            Ok(account) => account,
            Err(error) => {
                tracing::warn!(%error, "could not exchange an authorization code");
                return page::failed("Sign-in did not complete.");
            }
        };

        // 🔴 The tenant comes from the ticket the bot minted, never from the
        // browser and never from the provider. Taking it from anything the caller
        // influences would make linking a way into someone else's bots.
        let identity = LinkedIdentity {
            user_id: ticket.user_id,
            email: account.email,
            linked_at: now,
        };

        match self
            .identities
            .link(&self.provider, &account.subject, &identity)
            .await
        {
            Ok(LinkOutcome::Linked) => page::linked("Your account is linked."),
            Ok(LinkOutcome::AlreadyLinked) => page::linked("This account was already linked."),
            Ok(LinkOutcome::ClaimedByAnother) => {
                page::failed("That identity is already linked to a different account.")
            }
            Err(error) => {
                tracing::warn!(%error, "could not record a link");
                page::failed("Something went wrong. Try the button again.")
            }
        }
    }
}

/// One cookie by name. Hand-parsed rather than pulled in as a dependency: the
/// grammar is `name=value` pairs separated by "; ", and only one name matters.
fn cookie(request: &Request<Bytes>, name: &str) -> Option<String> {
    request
        .headers()
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
        .filter(|v| !v.is_empty())
}

fn query(request: &Request<Bytes>, key: &str) -> Option<String> {
    let query = request.uri().query()?;
    form_urlencoded::parse(query.as_bytes())
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
        .filter(|v| !v.is_empty())
}

/// A response that says only what the user needs and never echoes what they
/// sent: this page is reached straight from a redirect an attacker can compose.
fn respond(status: StatusCode, body: String) -> Response<Bytes> {
    let mut response = Response::new(Bytes::from(body));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}
