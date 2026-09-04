use crate::domain::configtemplate::{ConfigTemplate, ConfigTemplateRepository};
use crate::domain::error::DomainError;
use crate::infra::aws_error::repo_err;
use async_trait::async_trait;
use aws_sdk_s3::Client;

pub struct S3TemplateRepository {
    client: Client,
    bucket_name: String,
}

impl S3TemplateRepository {
    pub fn new(client: Client, bucket_name: String) -> Self {
        Self {
            client,
            bucket_name,
        }
    }

    /// Helper: construct S3 key for template
    fn template_key(template_name: &str) -> String {
        format!("predefined/{template_name}.json")
    }
}

#[async_trait]
impl ConfigTemplateRepository for S3TemplateRepository {
    async fn get(&self, template_name: &str) -> Result<ConfigTemplate, DomainError> {
        let key = Self::template_key(template_name);

        let result = self
            .client
            .get_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
            .map_err(|e| repo_err("Failed to get template from S3", e))?;

        let bytes = result
            .body
            .collect()
            .await
            .map_err(|e| repo_err("Failed to read template body", e))?
            .into_bytes();

        let json_value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| repo_err("Failed to parse template JSON", e))?;

        Ok(ConfigTemplate {
            name: template_name.to_string(),
            description: Option::from("".to_string()),
            version: Option::from("".to_string()),
            config_data: json_value,
        })
    }

    async fn list(&self) -> Result<Vec<String>, DomainError> {
        let result = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket_name)
            .prefix("predefined/")
            .send()
            .await
            .map_err(|e| repo_err("Failed to list templates from S3", e))?;

        let templates = result
            .contents()
            .iter()
            .filter_map(|obj| {
                obj.key().and_then(|key| {
                    key.strip_prefix("predefined/")
                        .and_then(|name| name.strip_suffix(".json"))
                        .map(|s| s.to_string())
                })
            })
            .collect();

        Ok(templates)
    }

    async fn exists(&self, template_name: &str) -> Result<bool, DomainError> {
        let key = Self::template_key(template_name);

        match self
            .client
            .head_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            // A 404/NotFound is a genuine absence; anything else is a real read
            // failure that must surface rather than read back as "no template".
            Err(e) => {
                let error_msg = format!("{e:?}");
                if error_msg.contains("NotFound") || error_msg.contains("404") {
                    Ok(false)
                } else {
                    Err(repo_err("Failed to check template existence", e))
                }
            }
        }
    }
}
