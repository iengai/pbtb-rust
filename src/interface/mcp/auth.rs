use std::collections::HashSet;

use async_trait::async_trait;
use subtle::ConstantTimeEq;

use crate::domain::user::MAX_VIP_LEVEL;

/// Who is calling, resolved from the transport's credentials before any tool
/// runs.
///
/// 🔴 `user_id` is the tenant every row is keyed under, and it comes from here —
/// never from a tool argument. No tool takes a `user_id` parameter, so a caller
/// cannot name a tenant that is not theirs, and cross-tenant access stays a
/// thing the code cannot express rather than a thing a check has to catch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub user_id: String,
    pub scopes: HashSet<String>,
    /// The account's level, read with the account so every surface gates on
    /// the same number without a second lookup.
    pub vip_level: u8,
}

pub const SCOPE_READ: &str = "bots:read";
pub const SCOPE_WRITE: &str = "bots:write";

impl Principal {
    /// A principal holding both scopes and the top level. The stdio
    /// transport's only caller is the operator who started the process, and the
    /// shared bearer stands for the deployment's own account.
    pub fn full(user_id: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            scopes: HashSet::from([SCOPE_READ.to_string(), SCOPE_WRITE.to_string()]),
            vip_level: MAX_VIP_LEVEL,
        }
    }

    pub fn has(&self, scope: &str) -> bool {
        self.scopes.contains(scope)
    }
}

/// Resolves a transport's credentials into a [`Principal`].
///
/// The seam between transports: stdio trusts the process owner, HTTP will verify
/// a bearer token and map its subject onto a Telegram id. Tools depend on this
/// trait, so adding the HTTP implementation changes no tool.
pub trait Authenticator: Send + Sync {
    /// `None` when the caller cannot be identified. A caller that is not
    /// recognised is refused; nothing is provisioned for them.
    fn authenticate(&self) -> Option<Principal>;
}

/// Trusts whoever started the process, and serves exactly one tenant.
///
/// Sound only for stdio, where the transport IS the credential: the client is a
/// child process of a shell the operator already controls. Over a network this
/// would be an open door, which is why the HTTP transport takes its own
/// implementation rather than reusing this one.
pub struct LocalOperator {
    principal: Principal,
}

impl LocalOperator {
    pub fn new(user_id: impl Into<String>) -> Self {
        Self {
            principal: Principal::full(user_id),
        }
    }
}

impl Authenticator for LocalOperator {
    fn authenticate(&self) -> Option<Principal> {
        Some(self.principal.clone())
    }
}

/// Serves a principal the transport has already established.
///
/// The HTTP edge verifies a credential per request and then builds the tool
/// surface around the principal it resolved, so by the time a tool runs the
/// identity is settled and this only carries it. Keeping the check at the edge
/// is what lets tools stay ignorant of how anyone authenticated.
pub struct Verified(pub Principal);

impl Authenticator for Verified {
    fn authenticate(&self) -> Option<Principal> {
        Some(self.0.clone())
    }
}

/// Why a request was refused, and with it which status the caller sees.
///
/// The two are kept apart because they mean different things to a client: a 401
/// invites it to get a better token, a 403 tells it the token is fine and the
/// answer is still no. Collapsing them would send clients into a refresh loop
/// over an authorization decision no new token can change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// No credential, or one that does not verify. The client should
    /// authenticate and try again.
    Unauthenticated(String),
    /// A credential that verifies, belonging to nobody this server serves.
    Forbidden(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthenticated(why) | Self::Forbidden(why) => f.write_str(why),
        }
    }
}

/// Turns a presented bearer token into a [`Principal`].
///
/// The seam between "how a caller proves who they are" and everything that acts
/// on who they are. Async because a real implementation reaches a key set and a
/// mapping table to answer.
#[async_trait]
pub trait TokenVerifier: Send + Sync {
    async fn verify(&self, bearer: &str) -> Result<Principal, AuthError>;

    /// The subject a bearer verifies as, before any account lookup: what
    /// signing up needs, since the account is what does not exist yet. A
    /// transport whose credential names a deployment rather than a person has
    /// no subject to offer.
    async fn identify(&self, bearer: &str) -> Result<VerifiedSubject, AuthError> {
        let _ = bearer;
        Err(AuthError::Forbidden(
            "this transport does not identify subjects".into(),
        ))
    }
}

/// A verified identity-provider subject, whether or not an account holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSubject {
    pub subject: String,
    pub email: Option<String>,
}

/// One shared bearer token standing for one tenant.
///
/// The stopgap before per-user OAuth: possession of the token is the whole
/// claim, so it identifies a deployment rather than a person and cannot express
/// more than one caller. Rotating it is revocation for everybody at once.
pub struct StaticToken {
    token: String,
    principal: Principal,
}

impl StaticToken {
    pub fn new(token: impl Into<String>, user_id: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            principal: Principal::full(user_id),
        }
    }
}

#[async_trait]
impl TokenVerifier for StaticToken {
    /// Compared in constant time: a byte-wise early return leaks the length of
    /// the matching prefix, and a caller who can measure that can find the token
    /// one byte at a time instead of guessing all of it.
    ///
    /// A deployment that never had its token set refuses everyone, rather than
    /// admitting whoever sends the empty bearer.
    async fn verify(&self, bearer: &str) -> Result<Principal, AuthError> {
        if self.token.is_empty() {
            return Err(AuthError::Unauthenticated("no token is configured".into()));
        }
        if bool::from(bearer.as_bytes().ct_eq(self.token.as_bytes())) {
            Ok(self.principal.clone())
        } else {
            Err(AuthError::Unauthenticated("bearer does not match".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_principal_holds_both_scopes() {
        let p = Principal::full("5351347639");
        assert!(p.has(SCOPE_READ));
        assert!(p.has(SCOPE_WRITE));
        assert_eq!(p.user_id, "5351347639");
    }

    #[test]
    fn a_read_only_principal_cannot_write() {
        let p = Principal {
            user_id: "u".into(),
            scopes: HashSet::from([SCOPE_READ.to_string()]),
            vip_level: 0,
        };
        assert!(p.has(SCOPE_READ));
        assert!(!p.has(SCOPE_WRITE));
    }

    #[tokio::test]
    async fn a_static_token_admits_only_its_own_value() {
        let t = StaticToken::new("s3cret", "5351347639");
        assert_eq!(t.verify("s3cret").await, Ok(Principal::full("5351347639")));
        assert!(
            t.verify("s3cre").await.is_err(),
            "a prefix is not the token"
        );
        assert!(
            t.verify("s3crets").await.is_err(),
            "an extension is not the token"
        );
        assert!(t.verify("").await.is_err());
    }

    #[tokio::test]
    async fn an_empty_configured_token_admits_nobody() {
        // A misconfigured deployment must refuse everyone rather than accept the
        // empty bearer any client can send.
        assert!(StaticToken::new("", "u").verify("").await.is_err());
    }
}
