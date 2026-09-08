//! The OAuth client half: send a user to authenticate, and find out who came
//! back.
//!
//! Authorization code with PKCE. The code alone is not enough to exchange —
//! whoever presents it must also hold the verifier whose challenge was sent at
//! the start, which is what stops a code intercepted in a redirect from being
//! spent by someone else.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Who came back, as the authorization server describes them.
pub struct Account {
    /// The provider's own id for this person. Stable across email changes, which
    /// is why the link is keyed on it and not on the address.
    pub subject: String,
    pub email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Endpoints {
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    sub: String,
    email: Option<String>,
}

pub struct OAuthClient {
    endpoints: Endpoints,
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    scope: String,
    http: reqwest::Client,
}

impl OAuthClient {
    /// Read the issuer's endpoints once, at startup, so a misconfigured issuer
    /// fails the deployment rather than the first person to press the button.
    pub async fn discover(
        issuer: &str,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        redirect_uri: impl Into<String>,
    ) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;

        let endpoints: Endpoints = http
            .get(format!(
                "{}/.well-known/openid-configuration",
                issuer.trim_end_matches('/')
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        // This flow takes the subject from an unsigned `userinfo` body, so
        // whoever controls these three URLs controls which identity gets bound to
        // a ticket's tenant. Requiring the issuer's own origin is what keeps that
        // decision with the issuer.
        for endpoint in [
            &endpoints.authorization_endpoint,
            &endpoints.token_endpoint,
            &endpoints.userinfo_endpoint,
        ] {
            anyhow::ensure!(
                same_origin(issuer, endpoint),
                "{issuer} publishes an endpoint off its own origin: {endpoint}"
            );
        }

        Ok(Self {
            endpoints,
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            redirect_uri: redirect_uri.into(),
            // Only what identifies the person. This flow records who they are; it
            // never asks for a token that can act on anything of theirs.
            scope: "openid email".to_string(),
            http,
        })
    }

    pub fn authorize_url(&self, state: &str, code_verifier: &str) -> String {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", &self.redirect_uri)
            .append_pair("scope", &self.scope)
            .append_pair("state", state)
            .append_pair("code_challenge", &challenge(code_verifier))
            .append_pair("code_challenge_method", "S256")
            .finish();

        let separator = if self.endpoints.authorization_endpoint.contains('?') {
            '&'
        } else {
            '?'
        };
        format!(
            "{}{separator}{query}",
            self.endpoints.authorization_endpoint
        )
    }

    /// Spend the code and ask who it was for.
    pub async fn exchange(&self, code: &str, code_verifier: &str) -> anyhow::Result<Account> {
        let token: TokenResponse = self
            .http
            .post(&self.endpoints.token_endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", self.redirect_uri.as_str()),
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("code_verifier", code_verifier),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        // Asked for over TLS with the token we were just handed, rather than read
        // out of it: the subject then comes from the issuer answering a question,
        // not from a string this code would have to verify for itself.
        let user: UserInfo = self
            .http
            .get(&self.endpoints.userinfo_endpoint)
            .bearer_auth(&token.access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        anyhow::ensure!(!user.sub.is_empty(), "the issuer returned no subject");
        Ok(Account {
            subject: user.sub,
            email: user.email.filter(|e| !e.is_empty()),
        })
    }
}

/// Scheme, host and port together. The scheme is part of it, so an issuer
/// reached over https cannot publish an endpoint over anything else; that the
/// issuer itself is https is settled where it is configured.
fn same_origin(a: &str, b: &str) -> bool {
    match (reqwest::Url::parse(a), reqwest::Url::parse(b)) {
        (Ok(a), Ok(b)) => a.origin() == b.origin(),
        _ => false,
    }
}

/// The S256 challenge, per RFC 7636: base64url of the verifier's SHA-256, with
/// no padding.
fn challenge(code_verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_matches_the_worked_example_in_rfc_7636() {
        // Appendix B, so the encoding is checked against the specification rather
        // than against this implementation's own idea of it.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
