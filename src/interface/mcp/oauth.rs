//! Bearer tokens issued by an OAuth authorization server.
//!
//! Three checks stand between a token and a tenant, and all three have to pass:
//! the signature and claims must verify against the issuer's published keys, the
//! subject must have been deliberately linked to a tenant, and that tenant must
//! still be on the telegram allowlist. The last one is what makes removing
//! someone from the bot remove them from here too, rather than leaving a second
//! door they still hold a key to.

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::bail;
use async_trait::async_trait;
use jsonwebtoken::jwk::{Jwk, JwkSet};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use tokio::sync::RwLock;

use super::auth::{AuthError, Principal, SCOPE_READ, SCOPE_WRITE, TokenVerifier};
use crate::domain::identity::{IdentityRepository, PROVIDER_WORKOS};

/// How long a fetched key set is trusted before an unknown `kid` is allowed to
/// trigger another fetch. Without a floor, a stream of tokens carrying invented
/// `kid`s would turn every request into a round trip to the issuer.
const MIN_REFETCH_INTERVAL: Duration = Duration::from_secs(60);

/// Tolerance for clock skew between this host and the issuer, in seconds.
const LEEWAY: u64 = 30;

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    /// Space-delimited, per RFC 6749. Absent on a token that never asked for
    /// any.
    #[serde(default)]
    scope: String,
}

#[derive(Debug, Deserialize)]
struct Discovery {
    issuer: String,
    jwks_uri: String,
}

struct Keys {
    set: JwkSet,
    fetched_at: Instant,
}

pub struct OAuthTokens {
    issuer: String,
    /// The resource this server is, and the only audience it accepts. A token
    /// minted for some other resource is a valid token — just not one for here,
    /// and honouring it would let any service the user also authorized replay
    /// their token against these bots.
    audience: String,
    jwks_uri: String,
    http: reqwest::Client,
    keys: RwLock<Keys>,
    identities: Arc<dyn IdentityRepository>,
    allowed_user_ids: HashSet<String>,
}

impl OAuthTokens {
    /// Discover the issuer's key set and hold it for the life of the process.
    ///
    /// Done at startup rather than on first use so a misconfigured issuer fails
    /// the deployment instead of every request.
    pub async fn discover(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        identities: Arc<dyn IdentityRepository>,
        allowed_user_ids: HashSet<String>,
    ) -> anyhow::Result<Self> {
        let issuer = issuer.into();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;

        let discovery: Discovery = http
            .get(format!(
                "{}/.well-known/openid-configuration",
                issuer.trim_end_matches('/')
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        // RFC 8414 §3.3: a document that names a different issuer than the one we
        // asked about is not this issuer's document, whatever served it.
        if discovery.issuer.trim_end_matches('/') != issuer.trim_end_matches('/') {
            bail!(
                "{issuer} publishes discovery for {} instead",
                discovery.issuer
            );
        }
        // The key set decides which signatures are genuine, so it has to come
        // from the issuer itself. A discovery document that points elsewhere is
        // handing that decision to whoever is at the other end.
        if !same_origin(&issuer, &discovery.jwks_uri) {
            bail!(
                "{issuer} publishes its key set off-origin at {}",
                discovery.jwks_uri
            );
        }

        let set = fetch_keys(&http, &discovery.jwks_uri).await?;

        Ok(Self {
            issuer,
            audience: audience.into(),
            jwks_uri: discovery.jwks_uri,
            http,
            keys: RwLock::new(Keys {
                set,
                fetched_at: Instant::now(),
            }),
            identities,
            allowed_user_ids,
        })
    }

    /// The key with this `kid`, refetching once if it is not one we hold.
    ///
    /// A `kid` we have never seen is what a key rotation looks like from here,
    /// so it is worth one fetch — but only one per interval, or an attacker
    /// picks the rate at which we call the issuer.
    async fn key_for(&self, kid: &str) -> Result<Jwk, AuthError> {
        if let Some(jwk) = self.keys.read().await.set.find(kid) {
            return Ok(jwk.clone());
        }

        let mut keys = self.keys.write().await;
        if let Some(jwk) = keys.set.find(kid) {
            return Ok(jwk.clone());
        }
        if keys.fetched_at.elapsed() < MIN_REFETCH_INTERVAL {
            return Err(AuthError::Unauthenticated(
                "token signed by an unknown key".into(),
            ));
        }

        // The attempt is what the interval counts, not the success. Advancing it
        // only on a 200 would mean an issuer that is down removes the throttle
        // entirely: every unknown key becomes another outbound call, which is a
        // retry storm aimed at the endpoint we are already failing to reach.
        keys.fetched_at = Instant::now();
        keys.set = fetch_keys(&self.http, &self.jwks_uri)
            .await
            .map_err(|e| AuthError::Unauthenticated(format!("key set unavailable: {e}")))?;

        keys.set
            .find(kid)
            .cloned()
            .ok_or_else(|| AuthError::Unauthenticated("token signed by an unknown key".into()))
    }
}

fn same_origin(a: &str, b: &str) -> bool {
    match (reqwest::Url::parse(a), reqwest::Url::parse(b)) {
        (Ok(a), Ok(b)) => a.origin() == b.origin(),
        _ => false,
    }
}

async fn fetch_keys(http: &reqwest::Client, jwks_uri: &str) -> anyhow::Result<JwkSet> {
    Ok(http
        .get(jwks_uri)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// The scopes a token actually carries, never more.
///
/// A token that asked for nothing gets nothing, and is refused by the first tool
/// it reaches. Handing it a floor of read access would make the scope claim
/// decorative: a token deliberately issued without `bots:read` would still list
/// every bot in the tenant and read its full trading config.
fn scopes_from(claim: &str) -> HashSet<String> {
    claim
        .split_whitespace()
        .filter(|s| *s == SCOPE_READ || *s == SCOPE_WRITE)
        .map(str::to_owned)
        .collect()
}

#[async_trait]
impl TokenVerifier for OAuthTokens {
    async fn verify(&self, bearer: &str) -> Result<Principal, AuthError> {
        let header = decode_header(bearer)
            .map_err(|e| AuthError::Unauthenticated(format!("malformed token: {e}")))?;
        let kid = header
            .kid
            .ok_or_else(|| AuthError::Unauthenticated("token names no signing key".into()))?;

        let jwk = self.key_for(&kid).await?;

        // The algorithm comes from the published key, never from the token's own
        // header: letting the token choose is how a signature check becomes an
        // attacker-supplied one.
        let algorithm = jwk
            .common
            .key_algorithm
            .and_then(|a| Algorithm::from_str(&a.to_string()).ok())
            .ok_or_else(|| {
                AuthError::Unauthenticated("the signing key publishes no usable algorithm".into())
            })?;

        // A key set is public, so an HMAC entry in one would publish the signing
        // secret: anyone who can read the document could then mint tokens for any
        // subject. Only signatures made with a key the issuer kept count.
        if matches!(
            algorithm,
            Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512
        ) {
            return Err(AuthError::Unauthenticated(
                "the key set publishes a symmetric key".into(),
            ));
        }

        let key = DecodingKey::from_jwk(&jwk)
            .map_err(|e| AuthError::Unauthenticated(format!("unusable signing key: {e}")))?;

        let mut validation = Validation::new(algorithm);
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.audience]);
        // Naming an expected value only rejects a claim that is present and
        // wrong; a claim that is absent is compared against nothing and passes.
        // Requiring all four is what turns "this token is not for us" from a
        // check into a guarantee — without it, any token the issuer minted
        // without an `aud` works here, whoever it was minted for.
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        // Only applied when the claim is present, which is the right shape: `nbf`
        // is optional, but a token that carries one is not valid before it.
        validation.validate_nbf = true;
        validation.leeway = LEEWAY;

        let claims = decode::<Claims>(bearer, &key, &validation)
            .map_err(|e| AuthError::Unauthenticated(format!("token rejected: {e}")))?
            .claims;

        let link = self
            .identities
            .find_link(PROVIDER_WORKOS, &claims.sub)
            .await
            .map_err(|e| AuthError::Forbidden(format!("identity lookup failed: {e}")))?
            .ok_or_else(|| {
                // Authenticating with the provider is not an application for an
                // account. An unlinked subject is refused, not provisioned.
                AuthError::Forbidden("this identity is not linked to any account".into())
            })?;

        if !self.allowed_user_ids.contains(&link.user_id) {
            return Err(AuthError::Forbidden(
                "the linked account is not on the allowlist".into(),
            ));
        }

        Ok(Principal {
            user_id: link.user_id,
            scopes: scopes_from(&claims.scope),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_gets_the_scopes_it_asked_for() {
        assert_eq!(
            scopes_from("bots:read bots:write"),
            HashSet::from([SCOPE_READ.to_string(), SCOPE_WRITE.to_string()])
        );
        assert_eq!(
            scopes_from("bots:read"),
            HashSet::from([SCOPE_READ.to_string()])
        );
    }

    #[test]
    fn scopes_this_server_does_not_define_are_dropped() {
        assert_eq!(
            scopes_from("bots:write openid email"),
            HashSet::from([SCOPE_WRITE.to_string()]),
            "an unknown scope must not widen what the token can do"
        );
    }

    #[test]
    fn a_token_that_asked_for_nothing_gets_nothing() {
        assert!(scopes_from("").is_empty());
        assert!(
            scopes_from("openid profile email").is_empty(),
            "scopes for some other API are not a claim on this one"
        );
    }
}
