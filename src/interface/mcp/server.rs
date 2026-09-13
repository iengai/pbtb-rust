use std::sync::Arc;

use rmcp::ErrorData as McpError;
use rmcp::ServerHandler;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::Deps;
use super::auth::{Authenticator, Principal, SCOPE_CONFIG_READ, SCOPE_READ, SCOPE_WRITE};
use crate::domain::bot::Bot;
use crate::domain::engine::Runtime;
use crate::domain::error::DomainError;
use crate::domain::identity::{LINK_TICKET_TTL, PROVIDER_TELEGRAM};
use crate::interface::describe;
use crate::interface::redaction::redact;
use crate::usecase::{DeleteOutcome, SetRuntimeOutcome, StartOutcome, StopOutcome};
use std::str::FromStr;

/// Identifies one bot. No tool takes a `user_id`: the tenant comes from the
/// authenticated principal, so a caller can only ever name a bot within their
/// own tenant.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct BotRef {
    /// The bot's id, as returned by `list_bots`.
    pub bot_id: String,
}

/// Identifies one configuration template.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TemplateRef {
    /// A template name from `list_templates`.
    pub template_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ApplyTemplateArgs {
    pub bot_id: String,
    /// A template name from `list_templates`.
    pub template_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RiskArgs {
    pub bot_id: String,
    /// Long-side wallet exposure limit.
    pub risk_long: f64,
    /// Short-side wallet exposure limit.
    pub risk_short: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StrategySideArgs {
    pub bot_id: String,
    /// `long` or `short`.
    pub side: String,
    pub enabled: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RuntimeArgs {
    pub bot_id: String,
    /// `py` for the Python passivbot image, `rs` for pb-runner.
    pub runtime: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteArgs {
    pub bot_id: String,
    /// Must equal `bot_id`. Deleting a bot drops its config and its exchange
    /// keys, so the caller has to name the target twice rather than delete on a
    /// single mistaken argument.
    pub confirm: String,
}

/// What a client is told the server is for, ahead of any tool call. It names the
/// two boundaries a caller would otherwise discover by hitting them: there is no
/// way to reach another tenant, and keys are not entered here.
const INSTRUCTIONS: &str = concat!(
    "Manage passivbot trading bots. Every tool acts on the authenticated caller's own ",
    "bots; there is no way to name another tenant. Exchange API keys are never accepted ",
    "or returned here - add a bot and enter its keys in the Telegram bot or the web ",
    "console. Config changes apply on a bot's next start.",
);

#[derive(Clone)]
pub struct BotTools {
    deps: Deps,
    auth: Arc<dyn Authenticator>,
}

impl BotTools {
    pub fn new(deps: Deps, auth: Arc<dyn Authenticator>) -> Self {
        Self { deps, auth }
    }

    /// The caller, once their scope is known to cover this call.
    fn principal(&self, scope: &str) -> Result<Principal, McpError> {
        let Some(principal) = self.auth.authenticate() else {
            return Err(McpError::invalid_request(
                "not authenticated: this server serves no anonymous caller",
                None,
            ));
        };
        if !principal.has(scope) {
            return Err(McpError::invalid_request(
                format!("this token does not carry the {scope} scope"),
                None,
            ));
        }
        Ok(principal)
    }

    /// Record a write with everything an audit needs: who, what, which bot, and
    /// how it ended. Emitted after the call so the outcome is real, not intended.
    fn audit(principal: &Principal, tool: &str, bot_id: &str, outcome: &str) {
        tracing::info!(
            principal = %principal.user_id,
            tool,
            bot_id,
            outcome,
            "mcp write"
        );
    }

    fn ok(value: serde_json::Value) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![ContentBlock::text(
            value.to_string(),
        )]))
    }

    /// The caller's bot by id. Looked up within the tenant, so a bot that
    /// exists under another one answers exactly like a bot that does not exist.
    async fn find_bot(&self, user_id: &str, bot_id: &str) -> Result<Bot, McpError> {
        self.deps
            .list_bots_usecase
            .execute(user_id)
            .await
            .map_err(|e| failed("reading the bot", e))?
            .into_iter()
            .find(|b| b.id == bot_id)
            .ok_or_else(|| McpError::resource_not_found(format!("no bot {bot_id:?}"), None))
    }
}

/// A use-case fault as a tool error, redacted the same way a chat reply is: the
/// consequence and a correlation id, never the cause. A tool result is read by a
/// model and kept in a transcript, so leaking an internal chain here is worse
/// than in a chat, not better.
fn failed(action: &str, err: DomainError) -> McpError {
    McpError::internal_error(redact(action, &err), None)
}

#[tool_router(vis = "pub")]
impl BotTools {
    /// List the caller's bots with their observed runtime phase.
    #[tool(annotations(read_only_hint = true))]
    pub async fn list_bots(&self) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_READ)?;
        let bots = self
            .deps
            .list_bots_usecase
            .execute(&principal.user_id)
            .await
            .map_err(|e| failed("listing bots", e))?;

        let mut out = Vec::with_capacity(bots.len());
        for bot in bots {
            // A runtime-read failure degrades that bot's phase to null rather
            // than failing the whole listing, and is recorded either way.
            let phase = match self
                .deps
                .get_bot_runtime_usecase
                .execute(&principal.user_id, &bot.id)
                .await
            {
                Ok(runtime) => runtime.map(|r| r.phase.as_str().to_string()),
                Err(e) => {
                    tracing::warn!(bot_id = %bot.id, error = %e, "runtime read failed for list_bots");
                    None
                }
            };
            // No api_key or secret_key: they are never part of a tool result.
            out.push(json!({
                "bot_id": bot.id,
                "name": bot.name,
                "exchange": bot.exchange.as_str(),
                "enabled": bot.enabled,
                "runtime": bot.runtime.as_str(),
                "phase": phase,
            }));
        }
        Self::ok(json!({ "bots": out }))
    }

    /// Who this token is: the account it resolves to, the level and scopes it
    /// carries, and the identities linked to it.
    #[tool(annotations(read_only_hint = true))]
    pub async fn whoami(&self) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_READ)?;
        let identities = self
            .deps
            .list_identities_usecase
            .execute(&principal.user_id)
            .await
            .map_err(|e| failed("listing linked identities", e))?;
        let mut scopes: Vec<&String> = principal.scopes.iter().collect();
        scopes.sort();
        let telegram = identities
            .iter()
            .find(|(provider, _)| provider == PROVIDER_TELEGRAM)
            .map(|(_, subject)| subject.clone());
        Self::ok(json!({
            "user_id": principal.user_id,
            "vip_level": principal.vip_level,
            "scopes": scopes,
            "telegram": telegram,
            "identities": identities
                .into_iter()
                .map(|(provider, subject)| json!({ "provider": provider, "subject": subject }))
                .collect::<Vec<_>>(),
        }))
    }

    /// One bot in full: identity, desired and observed state, and its config
    /// described — sides, coins, risk, the strategy it runs. The parameters
    /// themselves are `get_bot_config`'s, not this tool's.
    #[tool(annotations(read_only_hint = true))]
    pub async fn describe_bot(
        &self,
        Parameters(args): Parameters<BotRef>,
    ) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_READ)?;
        let bot = self.find_bot(&principal.user_id, &args.bot_id).await?;
        let runtime = self
            .deps
            .get_bot_runtime_usecase
            .execute(&principal.user_id, &args.bot_id)
            .await
            .map_err(|e| failed("reading the bot's status", e))?;

        // A bot with no config yet is a normal state — the one the add flow
        // leaves a bot in — so it renders as `null`, not as a fault.
        let config = match self
            .deps
            .get_bot_config_usecase
            .execute(&principal.user_id, &args.bot_id)
            .await
        {
            Ok(config) => Some(describe::config(&config)),
            Err(e) => {
                tracing::info!(bot_id = %args.bot_id, error = %e, "no config to describe");
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

    /// The bot's return series as the daily collector wrote it: a normalized
    /// index and cumulative return, no balances.
    #[tool(annotations(read_only_hint = true))]
    pub async fn get_bot_returns(
        &self,
        Parameters(args): Parameters<BotRef>,
    ) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_READ)?;
        self.find_bot(&principal.user_id, &args.bot_id).await?;
        let Some(returns) = &self.deps.get_bot_returns_usecase else {
            return Err(McpError::internal_error(
                "this deployment collects no return series: it has no chart bucket configured",
                None,
            ));
        };
        let series = returns
            .execute(&principal.user_id, &args.bot_id)
            .await
            .map_err(|e| failed("reading the return curve", e))?
            .ok_or_else(|| {
                McpError::resource_not_found(
                    format!("no series has been collected for {:?} yet", args.bot_id),
                    None,
                )
            })?;
        Self::ok(series)
    }

    /// The observed runtime state of one bot: phase, task id and restart
    /// generation.
    #[tool(annotations(read_only_hint = true))]
    pub async fn get_bot_status(
        &self,
        Parameters(args): Parameters<BotRef>,
    ) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_READ)?;
        let runtime = self
            .deps
            .get_bot_runtime_usecase
            .execute(&principal.user_id, &args.bot_id)
            .await
            .map_err(|e| failed("reading the bot's status", e))?;

        match runtime {
            Some(r) => Self::ok(json!({
                "bot_id": args.bot_id,
                "phase": r.phase.as_str(),
                "task_id": r.task_id,
                "version": r.version,
                "observed_at": r.observed_at,
            })),
            None => Self::ok(json!({ "bot_id": args.bot_id, "phase": null })),
        }
    }

    /// The bot's stored passivbot configuration, parameters included. Needs
    /// the `config:read` scope, which `bots:read` does not imply.
    #[tool(annotations(read_only_hint = true))]
    pub async fn get_bot_config(
        &self,
        Parameters(args): Parameters<BotRef>,
    ) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_CONFIG_READ)?;
        let config = self
            .deps
            .get_bot_config_usecase
            .execute(&principal.user_id, &args.bot_id)
            .await
            .map_err(|e| failed("reading the bot's config", e))?;

        Self::ok(json!({
            "bot_id": config.bot_id,
            "template_name": config.template_name,
            "template_version": config.template_version,
            "updated_at": config.updated_at,
            "config": config.config_data,
        }))
    }

    /// The configuration templates a bot can be switched to, each with its
    /// title and the VIP level it asks for (`apply_template` refuses one above
    /// the caller's). A template addressed to the operator is listed to the
    /// operator's account only.
    #[tool(annotations(read_only_hint = true))]
    pub async fn list_templates(&self) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_READ)?;
        let templates = self
            .deps
            .list_templates_usecase
            .execute(principal.role)
            .await
            .map_err(|e| failed("listing templates", e))?;
        let templates: Vec<Value> = templates
            .iter()
            .map(crate::interface::describe::listing)
            .collect();
        Self::ok(json!({ "templates": templates }))
    }

    /// One template described — what it trades, which sides, the level it asks
    /// for. Its parameters stay on the server.
    #[tool(annotations(read_only_hint = true))]
    pub async fn describe_template(
        &self,
        Parameters(args): Parameters<TemplateRef>,
    ) -> Result<CallToolResult, McpError> {
        self.principal(SCOPE_READ)?;
        let preview = self
            .deps
            .get_template_usecase
            .execute(&args.template_name)
            .await
            .map_err(|e| failed("reading the template", e))?
            .ok_or_else(|| {
                McpError::resource_not_found(format!("no template {:?}", args.template_name), None)
            })?;
        Self::ok(describe::template(&preview))
    }

    /// Mint a one-time link that binds a Telegram account to the caller's own
    /// account.
    ///
    /// The URL is a credential for that account and is spent by whoever opens
    /// it first: hand it to the account holder to open themselves, and to
    /// nobody else.
    #[tool]
    pub async fn issue_telegram_bind_ticket(&self) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_WRITE)?;
        let token = self
            .deps
            .issue_bind_ticket_usecase
            .execute(&principal.user_id)
            .await
            .map_err(|e| {
                Self::audit(&principal, "issue_telegram_bind_ticket", "-", "error");
                failed("preparing the bind link", e)
            })?;
        Self::audit(&principal, "issue_telegram_bind_ticket", "-", "issued");
        // Without the bot's username there is no deep link to build, so the
        // bare `/start` payload is what the caller gets.
        let username = self.deps.bot_username.trim().trim_start_matches('@');
        let url = (!username.is_empty()).then(|| format!("https://t.me/{username}?start={token}"));
        Self::ok(json!({
            "token": token,
            "url": url,
            "expires_in": LINK_TICKET_TTL,
        }))
    }

    /// Release the caller's Telegram id, so another can be bound.
    #[tool(annotations(idempotent_hint = true))]
    pub async fn unbind_telegram(&self) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_WRITE)?;
        let released = self
            .deps
            .unbind_telegram_usecase
            .execute(&principal.user_id)
            .await
            .map_err(|e| {
                Self::audit(&principal, "unbind_telegram", "-", "error");
                failed("unbinding telegram", e)
            })?;
        Self::audit(&principal, "unbind_telegram", "-", "released");
        Self::ok(json!({ "released": released }))
    }

    /// Turn a bot on: record the intent and launch its task if none is running.
    ///
    /// Idempotent by construction — the launch claims an exclusive DynamoDB
    /// lock, so calling this while a task is starting or running launches
    /// nothing and says so.
    #[tool(annotations(idempotent_hint = true))]
    pub async fn start_bot(
        &self,
        Parameters(args): Parameters<BotRef>,
    ) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_WRITE)?;
        let outcome = self
            .deps
            .start_bot_usecase
            .execute(&principal.user_id, principal.vip_level, &args.bot_id)
            .await
            .map_err(|e| {
                Self::audit(&principal, "start_bot", &args.bot_id, "error");
                failed("starting the bot", e)
            })?;

        let body = match &outcome {
            StartOutcome::Started { task_id } => {
                json!({ "status": "started", "task_id": task_id })
            }
            StartOutcome::AlreadyRunning => json!({ "status": "already_running" }),
            StartOutcome::AlreadyStarting => json!({ "status": "already_starting" }),
            StartOutcome::Stopping => json!({ "status": "stopping", "retry": true }),
            StartOutcome::BotNotFound => json!({ "status": "bot_not_found" }),
        };
        Self::audit(
            &principal,
            "start_bot",
            &args.bot_id,
            body["status"].as_str().unwrap_or("unknown"),
        );
        Self::ok(body)
    }

    /// Turn a bot off: clear the intent and stop its task.
    #[tool(annotations(idempotent_hint = true))]
    pub async fn stop_bot(
        &self,
        Parameters(args): Parameters<BotRef>,
    ) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_WRITE)?;
        let outcome = self
            .deps
            .stop_bot_usecase
            .execute(&principal.user_id, &args.bot_id)
            .await
            .map_err(|e| {
                Self::audit(&principal, "stop_bot", &args.bot_id, "error");
                failed("stopping the bot", e)
            })?;

        let body = match &outcome {
            StopOutcome::Stopped { task_id } => json!({ "status": "stopped", "task_id": task_id }),
            StopOutcome::NotRunning => json!({ "status": "not_running" }),
            StopOutcome::StartInProgress => json!({ "status": "start_in_progress", "retry": true }),
            StopOutcome::AlreadyStopping => json!({ "status": "already_stopping" }),
            StopOutcome::BotNotFound => json!({ "status": "bot_not_found" }),
        };
        Self::audit(
            &principal,
            "stop_bot",
            &args.bot_id,
            body["status"].as_str().unwrap_or("unknown"),
        );
        Self::ok(body)
    }

    /// Switch a bot to a configuration template. Applies on its next start.
    #[tool]
    pub async fn apply_template(
        &self,
        Parameters(args): Parameters<ApplyTemplateArgs>,
    ) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_WRITE)?;
        self.deps
            .apply_template_usecase
            .execute(
                &principal.user_id,
                principal.vip_level,
                principal.role,
                &args.bot_id,
                &args.template_name,
            )
            .await
            .map_err(|e| {
                Self::audit(&principal, "apply_template", &args.bot_id, "error");
                failed("applying the template", e)
            })?;
        Self::audit(&principal, "apply_template", &args.bot_id, "applied");
        Self::ok(json!({ "status": "applied", "template_name": args.template_name }))
    }

    /// Set the per-side wallet exposure limits. Applies on the bot's next start.
    #[tool]
    pub async fn set_risk_level(
        &self,
        Parameters(args): Parameters<RiskArgs>,
    ) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_WRITE)?;
        self.deps
            .update_risk_level_usecase
            .execute(
                &principal.user_id,
                &args.bot_id,
                args.risk_long,
                args.risk_short,
            )
            .await
            .map_err(|e| {
                Self::audit(&principal, "set_risk_level", &args.bot_id, "error");
                failed("setting the risk level", e)
            })?;
        Self::audit(&principal, "set_risk_level", &args.bot_id, "updated");
        Self::ok(json!({
            "status": "updated",
            "risk_long": args.risk_long,
            "risk_short": args.risk_short,
        }))
    }

    /// Enable or disable one side of the strategy. Applies on the next start.
    #[tool]
    pub async fn set_strategy_side(
        &self,
        Parameters(args): Parameters<StrategySideArgs>,
    ) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_WRITE)?;
        self.deps
            .set_strategy_side_usecase
            .execute(&principal.user_id, &args.bot_id, &args.side, args.enabled)
            .await
            .map_err(|e| {
                Self::audit(&principal, "set_strategy_side", &args.bot_id, "error");
                failed("setting the strategy side", e)
            })?;
        Self::audit(&principal, "set_strategy_side", &args.bot_id, "updated");
        Self::ok(json!({ "status": "updated", "side": args.side, "enabled": args.enabled }))
    }

    /// Choose which image the bot launches on within its engine line. Takes
    /// effect on the next start; a running task keeps the binary it started with.
    #[tool]
    pub async fn set_bot_runtime(
        &self,
        Parameters(args): Parameters<RuntimeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_WRITE)?;
        let runtime = Runtime::from_str(&args.runtime)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let outcome = self
            .deps
            .set_bot_runtime_usecase
            .execute(&principal.user_id, &args.bot_id, runtime)
            .await
            .map_err(|e| {
                Self::audit(&principal, "set_bot_runtime", &args.bot_id, "error");
                failed("setting the bot runtime", e)
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
            SetRuntimeOutcome::BotNotFound => json!({ "status": "bot_not_found" }),
        };
        Self::audit(
            &principal,
            "set_bot_runtime",
            &args.bot_id,
            body["status"].as_str().unwrap_or("unknown"),
        );
        Self::ok(body)
    }

    /// Delete a bot, its config and its stored exchange keys. Not reversible.
    #[tool(annotations(destructive_hint = true, idempotent_hint = false))]
    pub async fn delete_bot(
        &self,
        Parameters(args): Parameters<DeleteArgs>,
    ) -> Result<CallToolResult, McpError> {
        let principal = self.principal(SCOPE_WRITE)?;
        let outcome = self
            .deps
            .delete_bot_usecase
            .execute(&principal.user_id, &args.bot_id, &args.confirm)
            .await
            .map_err(|e| {
                Self::audit(&principal, "delete_bot", &args.bot_id, "error");
                failed("deleting the bot", e)
            })?;
        match outcome {
            DeleteOutcome::ConfirmMismatch => Err(McpError::invalid_params(
                format!("confirm must equal bot_id ({:?}) to delete it", args.bot_id),
                None,
            )),
            DeleteOutcome::Deleted => {
                Self::audit(&principal, "delete_bot", &args.bot_id, "deleted");
                Self::ok(json!({ "status": "deleted", "bot_id": args.bot_id }))
            }
        }
    }
}

#[tool_handler]
impl ServerHandler for BotTools {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        // The default names the SDK, which tells a client nothing about which
        // server it reached when several are configured.
        let mut server = Implementation::from_build_env();
        server.name = env!("CARGO_PKG_NAME").to_string();
        server.version = env!("CARGO_PKG_VERSION").to_string();
        info.server_info = server;
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.instructions = Some(INSTRUCTIONS.to_string());
        info
    }
}
