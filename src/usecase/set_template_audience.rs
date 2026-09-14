use std::sync::Arc;

use crate::domain::clock::Clock;
use crate::domain::configtemplate::ConfigTemplateRepository;
use crate::domain::error::DomainError;
use crate::domain::templateaudience::{PublishedTemplates, TemplateAudiencePublisher};
use crate::domain::user::Role;
use crate::usecase::list_templates::published_names;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetAudienceOutcome {
    /// The template is written with the new audience.
    Updated { operator_only: bool },
    /// The template already had this audience; it was not written.
    Unchanged { operator_only: bool },
    /// No template by that name in the catalogue.
    NotFound,
}

/// Publish a template to everyone or retire it to the operator's account. The
/// mark lives in the template file (`ConfigTemplate::is_operator_only`), so this
/// rewrites the file with its trading content as read.
///
/// The file is the record. After it, the public catalogue's overlay is rebuilt
/// from every listed template, on an unchanged audience too: a retry repairs a
/// publish that failed after the save, and any switch brings in line a template
/// whose file was edited in S3. Two switches racing can each read the other's
/// template before its save; the later overlay then misses that change until
/// the next switch.
pub struct SetTemplateAudienceUseCase {
    templates: Arc<dyn ConfigTemplateRepository>,
    clock: Arc<dyn Clock>,
    publisher: Option<Arc<dyn TemplateAudiencePublisher>>,
}

impl SetTemplateAudienceUseCase {
    pub fn new(
        templates: Arc<dyn ConfigTemplateRepository>,
        clock: Arc<dyn Clock>,
        publisher: Option<Arc<dyn TemplateAudiencePublisher>>,
    ) -> Self {
        Self {
            templates,
            clock,
            publisher,
        }
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
        let unchanged = template.is_operator_only() == operator_only;
        if !unchanged {
            template.set_operator_only(operator_only)?;
            self.templates.save(&template).await?;
        }
        self.reconcile().await?;
        Ok(if unchanged {
            SetAudienceOutcome::Unchanged { operator_only }
        } else {
            SetAudienceOutcome::Updated { operator_only }
        })
    }

    async fn reconcile(&self) -> Result<(), DomainError> {
        let Some(publisher) = &self.publisher else {
            return Ok(());
        };
        let published = published_names(self.templates.as_ref()).await?;
        publisher
            .publish(&PublishedTemplates {
                generated_at: self.clock.now(),
                published,
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::configtemplate::ConfigTemplate;
    use crate::domain::error::Retryability;
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Mutex;

    const NOW: i64 = 1_700_000_000;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> i64 {
            NOW
        }
    }

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
            match rows.iter_mut().find(|t| t.name == template.name) {
                Some(row) => *row = template.clone(),
                None => rows.push(template.clone()),
            }
            Ok(())
        }
    }

    /// Records every set it is given; `fault` makes every call fail with that
    /// retryability.
    #[derive(Default)]
    struct FakePublisher {
        published: Mutex<Vec<PublishedTemplates>>,
        fault: Mutex<Option<Retryability>>,
    }

    impl FakePublisher {
        fn sets(&self) -> Vec<Vec<String>> {
            self.published
                .lock()
                .unwrap()
                .iter()
                .map(|p| p.published.clone())
                .collect()
        }
    }

    #[async_trait]
    impl TemplateAudiencePublisher for FakePublisher {
        async fn publish(&self, published: &PublishedTemplates) -> Result<(), DomainError> {
            self.published.lock().unwrap().push(published.clone());
            match *self.fault.lock().unwrap() {
                Some(retry) => Err(DomainError::repository_with(
                    "s3",
                    retry,
                    std::io::Error::other("unreachable"),
                )),
                None => Ok(()),
            }
        }
    }

    fn template(name: &str, pbtb: serde_json::Value) -> ConfigTemplate {
        ConfigTemplate {
            name: name.to_string(),
            description: None,
            config_data: json!({ "pbtb": pbtb, "bot": { "long": { "n_positions": 1 } } }),
            version: None,
        }
    }

    /// `tpl-a` published, `tpl-b` retired.
    fn catalogue() -> Arc<Templates> {
        let templates = Templates::default();
        templates.rows.lock().unwrap().extend([
            template("tpl-a", json!({ "title": "A" })),
            template("tpl-b", json!({ "title": "B", "audience": "operator" })),
        ]);
        Arc::new(templates)
    }

    fn usecase(
        templates: Arc<Templates>,
        publisher: Option<Arc<FakePublisher>>,
    ) -> SetTemplateAudienceUseCase {
        SetTemplateAudienceUseCase::new(
            templates,
            Arc::new(FixedClock),
            publisher.map(|p| p as Arc<dyn TemplateAudiencePublisher>),
        )
    }

    #[tokio::test]
    async fn the_operator_retires_and_publishes_a_template() {
        let templates = catalogue();
        let uc = usecase(templates.clone(), None);

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
    async fn a_switch_publishes_every_listed_template_offered_to_everyone() {
        let templates = catalogue();
        let publisher = Arc::new(FakePublisher::default());
        let uc = usecase(templates, Some(publisher.clone()));

        uc.execute(Role::Operator, "tpl-a", true).await.unwrap();
        uc.execute(Role::Operator, "tpl-b", false).await.unwrap();

        assert_eq!(
            publisher.sets(),
            [vec![], vec!["tpl-b".to_string()]],
            "each set is read after its own save, and names no retired template"
        );
        assert_eq!(publisher.published.lock().unwrap()[0].generated_at, NOW);
    }

    #[tokio::test]
    async fn an_unchanged_switch_writes_no_template_and_rewrites_a_stale_overlay() {
        let templates = catalogue();
        // An edit of the object in S3 the overlay never heard of.
        templates
            .rows
            .lock()
            .unwrap()
            .push(template("tpl-c", json!({})));
        let publisher = Arc::new(FakePublisher::default());

        let out = usecase(templates.clone(), Some(publisher.clone()))
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
        assert_eq!(
            publisher.sets(),
            [vec!["tpl-a".to_string(), "tpl-c".to_string()]]
        );
    }

    #[tokio::test]
    async fn a_publish_fault_after_the_save_keeps_its_retryability_and_the_template() {
        for retry in [Retryability::Transient, Retryability::Permanent] {
            let templates = catalogue();
            let publisher = Arc::new(FakePublisher::default());
            *publisher.fault.lock().unwrap() = Some(retry);

            let err = usecase(templates.clone(), Some(publisher))
                .execute(Role::Operator, "tpl-a", true)
                .await
                .unwrap_err();

            assert_eq!(err.retryability(), retry);
            assert!(
                templates.get("tpl-a").await.unwrap().is_operator_only(),
                "the file is the record; a retry reconciles the overlay"
            );
        }
    }

    #[tokio::test]
    async fn a_member_is_refused_whether_or_not_the_template_exists() {
        let templates = catalogue();
        let publisher = Arc::new(FakePublisher::default());
        let uc = usecase(templates.clone(), Some(publisher.clone()));
        for name in ["tpl-a", "tpl-nope"] {
            let err = uc.execute(Role::Member, name, true).await.unwrap_err();
            assert!(matches!(err, DomainError::OperatorOnly), "{name}: {err}");
        }
        assert_eq!(*templates.writes.lock().unwrap(), 0);
        assert!(publisher.sets().is_empty());
    }

    #[tokio::test]
    async fn an_unlisted_name_is_not_found_and_not_created() {
        let templates = catalogue();
        let publisher = Arc::new(FakePublisher::default());
        let out = usecase(templates.clone(), Some(publisher.clone()))
            .execute(Role::Operator, "tpl-nope", true)
            .await
            .unwrap();
        assert_eq!(out, SetAudienceOutcome::NotFound);
        assert_eq!(*templates.writes.lock().unwrap(), 0);
        assert!(
            publisher.sets().is_empty(),
            "a missing name publishes nothing"
        );
    }
}
