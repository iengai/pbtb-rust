use crate::domain::configtemplate::{ConfigTemplate, ConfigTemplateRepository};
use crate::domain::error::DomainError;
use std::sync::Arc;

/// Read one predefined template by name, as stored.
pub struct GetTemplateUseCase {
    template_repository: Arc<dyn ConfigTemplateRepository>,
}

impl GetTemplateUseCase {
    pub fn new(template_repository: Arc<dyn ConfigTemplateRepository>) -> Self {
        Self {
            template_repository,
        }
    }

    /// `None` when no template by that name exists; a missing template is an
    /// answer, not a fault.
    pub async fn execute(
        &self,
        template_name: &str,
    ) -> Result<Option<ConfigTemplate>, DomainError> {
        if !self.template_repository.exists(template_name).await? {
            return Ok(None);
        }
        self.template_repository.get(template_name).await.map(Some)
    }
}
