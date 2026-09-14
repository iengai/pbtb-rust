use std::sync::Arc;

use crate::domain::configtemplate::ConfigTemplateRepository;
use crate::domain::error::DomainError;
use crate::domain::user::Role;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetAudienceOutcome {
    /// The template is written with the new audience.
    Updated { operator_only: bool },
    /// The template already had this audience; nothing was written.
    Unchanged { operator_only: bool },
    /// No template by that name in the catalogue.
    NotFound,
}

/// Publish a template to everyone or retire it to the operator's account. The
/// mark lives in the template file (`ConfigTemplate::is_operator_only`), so this
/// rewrites the file with its trading content as read.
pub struct SetTemplateAudienceUseCase {
    templates: Arc<dyn ConfigTemplateRepository>,
}

impl SetTemplateAudienceUseCase {
    pub fn new(templates: Arc<dyn ConfigTemplateRepository>) -> Self {
        Self { templates }
    }

    pub async fn execute(
        &self,
        role: Role,
        name: &str,
        operator_only: bool,
    ) -> Result<SetAudienceOutcome, DomainError> {
        // Refused before anything is read, so a member learns nothing about
        // which names exist.
        if !role.is_operator() {
            return Err(DomainError::OperatorOnly);
        }
        // A write is a put, and a put under a name the catalogue does not list
        // would create a template out of nothing.
        if !self.templates.list().await?.iter().any(|n| n == name) {
            return Ok(SetAudienceOutcome::NotFound);
        }
        let mut template = self.templates.get(name).await?;
        if template.is_operator_only() == operator_only {
            return Ok(SetAudienceOutcome::Unchanged { operator_only });
        }
        template.set_operator_only(operator_only)?;
        self.templates.save(&template).await?;
        Ok(SetAudienceOutcome::Updated { operator_only })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::configtemplate::ConfigTemplate;
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Templates {
        rows: Mutex<Vec<ConfigTemplate>>,
        writes: Mutex<usize>,
    }

    #[async_trait]
    impl ConfigTemplateRepository for Templates {
        async fn get(&self, name: &str) -> Result<ConfigTemplate, DomainError> {
            self.rows
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.name == name)
                .cloned()
                .ok_or_else(|| DomainError::InvalidConfig(format!("no template {name}")))
        }
        async fn list(&self) -> Result<Vec<String>, DomainError> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .iter()
                .map(|t| t.name.clone())
                .collect())
        }
        async fn exists(&self, name: &str) -> Result<bool, DomainError> {
            Ok(self.rows.lock().unwrap().iter().any(|t| t.name == name))
        }
        async fn save(&self, template: &ConfigTemplate) -> Result<(), DomainError> {
            *self.writes.lock().unwrap() += 1;
            let mut rows = self.rows.lock().unwrap();
            rows.retain(|t| t.name != template.name);
            rows.push(template.clone());
            Ok(())
        }
    }

    fn catalogue() -> Arc<Templates> {
        let templates = Templates::default();
        templates.rows.lock().unwrap().push(ConfigTemplate {
            name: "tpl-a".to_string(),
            description: None,
            config_data: json!({ "pbtb": { "title": "A" }, "bot": { "long": { "n_positions": 1 } } }),
            version: None,
        });
        Arc::new(templates)
    }

    #[tokio::test]
    async fn the_operator_retires_and_publishes_a_template() {
        let templates = catalogue();
        let uc = SetTemplateAudienceUseCase::new(templates.clone());

        let out = uc.execute(Role::Operator, "tpl-a", true).await.unwrap();
        assert_eq!(
            out,
            SetAudienceOutcome::Updated {
                operator_only: true
            }
        );
        let saved = templates.get("tpl-a").await.unwrap();
        assert!(saved.is_operator_only());
        assert_eq!(
            saved.config_data["bot"],
            json!({ "long": { "n_positions": 1 } })
        );

        let out = uc.execute(Role::Operator, "tpl-a", false).await.unwrap();
        assert_eq!(
            out,
            SetAudienceOutcome::Updated {
                operator_only: false
            }
        );
        assert!(!templates.get("tpl-a").await.unwrap().is_operator_only());
    }

    #[tokio::test]
    async fn the_same_audience_writes_nothing() {
        let templates = catalogue();
        let out = SetTemplateAudienceUseCase::new(templates.clone())
            .execute(Role::Operator, "tpl-a", false)
            .await
            .unwrap();
        assert_eq!(
            out,
            SetAudienceOutcome::Unchanged {
                operator_only: false
            }
        );
        assert_eq!(*templates.writes.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn a_member_is_refused_whether_or_not_the_template_exists() {
        let templates = catalogue();
        let uc = SetTemplateAudienceUseCase::new(templates.clone());
        for name in ["tpl-a", "tpl-nope"] {
            let err = uc.execute(Role::Member, name, true).await.unwrap_err();
            assert!(matches!(err, DomainError::OperatorOnly), "{name}: {err}");
        }
        assert_eq!(*templates.writes.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn an_unlisted_name_is_not_found_and_not_created() {
        let templates = catalogue();
        let out = SetTemplateAudienceUseCase::new(templates.clone())
            .execute(Role::Operator, "tpl-nope", true)
            .await
            .unwrap();
        assert_eq!(out, SetAudienceOutcome::NotFound);
        assert_eq!(*templates.writes.lock().unwrap(), 0);
    }
}
