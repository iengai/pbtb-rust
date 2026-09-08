use std::sync::Arc;

use crate::domain::error::DomainError;
use crate::domain::identity::IdentityRepository;

/// Release every identity a tenant holds.
///
/// The way back from a link that went to the wrong place. Deliberately
/// all-or-nothing rather than per-identity: it is a recovery action, a tenant
/// holds a handful of these at most, and re-linking is a button press.
pub struct UnlinkIdentitiesUseCase {
    identities: Arc<dyn IdentityRepository>,
}

impl UnlinkIdentitiesUseCase {
    pub fn new(identities: Arc<dyn IdentityRepository>) -> Self {
        Self { identities }
    }

    /// How many were released.
    pub async fn execute(&self, user_id: &str) -> Result<usize, DomainError> {
        let links = self.identities.links_of(user_id).await?;

        let mut released = 0;
        for (provider, subject) in links {
            if self.identities.unlink(&provider, &subject, user_id).await? {
                released += 1;
            }
        }
        Ok(released)
    }
}
