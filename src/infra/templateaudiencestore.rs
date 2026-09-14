//! The public catalogue's audience overlay in the chart bucket,
//! `<public prefix>/templates/audience.json`, which the showcase CDN serves to
//! the public pages beside the showcase.

use async_trait::async_trait;
use aws_sdk_s3::Client;

use crate::config::chart::ChartConfig;
use crate::domain::error::DomainError;
use crate::domain::templateaudience::{PublishedTemplates, TemplateAudiencePublisher};
use crate::infra::aws_error::repo_err;
use crate::infra::publicobjects::{Objects, PUBLIC_CACHE_CONTROL, S3Objects};

fn audience_key(public_prefix: &str) -> String {
    format!(
        "{}/templates/audience.json",
        public_prefix.trim_matches('/')
    )
}

pub struct S3TemplateAudienceStore {
    objects: Box<dyn Objects>,
    key: String,
}

impl S3TemplateAudienceStore {
    pub fn new(client: Client, chart: &ChartConfig) -> Self {
        Self::over(
            Box::new(S3Objects::new(client, &chart.bucket_name)),
            &chart.public_prefix,
        )
    }

    fn over(objects: Box<dyn Objects>, public_prefix: &str) -> Self {
        Self {
            objects,
            key: audience_key(public_prefix),
        }
    }
}

#[async_trait]
impl TemplateAudiencePublisher for S3TemplateAudienceStore {
    async fn publish(&self, published: &PublishedTemplates) -> Result<(), DomainError> {
        let body = serde_json::to_vec(published)
            .map_err(|e| repo_err("Failed to serialize the template audiences", e))?;
        self.objects
            .put(&self.key, body, Some(PUBLIC_CACHE_CONTROL))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::publicobjects::memory::Memory;

    #[test]
    fn the_overlay_sits_under_the_public_prefix() {
        assert_eq!(audience_key("public"), "public/templates/audience.json");
        assert_eq!(audience_key("/public/"), "public/templates/audience.json");
    }

    #[tokio::test]
    async fn publishing_writes_the_set_with_the_public_cache_header() {
        let memory = Memory::default();
        let store = S3TemplateAudienceStore::over(Box::new(memory.clone()), "public");
        let overlay = PublishedTemplates {
            generated_at: 1_700_000_000,
            published: vec!["tpl-a".into(), "tpl-c".into()],
        };

        store.publish(&overlay).await.unwrap();

        assert_eq!(memory.keys(), ["public/templates/audience.json"]);
        let (written, cache): (PublishedTemplates, _) =
            memory.object("public/templates/audience.json");
        assert_eq!(written, overlay);
        assert_eq!(cache, Some(PUBLIC_CACHE_CONTROL));
    }

    #[tokio::test]
    async fn publishing_replaces_the_previous_set_whole() {
        let memory = Memory::default();
        let store = S3TemplateAudienceStore::over(Box::new(memory.clone()), "public");
        for published in [vec!["tpl-a".to_string(), "tpl-b".into()], vec![]] {
            store
                .publish(&PublishedTemplates {
                    generated_at: 1,
                    published,
                })
                .await
                .unwrap();
        }
        let (written, _): (PublishedTemplates, _) = memory.object("public/templates/audience.json");
        assert!(
            written.published.is_empty(),
            "an empty set is written, not skipped"
        );
    }
}
