use std::collections::HashSet;

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
}
