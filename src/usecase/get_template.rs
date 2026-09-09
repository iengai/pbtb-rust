use crate::domain::botconfig::BotConfig;
use crate::domain::configtemplate::{ConfigTemplate, ConfigTemplateRepository};
use crate::domain::error::DomainError;
use std::sync::Arc;

/// A template as stored, and the config a bot would hold after applying it.
///
/// The config belongs to no tenant and no bot: its ids are empty and its
/// timestamp zero. It exists so a caller can describe the template — sides,
/// coins, strategies — through the same accessors a bot's config offers,
/// without a bot to apply it to.
pub struct TemplatePreview {
    pub template: ConfigTemplate,
    pub config: BotConfig,
}

/// Read one predefined template by name and derive its preview.
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
    ) -> Result<Option<TemplatePreview>, DomainError> {
        if !self.template_repository.exists(template_name).await? {
            return Ok(None);
        }
        let template = self.template_repository.get(template_name).await?;
        let config = BotConfig::from_template(String::new(), String::new(), &template, 0)?;
        Ok(Some(TemplatePreview { template, config }))
    }
}
