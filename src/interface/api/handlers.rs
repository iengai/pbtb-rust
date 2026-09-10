//! The route bodies. Each one resolves the principal's scope, drives one use
//! case, and renders the outcome; the edge in `mod.rs` owns authentication,
//! routing and error rendering.

use std::str::FromStr;

use http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, Deps, READ, WRITE, require, respond};
use crate::domain::bot::Bot;
use crate::domain::engine::Runtime;
use crate::domain::identity::{LINK_TICKET_TTL, PROVIDER_TELEGRAM};
use crate::interface::describe;
use crate::interface::mcp::auth::Principal;
use crate::usecase::{AddOutcome, DeleteOutcome, SetRuntimeOutcome, StartOutcome, StopOutcome};

pub(super) struct Handlers<'a> {
    pub deps: &'a Deps,
    pub principal: &'a Principal,
}

#[derive(Deserialize)]
pub(super) struct AddBotBody {
    pub name: String,
    pub api_key: String,
    pub secret_key: String,
    /// Replace the keys of an existing bot of the same name. Off by default so
    /// a re-add never silently rotates keys; the caller sees a 409 first.
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Deserialize)]
pub(super) struct ConfirmBody {
    /// Must equal the bot id in the path. A delete drops the config and the
    /// exchange keys, so the target is named twice rather than once.
    #[serde(default)]
    pub confirm: String,
}

#[derive(Deserialize)]
pub(super) struct RiskBody {
    pub long: f64,
    pub short: f64,
}

#[derive(Deserialize)]
pub(super) struct SideBody {
    /// `long` or `short`.
    pub side: String,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub(super) struct RuntimeBody {
    /// `py` for the Python passivbot image, `rs` for pb-runner.
    pub runtime: String,
}

#[derive(Deserialize)]
pub(super) struct TemplateBody {
    pub name: String,
}

impl Handlers<'_> {
    fn user_id(&self) -> &str {
        &self.principal.user_id
    }

    fn vip_level(&self) -> u8 {
        self.principal.vip_level
    }

    /// Record a write with everything an audit needs: who, what, which bot, and
    /// how it ended. Emitted after the call so the outcome is real, not intended.
    fn audit(&self, route: &str, bot_id: &str, outcome: &str) {
        tracing::info!(
            principal = %self.principal.user_id,
            route,
            bot_id,
            outcome,
            "api write"
        );
    }

    fn ok(value: Value) -> ApiResult {
        Ok(respond(StatusCode::OK, value))
    }

    // ---------------------------------------------------------------- account

    pub async fn me(&self) -> ApiResult {
        require(self.principal, READ)?;
        let identities = self
            .deps
            .mcp
            .list_identities_usecase
            .execute(self.user_id())
            .await
            .map_err(|e| ApiError::from_domain("listing linked identities", e))?;
        let mut scopes: Vec<&String> = self.principal.scopes.iter().collect();
        scopes.sort();
        let telegram = identities
            .iter()
            .find(|(provider, _)| provider == PROVIDER_TELEGRAM)
            .map(|(_, subject)| subject.clone());
        Self::ok(json!({
            "user_id": self.user_id(),
            "vip_level": self.principal.vip_level,
            "scopes": scopes,
            "telegram": telegram,
            "identities": identities
                .into_iter()
                .map(|(provider, subject)| json!({ "provider": provider, "subject": subject }))
                .collect::<Vec<_>>(),
        }))
    }

    /// A one-time token that binds whichever Telegram account opens it to the
    /// caller's account. Handed out as the bot's deep link when the bot's
    /// username is known, and as the bare `/start` payload otherwise.
    pub async fn bind_ticket(&self) -> ApiResult {
        require(self.principal, WRITE)?;
        let token = self
            .deps
            .mcp
            .issue_bind_ticket_usecase
            .execute(self.user_id())
            .await
            .map_err(|e| {
                self.audit("bind_ticket", "-", "error");
                ApiError::from_domain("preparing the bind link", e)
            })?;
        self.audit("bind_ticket", "-", "issued");
        let username = self.deps.mcp.bot_username.trim().trim_start_matches('@');
        let url = (!username.is_empty()).then(|| format!("https://t.me/{username}?start={token}"));
        Self::ok(json!({
            "token": token,
            "url": url,
            "expires_in": LINK_TICKET_TTL,
        }))
    }

    /// Release the caller's Telegram id, so another can be bound.
    pub async fn unbind_telegram(&self) -> ApiResult {
        require(self.principal, WRITE)?;
        let released = self
            .deps
            .mcp
            .unbind_telegram_usecase
            .execute(self.user_id())
            .await
            .map_err(|e| {
                self.audit("unbind_telegram", "-", "error");
                ApiError::from_domain("unbinding telegram", e)
            })?;
        self.audit("unbind_telegram", "-", "released");
        Self::ok(json!({ "released": released }))
    }

    // ---------------------------------------------------------------- bots

    pub async fn list_bots(&self) -> ApiResult {
        require(self.principal, READ)?;
        let bots = self
            .deps
            .mcp
            .list_bots_usecase
            .execute(self.user_id())
            .await
            .map_err(|e| ApiError::from_domain("listing bots", e))?;

        let mut out = Vec::with_capacity(bots.len());
        for bot in bots {
            let phase = self.phase_of(&bot.id).await;
            out.push(describe::bot(&bot, phase));
        }
        Self::ok(json!({ "bots": out }))
    }

    /// One bot the way telebot's State view shows it: identity, desired and
    /// observed state, and the config described rather than dumped.
    pub async fn get_bot(&self, bot_id: &str) -> ApiResult {
        require(self.principal, READ)?;
        let bot = self.find_bot(bot_id).await?;
        let runtime = self
            .deps
            .mcp
            .get_bot_runtime_usecase
            .execute(self.user_id(), bot_id)
            .await
            .map_err(|e| ApiError::from_domain("reading the bot's status", e))?;

        // A bot with no config yet is a normal state — the one the add flow
        // leaves a bot in — so it renders as `null`, not as a fault.
        let config = match self
            .deps
            .mcp
            .get_bot_config_usecase
            .execute(self.user_id(), bot_id)
            .await
        {
            Ok(config) => Some(describe::config(&config)),
            Err(e) => {
                tracing::info!(bot_id, error = %e, "no config for the bot detail view");
                None
            }
        };

        let mut body = describe::bot(&bot, runtime.as_ref().map(|r| r.phase.as_str().to_string()));
        body["task_id"] = json!(runtime.as_ref().and_then(|r| r.task_id.clone()));
        body["observed_at"] = json!(runtime.as_ref().map(|r| r.observed_at));
        body["restarts"] = json!(runtime.as_ref().map(|r| r.version));
        body["config"] = json!(config);
        Self::ok(body)
    }

    /// Store a bot and its exchange keys. The one route on any surface that
    /// accepts a secret; the body is handed to the use case and nothing of it is
    /// logged or echoed.
    pub async fn add_bot(&self, body: AddBotBody) -> ApiResult {
        require(self.principal, WRITE)?;
        let name = body.name.trim().to_string();
        let api_key = body.api_key.trim().to_string();
        let secret_key = body.secret_key.trim().to_string();
        if name.is_empty() || api_key.is_empty() || secret_key.is_empty() {
            return Err(ApiError::BadRequest(
                "name, api_key and secret_key are all required".into(),
            ));
        }

        if body.overwrite {
            let bot = self
                .deps
                .add_bot_usecase
                .overwrite(self.user_id(), name, api_key, secret_key)
                .await
                .map_err(|e| {
                    self.audit("add_bot", "-", "error");
                    ApiError::from_domain("saving the bot", e)
                })?;
            self.audit("add_bot", &bot.id, "overwritten");
            return Self::ok(json!({ "status": "overwritten", "bot": describe::bot(&bot, None) }));
        }

        let outcome = self
            .deps
            .add_bot_usecase
            .execute(self.user_id(), name, api_key, secret_key)
            .await
            .map_err(|e| {
                self.audit("add_bot", "-", "error");
                ApiError::from_domain("saving the bot", e)
            })?;
        match outcome {
            AddOutcome::Added(bot) => {
                self.audit("add_bot", &bot.id, "added");
                Ok(respond(
                    StatusCode::CREATED,
                    json!({ "status": "added", "bot": describe::bot(&bot, None) }),
                ))
            }
            AddOutcome::AlreadyExists(existing) => {
                self.audit("add_bot", &existing.id, "already_exists");
                Err(ApiError::Conflict(json!({
                    "status": "already_exists",
                    "bot": describe::bot(&existing, None),
                })))
            }
        }
    }

    /// Delete a bot, its config and its stored exchange keys. Not reversible.
    pub async fn delete_bot(&self, bot_id: &str, body: ConfirmBody) -> ApiResult {
        require(self.principal, WRITE)?;
        self.find_bot(bot_id).await?;
        let outcome = self
            .deps
            .mcp
            .delete_bot_usecase
            .execute(self.user_id(), bot_id, &body.confirm)
            .await
            .map_err(|e| {
                self.audit("delete_bot", bot_id, "error");
                ApiError::from_domain("deleting the bot", e)
            })?;
        match outcome {
            DeleteOutcome::ConfirmMismatch => Err(ApiError::BadRequest(format!(
                "confirm must equal the bot id ({bot_id:?}) to delete it"
            ))),
            DeleteOutcome::Deleted => {
                self.audit("delete_bot", bot_id, "deleted");
                Self::ok(json!({ "status": "deleted", "bot_id": bot_id }))
            }
        }
    }

    /// Turn a bot on. Idempotent by construction: the launch claims the
    /// exclusive DynamoDB lock, so a second call while a task is starting or
    /// running launches nothing and says so.
    pub async fn start_bot(&self, bot_id: &str) -> ApiResult {
        require(self.principal, WRITE)?;
        let outcome = self
            .deps
            .mcp
            .start_bot_usecase
            .execute(self.user_id(), self.vip_level(), bot_id)
            .await
            .map_err(|e| {
                self.audit("start_bot", bot_id, "error");
                ApiError::from_domain("starting the bot", e)
            })?;
        let (status, body) = match &outcome {
            StartOutcome::Started { task_id } => (
                StatusCode::OK,
                json!({ "status": "started", "task_id": task_id }),
            ),
            StartOutcome::AlreadyRunning => {
                (StatusCode::OK, json!({ "status": "already_running" }))
            }
            StartOutcome::AlreadyStarting => {
                (StatusCode::OK, json!({ "status": "already_starting" }))
            }
            StartOutcome::Stopping => (
                StatusCode::CONFLICT,
                json!({ "status": "stopping", "retry": true }),
            ),
            StartOutcome::BotNotFound => {
                self.audit("start_bot", bot_id, "bot_not_found");
                return Err(ApiError::NotFound);
            }
        };
        self.audit(
            "start_bot",
            bot_id,
            body["status"].as_str().unwrap_or("unknown"),
        );
        Ok(respond(status, body))
    }

    /// Turn a bot off: clear the intent and stop its task.
    pub async fn stop_bot(&self, bot_id: &str) -> ApiResult {
        require(self.principal, WRITE)?;
        let outcome = self
            .deps
            .mcp
            .stop_bot_usecase
            .execute(self.user_id(), bot_id)
            .await
            .map_err(|e| {
                self.audit("stop_bot", bot_id, "error");
                ApiError::from_domain("stopping the bot", e)
            })?;
        let (status, body) = match &outcome {
            StopOutcome::Stopped { task_id } => (
                StatusCode::OK,
                json!({ "status": "stopped", "task_id": task_id }),
            ),
            StopOutcome::NotRunning => (StatusCode::OK, json!({ "status": "not_running" })),
            StopOutcome::AlreadyStopping => {
                (StatusCode::OK, json!({ "status": "already_stopping" }))
            }
            StopOutcome::StartInProgress => (
                StatusCode::CONFLICT,
                json!({ "status": "start_in_progress", "retry": true }),
            ),
            StopOutcome::BotNotFound => {
                self.audit("stop_bot", bot_id, "bot_not_found");
                return Err(ApiError::NotFound);
            }
        };
        self.audit(
            "stop_bot",
            bot_id,
            body["status"].as_str().unwrap_or("unknown"),
        );
        Ok(respond(status, body))
    }

    /// Set the per-side wallet exposure limits. Applies on the bot's next start.
    pub async fn set_risk(&self, bot_id: &str, body: RiskBody) -> ApiResult {
        require(self.principal, WRITE)?;
        self.find_bot(bot_id).await?;
        self.deps
            .mcp
            .update_risk_level_usecase
            .execute(self.user_id(), bot_id, body.long, body.short)
            .await
            .map_err(|e| {
                self.audit("set_risk_level", bot_id, "error");
                ApiError::from_domain("setting the risk level", e)
            })?;
        self.audit("set_risk_level", bot_id, "updated");
        Self::ok(json!({ "status": "updated", "risk": { "long": body.long, "short": body.short } }))
    }

    /// Enable or disable one side of the strategy. Applies on the next start.
    pub async fn set_side(&self, bot_id: &str, body: SideBody) -> ApiResult {
        require(self.principal, WRITE)?;
        if body.side != "long" && body.side != "short" {
            return Err(ApiError::BadRequest(
                "side must be `long` or `short`".into(),
            ));
        }
        self.find_bot(bot_id).await?;
        self.deps
            .mcp
            .set_strategy_side_usecase
            .execute(self.user_id(), bot_id, &body.side, body.enabled)
            .await
            .map_err(|e| {
                self.audit("set_strategy_side", bot_id, "error");
                ApiError::from_domain("setting the strategy side", e)
            })?;
        self.audit("set_strategy_side", bot_id, "updated");
        Self::ok(json!({ "status": "updated", "side": body.side, "enabled": body.enabled }))
    }

    /// Choose which image the bot launches on within its engine line. Takes
    /// effect on the next start; a running task keeps the binary it started with.
    pub async fn set_runtime(&self, bot_id: &str, body: RuntimeBody) -> ApiResult {
        require(self.principal, WRITE)?;
        let runtime =
            Runtime::from_str(&body.runtime).map_err(|e| ApiError::BadRequest(e.to_string()))?;
        let outcome = self
            .deps
            .mcp
            .set_bot_runtime_usecase
            .execute(self.user_id(), bot_id, runtime)
            .await
            .map_err(|e| {
                self.audit("set_bot_runtime", bot_id, "error");
                ApiError::from_domain("setting the bot runtime", e)
            })?;
        let body = match outcome {
            SetRuntimeOutcome::Updated { previous, runtime } => json!({
                "status": "updated",
                "previous": previous.as_str(),
                "runtime": runtime.as_str(),
            }),
            SetRuntimeOutcome::Unchanged { runtime } => {
                json!({ "status": "unchanged", "runtime": runtime.as_str() })
            }
            SetRuntimeOutcome::BotNotFound => {
                self.audit("set_bot_runtime", bot_id, "bot_not_found");
                return Err(ApiError::NotFound);
            }
        };
        self.audit(
            "set_bot_runtime",
            bot_id,
            body["status"].as_str().unwrap_or("unknown"),
        );
        Self::ok(body)
    }

    /// Switch a bot to a configuration template. Applies on its next start.
    pub async fn apply_template(&self, bot_id: &str, body: TemplateBody) -> ApiResult {
        require(self.principal, WRITE)?;
        self.find_bot(bot_id).await?;
        self.deps
            .mcp
            .apply_template_usecase
            .execute(self.user_id(), self.vip_level(), bot_id, &body.name)
            .await
            .map_err(|e| {
                self.audit("apply_template", bot_id, "error");
                ApiError::from_domain("applying the template", e)
            })?;
        self.audit("apply_template", bot_id, "applied");
        Self::ok(json!({ "status": "applied", "template_name": body.name }))
    }

    /// The bot's return series as the daily collector wrote it: a normalized
    /// index and cumulative return, no balances. Read under the caller's own
    /// tenant, so it is theirs or it is not found.
    pub async fn bot_returns(&self, bot_id: &str) -> ApiResult {
        require(self.principal, READ)?;
        self.find_bot(bot_id).await?;
        let Some(returns) = &self.deps.mcp.get_bot_returns_usecase else {
            return Err(ApiError::NotAvailable);
        };
        let series = returns
            .execute(self.user_id(), bot_id)
            .await
            .map_err(|e| ApiError::from_domain("reading the return curve", e))?
            .ok_or(ApiError::NotFound)?;
        Self::ok(series)
    }

    // ---------------------------------------------------------------- templates

    /// Every template with the level it asks for. Nothing is hidden by level:
    /// what a higher level unlocks is part of the catalogue.
    pub async fn list_templates(&self) -> ApiResult {
        require(self.principal, READ)?;
        let templates = self
            .deps
            .mcp
            .list_templates_usecase
            .execute()
            .await
            .map_err(|e| ApiError::from_domain("listing templates", e))?;
        Self::ok(
            json!({ "templates": templates.iter().map(describe::listing).collect::<Vec<_>>() }),
        )
    }

    /// A template described, never dumped: the parameters are what make the
    /// strategy worth running, and a caller who can read them can run them
    /// anywhere.
    pub async fn get_template(&self, name: &str) -> ApiResult {
        require(self.principal, READ)?;
        let preview = self
            .deps
            .mcp
            .get_template_usecase
            .execute(name)
            .await
            .map_err(|e| ApiError::from_domain("reading the template", e))?
            .ok_or(ApiError::NotFound)?;
        Self::ok(describe::template(&preview))
    }

    // ---------------------------------------------------------------- helpers

    /// The caller's bot by id, or `NotFound`. Looked up within the tenant, so a
    /// bot that exists elsewhere answers exactly like one that does not exist.
    async fn find_bot(&self, bot_id: &str) -> Result<Bot, ApiError> {
        self.deps
            .mcp
            .list_bots_usecase
            .execute(self.user_id())
            .await
            .map_err(|e| ApiError::from_domain("reading the bot", e))?
            .into_iter()
            .find(|b| b.id == bot_id)
            .ok_or(ApiError::NotFound)
    }

    /// A runtime-read failure degrades that bot's phase to `null` rather than
    /// failing a whole listing, and is recorded either way.
    async fn phase_of(&self, bot_id: &str) -> Option<String> {
        match self
            .deps
            .mcp
            .get_bot_runtime_usecase
            .execute(self.user_id(), bot_id)
            .await
        {
            Ok(runtime) => runtime.map(|r| r.phase.as_str().to_string()),
            Err(e) => {
                tracing::warn!(bot_id, error = %e, "runtime read failed for the bot list");
                None
            }
        }
    }
}
