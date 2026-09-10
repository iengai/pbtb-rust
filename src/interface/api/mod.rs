//! The REST surface the web console drives, on the same host as the MCP
//! surface and behind the same bearer check.
//!
//! A sibling of `interface::mcp`, not a layer above it: both call the same use
//! cases, so the exclusive start lock and the tenant boundary hold for a fetch
//! from a browser exactly as they do for a tool call or a button press.
//!
//! What sets this surface apart from the MCP one, and why it exists at all:
//!
//! 1. **It accepts exchange credentials.** Adding a bot means entering keys, which
//!    the MCP surface refuses because a tool argument lands in a model's context.
//!    A browser posting over TLS has no such audience, so `POST /bots` lives
//!    here and only here. The body is parsed and handed to the use case; it is
//!    never logged.
//! 2. **It never returns a strategy's parameters.** A template's config is what
//!    makes a bot worth running, and a caller who can read it can run it
//!    elsewhere. Templates and bots are described — name, sides, coins, risk,
//!    the backtest's headline — and the `bot` section stays on the server.
//! 3. **Every write is audited** with principal, route, bot and outcome.
//!
//! Tenant isolation is the same rule as everywhere else: `user_id` comes from
//! the verified token, no route takes one, and a bot outside the caller's tenant
//! is indistinguishable from a bot that does not exist.

mod handlers;

use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderValue, Method, Request, Response, StatusCode, header};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::mcp;
use super::mcp::auth::{AuthError, Principal, SCOPE_READ, SCOPE_WRITE, TokenVerifier};
use super::mcp::http::{Metadata, bearer, insufficient_scope, refuse};
use super::redaction::redact;
use crate::domain::error::{DomainError, Retryability};
use crate::usecase::{AddBotUseCase, SignupOutcome, SignupUseCase};

/// Every route lives under this prefix, so the MCP protocol keeps `/` and the
/// link flow keeps `/link` on the shared host.
pub const PREFIX: &str = "/api/v1";

/// The use cases the routes drive: everything the MCP tools have, plus the two
/// a tool must not offer — key entry, and creating the account itself.
#[derive(Clone)]
pub struct Deps {
    pub mcp: mcp::Deps,
    pub add_bot_usecase: Arc<AddBotUseCase>,
    pub signup_usecase: Arc<SignupUseCase>,
}

pub struct WebApi {
    deps: Deps,
    tokens: Arc<dyn TokenVerifier>,
    metadata: Metadata,
}

/// Why a route did not produce its normal answer, and the status that says so.
#[derive(Debug)]
pub(crate) enum ApiError {
    /// A good token without the scope this route needs.
    Scope(&'static str),
    /// The caller's own input: a malformed body, a confirm that does not match.
    BadRequest(String),
    /// Nothing by that name in the caller's tenant.
    NotFound,
    /// The bot is not in a state that admits this write right now.
    Conflict(Value),
    /// The account's level does not allow this: `{error, ...}` with the code
    /// (`quota_exceeded`, `insufficient_level`) and the numbers behind it.
    Forbidden(Value),
    /// A use-case fault, already redacted to what the caller may see.
    Failed { message: String, retryable: bool },
    /// A telebot feature that is a placeholder there and so a placeholder here.
    NotAvailable,
}

impl ApiError {
    /// Map a use-case fault the way every adapter does: the user's own mistakes
    /// come back verbatim, everything else is redacted to a category and a
    /// correlation id, with the cause logged under that id.
    pub(crate) fn from_domain(action: &str, err: DomainError) -> Self {
        match err {
            DomainError::RiskOutOfRange { .. }
            | DomainError::LeverageOutOfRange { .. }
            | DomainError::MissingConfigPath(_)
            | DomainError::InvalidConfig(_) => Self::BadRequest(err.to_string()),
            DomainError::QuotaExceeded { limit } => Self::Forbidden(json!({
                "error": "quota_exceeded",
                "limit": limit,
                "message": err.to_string(),
            })),
            DomainError::InsufficientLevel { required, current } => Self::Forbidden(json!({
                "error": "insufficient_level",
                "required": required,
                "current": current,
                "message": err.to_string(),
            })),
            other => Self::Failed {
                message: redact(action, &other),
                retryable: matches!(other.retryability(), Retryability::Transient),
            },
        }
    }
}

type ApiResult = Result<Response<Bytes>, ApiError>;

impl WebApi {
    pub fn new(deps: Deps, tokens: Arc<dyn TokenVerifier>, metadata: Metadata) -> Self {
        Self {
            deps,
            tokens,
            metadata,
        }
    }

    /// Whether a request is addressed to this surface. Routing stays with the
    /// caller: on the shared host a path this surface does not own may be the
    /// protocol's or the link flow's.
    pub fn owns<T>(request: &Request<T>) -> bool {
        let path = request.uri().path();
        path == PREFIX || path.starts_with(&format!("{PREFIX}/"))
    }

    /// Serve one request. Authentication comes first and unconditionally: no
    /// route on this surface answers an anonymous caller, so the principal is
    /// resolved before the path is even looked at. The one route that takes a
    /// verified subject without an account is signup, which is what creates
    /// the account a principal is resolved from.
    pub async fn handle(&self, request: Request<Bytes>) -> Response<Bytes> {
        if request.method() == Method::POST && request.uri().path() == format!("{PREFIX}/signup") {
            return self.signup(&request).await;
        }

        let principal = match bearer(&request) {
            Some(token) => self.tokens.verify(token).await,
            None => Err(AuthError::Unauthenticated("no bearer presented".into())),
        };
        let principal = match principal {
            Ok(principal) => principal,
            Err(refusal) => {
                // Logged, never returned: whether a token failed verification or
                // belongs to nobody here is a free probe otherwise.
                tracing::info!(%refusal, "refused an API request");
                return refuse(&self.metadata, &refusal);
            }
        };

        let result = self.route(&principal, &request).await;
        match result {
            Ok(response) => response,
            Err(error) => self.error_response(error),
        }
    }

    /// Create the account behind a verified subject, or find the one it has.
    ///
    /// Needs a bearer like every route, but only asks it who it is: the
    /// subject is refused nowhere else until this has run. Explicit rather
    /// than a side effect of the first request, so authenticating with the
    /// provider is never by itself an account.
    async fn signup(&self, request: &Request<Bytes>) -> Response<Bytes> {
        let verified = match bearer(request) {
            Some(token) => self.tokens.identify(token).await,
            None => Err(AuthError::Unauthenticated("no bearer presented".into())),
        };
        let verified = match verified {
            Ok(verified) => verified,
            Err(refusal) => {
                tracing::info!(%refusal, "refused a signup");
                return refuse(&self.metadata, &refusal);
            }
        };

        match self
            .deps
            .signup_usecase
            .execute(&verified.subject, verified.email.as_deref())
            .await
        {
            Ok(SignupOutcome::Created(user)) => {
                tracing::info!(principal = %user.id, route = "signup", outcome = "created", "api write");
                respond(
                    StatusCode::CREATED,
                    json!({ "status": "created", "user_id": user.id, "vip_level": user.vip_level }),
                )
            }
            Ok(SignupOutcome::Existing(user)) => respond(
                StatusCode::OK,
                json!({ "status": "existing", "user_id": user.id, "vip_level": user.vip_level }),
            ),
            Err(e) => self.error_response(ApiError::from_domain("signing up", e)),
        }
    }

    async fn route(&self, principal: &Principal, request: &Request<Bytes>) -> ApiResult {
        let path = request.uri().path();
        let rest = path.strip_prefix(PREFIX).unwrap_or_default();
        let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
        let method = request.method();
        let body = request.body();
        let h = handlers::Handlers {
            deps: &self.deps,
            principal,
        };

        match (method, segments.as_slice()) {
            (&Method::GET, ["me"]) => h.me().await,
            (&Method::POST, ["me", "telegram", "bind-ticket"]) => h.bind_ticket().await,
            (&Method::DELETE, ["me", "telegram"]) => h.unbind_telegram().await,

            (&Method::GET, ["bots"]) => h.list_bots().await,
            (&Method::POST, ["bots"]) => h.add_bot(parse(body)?).await,
            (&Method::GET, ["bots", id]) => h.get_bot(id).await,
            (&Method::DELETE, ["bots", id]) => h.delete_bot(id, parse(body)?).await,
            (&Method::POST, ["bots", id, "start"]) => h.start_bot(id).await,
            (&Method::POST, ["bots", id, "stop"]) => h.stop_bot(id).await,
            (&Method::PUT, ["bots", id, "risk"]) => h.set_risk(id, parse(body)?).await,
            (&Method::PUT, ["bots", id, "sides"]) => h.set_side(id, parse(body)?).await,
            (&Method::PUT, ["bots", id, "runtime"]) => h.set_runtime(id, parse(body)?).await,
            (&Method::POST, ["bots", id, "template"]) => h.apply_template(id, parse(body)?).await,
            (&Method::GET, ["bots", id, "returns"]) => h.bot_returns(id).await,
            (&Method::GET, ["bots", _, "balance"]) | (&Method::POST, ["bots", _, "unstuck"]) => {
                Err(ApiError::NotAvailable)
            }

            (&Method::GET, ["templates"]) => h.list_templates().await,
            (&Method::GET, ["templates", name]) => h.get_template(name).await,

            _ => Err(ApiError::NotFound),
        }
    }

    fn error_response(&self, error: ApiError) -> Response<Bytes> {
        match error {
            ApiError::Scope(scope) => insufficient_scope(&self.metadata, scope),
            ApiError::BadRequest(message) => {
                respond(StatusCode::BAD_REQUEST, json!({ "error": message }))
            }
            ApiError::NotFound => respond(StatusCode::NOT_FOUND, json!({ "error": "not found" })),
            ApiError::Conflict(body) => respond(StatusCode::CONFLICT, body),
            ApiError::Forbidden(body) => respond(StatusCode::FORBIDDEN, body),
            ApiError::Failed { message, retryable } => {
                let status = if retryable {
                    StatusCode::SERVICE_UNAVAILABLE
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                };
                respond(status, json!({ "error": message, "retryable": retryable }))
            }
            ApiError::NotAvailable => respond(
                StatusCode::NOT_IMPLEMENTED,
                json!({ "error": "not available yet" }),
            ),
        }
    }
}

/// The caller, once their scope is known to cover this route.
pub(crate) fn require(principal: &Principal, scope: &'static str) -> Result<(), ApiError> {
    if principal.has(scope) {
        Ok(())
    } else {
        Err(ApiError::Scope(scope))
    }
}

pub(crate) const READ: &str = SCOPE_READ;
pub(crate) const WRITE: &str = SCOPE_WRITE;

/// Parse a JSON body. serde's message names the field and the position, never
/// the value, so it is safe to return even for a body that carries a secret.
fn parse<T: DeserializeOwned>(body: &Bytes) -> Result<T, ApiError> {
    let body: &[u8] = if body.is_empty() { b"{}" } else { body };
    serde_json::from_slice(body).map_err(|e| ApiError::BadRequest(format!("invalid body: {e}")))
}

/// A JSON response. `no-store` because every answer here is either tenant data
/// or the outcome of a write, and neither belongs in a shared cache.
pub(crate) fn respond(status: StatusCode, value: Value) -> Response<Bytes> {
    let mut response = Response::new(Bytes::from(value.to_string()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
