use async_trait::async_trait;
use aws_sdk_s3::Client;
use crate::domain::configtemplate::{ConfigTemplate, ConfigTemplateRepository};

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
        format!("predefined/{}.json", template_name)
    }
}

#[async_trait]
impl ConfigTemplateRepository for S3TemplateRepository {
    async fn get(&self, template_name: &str) -> Result<ConfigTemplate, String> {
        let key = Self::template_key(template_name);

        let result = self
            .client
            .get_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
            .map_err(|e| format!("Failed to get template from S3: {:?}", e))?;

        let bytes = result
            .body
            .collect()
            .await
            .map_err(|e| format!("Failed to read template body: {:?}", e))?
            .into_bytes();

        let json_value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Failed to parse bot config JSON: {:?}", e))?;

        Ok(ConfigTemplate {
            name: template_name.to_string(),
            description: Option::from("".to_string()),
            version:Option::from("".to_string()),
            config_data:json_value,
        })
    }

    async fn list(&self) -> Result<Vec<String>, String> {
        let result = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket_name)
            .prefix("predefined/")
            .send()
            .await
            .map_err(|e| format!("Failed to list templates from S3: {:?}", e))?;

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

    async fn exists(&self, template_name: &str) -> Result<bool, String> {
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
            Err(e) => {
                let error_msg = format!("{:?}", e);
                if error_msg.contains("NotFound") || error_msg.contains("404") {
                    Ok(false)
                } else {
                    Err(format!("Failed to check template existence: {:?}", e))
                }
            }
        }
    }
}