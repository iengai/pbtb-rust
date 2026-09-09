use crate::domain::configtemplate::ConfigTemplateRepository;
use crate::domain::error::DomainError;
use std::sync::Arc;

/// One template as a chooser shows it: its name and the level it asks for, so
/// a surface can mark what the caller cannot apply yet without hiding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateListing {
    pub name: String,
    pub min_vip_level: u8,
}

pub struct ListTemplatesUseCase {
    template_repository: Arc<dyn ConfigTemplateRepository>,
}

impl ListTemplatesUseCase {
    pub fn new(template_repository: Arc<dyn ConfigTemplateRepository>) -> Self {
        Self {
            template_repository,
        }
    }

    /// Every template, whatever the caller's level: the list is a catalogue,
    /// and seeing what a higher level unlocks is part of what it is for. The
    /// level sits inside each template's file, so the listing reads each one;
    /// the catalogue is a handful of objects and this is a per-click read.
    pub async fn execute(&self) -> Result<Vec<TemplateListing>, DomainError> {
        let mut listings = Vec::new();
        for name in self.template_repository.list().await? {
            let template = self.template_repository.get(&name).await?;
            listings.push(TemplateListing {
                min_vip_level: template.min_vip_level(),
                name,
            });
        }
        Ok(listings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::configtemplate::ConfigTemplate;
    use async_trait::async_trait;
    use serde_json::json;

    struct Templates(Vec<ConfigTemplate>);
    #[async_trait]
    impl ConfigTemplateRepository for Templates {
        async fn get(&self, name: &str) -> Result<ConfigTemplate, DomainError> {
            self.0
                .iter()
                .find(|t| t.name == name)
                .cloned()
                .ok_or_else(|| DomainError::InvalidConfig(format!("no template {name}")))
        }
        async fn list(&self) -> Result<Vec<String>, DomainError> {
            Ok(self.0.iter().map(|t| t.name.clone()).collect())
        }
        async fn exists(&self, name: &str) -> Result<bool, DomainError> {
            Ok(self.0.iter().any(|t| t.name == name))
        }
    }

    fn template(name: &str, config_data: serde_json::Value) -> ConfigTemplate {
        ConfigTemplate {
            name: name.to_string(),
            description: None,
            config_data,
            version: None,
        }
    }

    #[tokio::test]
    async fn the_listing_carries_each_templates_level_and_hides_none() {
        let uc = ListTemplatesUseCase::new(Arc::new(Templates(vec![
            template("open", json!({ "pbtb": {} })),
            template("gated", json!({ "pbtb": { "min_vip_level": 5 } })),
        ])));

        let listed = uc.execute().await.expect("list");

        assert_eq!(
            listed,
            vec![
                TemplateListing {
                    name: "open".to_string(),
                    min_vip_level: 0
                },
                TemplateListing {
                    name: "gated".to_string(),
                    min_vip_level: 5
                },
            ]
        );
    }
}
