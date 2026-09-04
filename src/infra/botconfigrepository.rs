use crate::domain::botconfig::{BotConfig, BotConfigRepository, BotType};
use crate::domain::error::DomainError;
use crate::infra::aws_error::repo_err;
use async_trait::async_trait;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;

pub struct S3BotConfigRepository {
    client: Client,
    bucket_name: String,
}

impl S3BotConfigRepository {
    pub fn new(client: Client, bucket_name: String) -> Self {
        Self {
            client,
            bucket_name,
        }
    }

    /// Helper: construct S3 key for bot config
    fn bot_config_key(user_id: &str, bot_id: &str) -> String {
        format!("{user_id}/{bot_id}/{bot_id}.json")
    }
}

#[async_trait]
impl BotConfigRepository for S3BotConfigRepository {
    async fn get(&self, user_id: &str, bot_id: &str) -> Result<BotConfig, DomainError> {
        let key = Self::bot_config_key(user_id, bot_id);

        let result = self
            .client
            .get_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
            .map_err(|e| repo_err("Failed to get bot config from S3", e))?;

        let bytes = result
            .body
            .collect()
            .await
            .map_err(|e| repo_err("Failed to read bot config body", e))?
            .into_bytes();

        let json_value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| repo_err("Failed to parse bot config JSON", e))?;

        let template_name = BotConfig::embedded_template_name(&json_value)
            .unwrap_or("")
            .to_string();

        let created_at = json_value
            .get("created_at")
            .and_then(|v| v.as_i64())
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
            });

        let updated_at = json_value
            .get("updated_at")
            .and_then(|v| v.as_i64())
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
            });

        Ok(BotConfig {
            user_id: user_id.to_string(),
            bot_id: bot_id.to_string(),
            bot_type: BotType::Passivbot,
            template_name,
            template_version: Option::from("".to_string()),
            config_data: json_value,
            created_at,
            updated_at,
        })
    }

    async fn save(&self, config: &BotConfig) -> Result<(), DomainError> {
        let key = Self::bot_config_key(&config.user_id, &config.bot_id);

        let json = serde_json::to_vec_pretty(&config.config_data)
            .map_err(|e| repo_err("Failed to serialize bot config", e))?;

        self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .body(ByteStream::from(json))
            .content_type("application/json")
            .send()
            .await
            .map_err(|e| repo_err("Failed to save bot config to S3", e))?;

        Ok(())
    }

    async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), DomainError> {
        let key = Self::bot_config_key(user_id, bot_id);

        self.client
            .delete_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
            .map_err(|e| repo_err("Failed to delete bot config from S3", e))?;

        Ok(())
    }

    async fn exists(&self, user_id: &str, bot_id: &str) -> Result<bool, DomainError> {
        let key = Self::bot_config_key(user_id, bot_id);

        match self
            .client
            .head_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            // A 404/NotFound is a genuine absence, not a fault; anything else is a
            // real read failure that must surface (a swallowed permission error
            // here would read back as "no config").
            Err(e) => {
                let error_msg = format!("{e:?}");
                if error_msg.contains("NotFound") || error_msg.contains("404") {
                    Ok(false)
                } else {
                    Err(repo_err("Failed to check bot config existence", e))
                }
            }
        }
    }
}
