// src/config/mcp.rs
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct McpConfig {
    /// The Telegram user id the MCP server acts as. Every tool resolves its
    /// tenant from this, never from a tool argument.
    ///
    /// Empty by default because the telebot binary shares this config struct and
    /// has no use for it; both MCP binaries refuse to start without it, and
    /// refuse a value that is not on `APP__TELEGRAM__ALLOWED_USER_IDS` — so
    /// removing someone from the bot's allowlist takes their MCP access with it.
    ///
    /// Env: APP__MCP__USER_ID.
    #[serde(default)]
    pub user_id: String,

    /// SSM parameter holding the HTTP transport's bearer token, read once at
    /// cold start. The name rather than the value, so the secret is never in the
    /// function's environment, in Terraform state, or in a console page anyone
    /// with read access to the function can open.
    ///
    /// Env: APP__MCP__TOKEN_PARAM.
    #[serde(default)]
    pub token_param: String,
}
