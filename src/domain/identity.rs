use crate::domain::error::DomainError;
use async_trait::async_trait;

/// An external identity that has been linked to a tenant.
///
/// The link is what turns a token's subject into a `user_id`. It exists only
/// because someone deliberately created it: an unlinked subject is refused
/// rather than given a tenant of its own, so authenticating with the identity
/// provider is never, by itself, enough to get an account here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedIdentity {
    pub user_id: String,
    pub email: Option<String>,
    pub linked_at: i64,
}

#[async_trait]
pub trait IdentityRepository: Send + Sync {
    /// The tenant `subject` was linked to, or `None` when it was never linked.
    ///
    /// `provider` and `subject` together name the identity: a subject is only
    /// unique within the provider that issued it.
    async fn find_link(
        &self,
        provider: &str,
        subject: &str,
    ) -> Result<Option<LinkedIdentity>, DomainError>;
}
