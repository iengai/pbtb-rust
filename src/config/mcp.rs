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

    /// SSM parameter holding this server's own public URL, written by Terraform
    /// after the Function URL exists.
    ///
    /// Indirected through SSM rather than passed as an environment variable
    /// because a function cannot name the URL of the function it belongs to —
    /// Terraform would have to build the environment from a resource that
    /// depends on it.
    ///
    /// Env: APP__MCP__RESOURCE_URL_PARAM.
    #[serde(default)]
    pub resource_url_param: String,

    /// OAuth issuer to accept tokens from, e.g. `https://<project>.authkit.app`.
    ///
    /// Empty selects the shared-bearer transport instead. Set, it selects
    /// per-user OAuth: tokens are verified against the issuer's published keys
    /// and the subject is resolved through the identity table.
    ///
    /// Env: APP__MCP__ISSUER.
    #[serde(default)]
    pub issuer: String,
}
