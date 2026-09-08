// src/config/link.rs
use serde::Deserialize;

/// The OAuth *client* half, used only by the account-linking flow.
///
/// Separate from `McpConfig` because the two are opposite roles against the same
/// authorization server: linking sends a user off to authenticate and needs
/// client credentials, while the MCP surface only ever checks a token someone
/// brings. The issuer is shared — one project, so the subject the link records
/// is the subject a token later presents.
#[derive(Debug, Deserialize, Default)]
pub struct LinkConfig {
    /// The bot's end of the flow: where the "Link account" button points.
    /// Empty hides the button, so nobody is offered a dead end.
    ///
    /// Env: APP__LINK__URL.
    #[serde(default)]
    pub url: String,

    /// Empty leaves the link flow off, and its routes unreachable.
    ///
    /// Env: APP__LINK__CLIENT_ID.
    #[serde(default)]
    pub client_id: String,

    /// SSM parameter holding the client secret, read once at cold start.
    ///
    /// Env: APP__LINK__CLIENT_SECRET_PARAM.
    #[serde(default)]
    pub client_secret_param: String,
}
