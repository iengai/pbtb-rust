//! The account behind a tenant.
//!
//! A `user_id` is the tenant boundary everywhere else in the system; this is
//! the row that says the tenant exists and what it is entitled to. External
//! identities (a WorkOS subject, a Telegram id) map onto it through
//! [`crate::domain::identity`] and can come and go; the id itself is opaque
//! and ours, so no identity provider's lifecycle is the account's lifecycle.

use crate::domain::error::DomainError;
use async_trait::async_trait;

/// The highest level there is. Levels are compared numerically and nothing
/// else: a higher level unlocks whatever a lower one does.
pub const MAX_VIP_LEVEL: u8 = 9;

/// The level a fresh account starts at.
pub const DEFAULT_VIP_LEVEL: u8 = 0;

/// What an account may do beyond its own tenant. `Operator` is the person who
/// runs this deployment: the account whose bots may be put on the public
/// showcase page and the one offered the operator-only templates. Every
/// surface gates those on this row attribute, read with the account (the
/// Telegram sender, the API's and the MCP's principal); the WorkOS org role of
/// the same name (docs/workos.md) governs a token's scopes only. It is set by
/// the operator's own hand (`pbtb_ops.py set-role`), never from a client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Role {
    #[default]
    Member,
    Operator,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Member => "member",
            Role::Operator => "operator",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "member" => Some(Role::Member),
            "operator" => Some(Role::Operator),
            _ => None,
        }
    }

    pub fn is_operator(&self) -> bool {
        *self == Role::Operator
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserStatus {
    Active,
    /// Turned away at every surface, without the account or its data being
    /// removed. The way back is an operator flipping it, not the user.
    Suspended,
}

impl UserStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserStatus::Active => "active",
            UserStatus::Suspended => "suspended",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "active" => Some(UserStatus::Active),
            "suspended" => Some(UserStatus::Suspended),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub vip_level: u8,
    pub status: UserStatus,
    pub role: Role,
    /// The address the primary identity signed up with, kept for the operator
    /// to recognise an account by. Never used for authentication.
    pub email: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl User {
    /// A new account: level [`DEFAULT_VIP_LEVEL`], active.
    pub fn new(id: String, email: Option<String>, now: i64) -> Self {
        Self {
            id,
            vip_level: DEFAULT_VIP_LEVEL,
            status: UserStatus::Active,
            role: Role::Member,
            email,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn is_active(&self) -> bool {
        self.status == UserStatus::Active
    }
}

/// A fresh opaque account id.
///
/// Deliberately not a provider's subject and not a Telegram id: both are
/// someone else's namespace, and one of them changes per WorkOS environment.
/// Hex without dashes so it is safe as a DynamoDB key fragment, an S3 prefix
/// and an ECS environment value without escaping anywhere.
pub fn new_user_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// A level outside `0..=MAX_VIP_LEVEL`, refused before it reaches storage.
pub fn validate_vip_level(level: u8) -> Result<u8, DomainError> {
    if level > MAX_VIP_LEVEL {
        return Err(DomainError::InvalidConfig(format!(
            "vip level {level} out of range [0, {MAX_VIP_LEVEL}]"
        )));
    }
    Ok(level)
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    /// The account, or `None` when no such tenant was ever created.
    async fn find_user(&self, user_id: &str) -> Result<Option<User>, DomainError>;

    /// Create the account. `false` when one with this id already exists, in
    /// which case nothing is written: an id is minted once and never reused, so
    /// a collision is a bug to surface rather than a row to overwrite.
    async fn create_user(&self, user: &User) -> Result<bool, DomainError>;

    /// Set the level. `false` when there is no such account.
    async fn set_vip_level(&self, user_id: &str, level: u8, now: i64) -> Result<bool, DomainError>;

    /// Set the role. `false` when there is no such account.
    async fn set_role(&self, user_id: &str, role: Role, now: i64) -> Result<bool, DomainError>;

    /// Set the status. `false` when there is no such account.
    async fn set_status(
        &self,
        user_id: &str,
        status: UserStatus,
        now: i64,
    ) -> Result<bool, DomainError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_account_starts_at_level_zero_and_active() {
        let user = User::new("u-1".to_string(), None, 7);
        assert_eq!(user.vip_level, 0);
        assert!(user.is_active());
        assert_eq!(user.role, Role::Member);
        assert_eq!(user.created_at, 7);
        assert_eq!(user.updated_at, 7);
    }

    #[test]
    fn a_role_round_trips_through_its_name_and_an_unknown_one_is_none() {
        for role in [Role::Member, Role::Operator] {
            assert_eq!(Role::parse(role.as_str()), Some(role));
        }
        assert_eq!(Role::parse("admin"), None);
        assert!(Role::Operator.is_operator() && !Role::Member.is_operator());
    }

    #[test]
    fn levels_beyond_the_top_are_refused() {
        assert_eq!(validate_vip_level(0).unwrap(), 0);
        assert_eq!(validate_vip_level(MAX_VIP_LEVEL).unwrap(), MAX_VIP_LEVEL);
        assert!(validate_vip_level(MAX_VIP_LEVEL + 1).is_err());
    }

    #[test]
    fn an_id_is_opaque_hex_and_never_repeats() {
        let id = new_user_id();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(id, new_user_id());
    }

    #[test]
    fn status_round_trips_through_its_name() {
        for s in [UserStatus::Active, UserStatus::Suspended] {
            assert_eq!(UserStatus::parse(s.as_str()), Some(s));
        }
        assert_eq!(UserStatus::parse("banned"), None);
    }
}
