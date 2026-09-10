//! MCP adapter: the bot-management use cases exposed as tools.
//!
//! A sibling of `interface::telegram`, not a layer above it — both drive the
//! same use cases, so a rule enforced in a use case (the exclusive start lock
//! above all) holds for a tool call exactly as it does for a button press.
//!
//! Three constraints shape what is here, and none of them are negotiable:
//!
//! 1. **No exchange credentials in a tool signature.** A tool argument lands in
//!    the model's context and in the transcript. Key entry stays on the Telegram
//!    path, so `add_bot` is absent rather than key-taking.
//! 2. **No whole-config overwrite.** `update_bot_config` replaces a live bot's
//!    position parameters wholesale; only named, validated fields are exposed.
//! 3. **Every write is audited** with principal, tool, bot and outcome.
//!
//! The launch path reuses `StartBotUseCase`, so a tool call claims the same
//! DynamoDB start lock a button does. A second launch path here would be a way
//! to give one bot two live trading tasks.

pub mod auth;
pub mod http;
pub mod oauth;
pub mod server;

use crate::usecase::*;
use std::sync::Arc;

pub use auth::{
    AuthError, Authenticator, LocalOperator, Principal, SCOPE_CONFIG_READ, SCOPE_READ, SCOPE_WRITE,
    StaticToken, TokenVerifier,
};
pub use http::HttpMcp;
pub use oauth::OAuthTokens;
pub use server::BotTools;

/// The use cases the tools drive.
///
/// Deliberately its own bundle rather than `telegram::Deps`: the two adapters
/// expose different surfaces (no add-bot dialogue here, no whole-config write),
/// and sharing one struct would make every field either adapter adds a field the
/// other silently gains. The REST surface embeds this one and adds only what a
/// tool must not offer at all — key entry, and signing up.
#[derive(Clone)]
pub struct Deps {
    pub list_bots_usecase: Arc<ListBotsUseCase>,
    pub delete_bot_usecase: Arc<DeleteBotUseCase>,
    pub list_templates_usecase: Arc<ListTemplatesUseCase>,
    pub apply_template_usecase: Arc<ApplyTemplateUseCase>,
    pub get_bot_config_usecase: Arc<GetBotConfigUseCase>,
    pub update_risk_level_usecase: Arc<UpdateRiskLevelUseCase>,
    pub set_strategy_side_usecase: Arc<SetStrategySideUseCase>,
    pub set_bot_runtime_usecase: Arc<SetBotRuntimeUseCase>,
    pub get_bot_runtime_usecase: Arc<GetBotRuntimeUseCase>,
    pub start_bot_usecase: Arc<StartBotUseCase>,
    pub stop_bot_usecase: Arc<StopBotUseCase>,
    pub get_template_usecase: Arc<GetTemplateUseCase>,
    /// `None` where no chart bucket is configured; the surface then says so
    /// rather than failing.
    pub get_bot_returns_usecase: Option<Arc<GetBotReturnsUseCase>>,
    pub list_identities_usecase: Arc<ListIdentitiesUseCase>,
    pub issue_bind_ticket_usecase: Arc<IssueTelegramBindTicketUseCase>,
    pub unbind_telegram_usecase: Arc<UnbindTelegramUseCase>,
    /// The bot's `@username`, for the deep link a bind ticket is handed out
    /// as. Empty hands out the token alone.
    pub bot_username: String,
}
