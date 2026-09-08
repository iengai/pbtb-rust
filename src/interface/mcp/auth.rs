use std::collections::HashSet;

use subtle::ConstantTimeEq;

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
}

pub const SCOPE_READ: &str = "bots:read";
pub const SCOPE_WRITE: &str = "bots:write";

impl Principal {
    /// A principal holding both scopes. The stdio transport's only caller is the
    /// operator who started the process.
    pub fn full(user_id: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            scopes: HashSet::from([SCOPE_READ.to_string(), SCOPE_WRITE.to_string()]),
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

    /// The presented bearer, or `None` when it does not match.
    ///
    /// Compared in constant time: a byte-wise early return leaks the length of
    /// the matching prefix, and a caller who can measure that can find the token
    /// one byte at a time instead of guessing all of it.
    ///
    /// A deployment that never had its token set refuses everyone, rather than
    /// admitting whoever sends the empty bearer.
    pub fn verify(&self, presented: &str) -> Option<Principal> {
        if self.token.is_empty() {
            return None;
        }
        bool::from(presented.as_bytes().ct_eq(self.token.as_bytes()))
            .then(|| self.principal.clone())
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
        };
        assert!(p.has(SCOPE_READ));
        assert!(!p.has(SCOPE_WRITE));
    }

    #[test]
    fn a_static_token_admits_only_its_own_value() {
        let t = StaticToken::new("s3cret", "5351347639");
        assert_eq!(t.verify("s3cret"), Some(Principal::full("5351347639")));
        assert!(t.verify("s3cre").is_none(), "a prefix is not the token");
        assert!(
            t.verify("s3crets").is_none(),
            "an extension is not the token"
        );
        assert!(t.verify("").is_none());
    }

    #[test]
    fn an_empty_configured_token_admits_nobody() {
        // A misconfigured deployment must refuse everyone rather than accept the
        // empty bearer any client can send.
        assert!(StaticToken::new("", "u").verify("").is_none());
    }
}
