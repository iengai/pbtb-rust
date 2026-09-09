use crate::domain::error::DomainError;
use crate::domain::identity::IdentityRepository;
use std::sync::Arc;

/// The external identities a tenant holds, as `(provider, subject)` pairs.
pub struct ListIdentitiesUseCase {
    identities: Arc<dyn IdentityRepository>,
}

impl ListIdentitiesUseCase {
    pub fn new(identities: Arc<dyn IdentityRepository>) -> Self {
        Self { identities }
    }

    pub async fn execute(&self, user_id: &str) -> Result<Vec<(String, String)>, DomainError> {
        self.identities.links_of(user_id).await
    }
}
