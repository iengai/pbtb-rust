use std::sync::Arc;
use crate::domain::configtemplate::ConfigTemplateRepository;

pub struct ListTemplatesUseCase {
    template_repository: Arc<dyn ConfigTemplateRepository>,
}

impl ListTemplatesUseCase {
    pub fn new(template_repository: Arc<dyn ConfigTemplateRepository>) -> Self {
        Self { template_repository }
    }

    pub async fn execute(&self) -> Result<Vec<String>, String> {
        self.template_repository.list().await
    }
}