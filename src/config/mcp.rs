// src/config/mcp.rs
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct McpConfig {
    /// The Telegram user id the stdio MCP server acts as. Every tool resolves
    /// its tenant from this, never from a tool argument.
    ///
    /// Empty by default because the telebot binary shares this config struct and
    /// has no use for it; `mcp_stdio` refuses to start without it, and refuses a
    /// value that is not on `APP__TELEGRAM__ALLOWED_USER_IDS` — so removing
    /// someone from the bot's allowlist takes their MCP access with it.
    ///
    /// Env: APP__MCP__USER_ID.
    #[serde(default)]
    pub user_id: String,
}
