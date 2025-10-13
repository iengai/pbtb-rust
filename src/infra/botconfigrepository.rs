use async_trait::async_trait;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::domain::botconfig::{BotConfig, BotConfigRepository, BotType};

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
        format!("{}/{}.json", user_id, bot_id)
    }

    /// Helper: construct S3 prefix for user's configs
    fn user_prefix(user_id: &str) -> String {
        format!("{}/", user_id)
    }
}

#[async_trait]
impl BotConfigRepository for S3BotConfigRepository {
    async fn get(&self, user_id: &str, bot_id: &str) -> Result<BotConfig, String> {
        let key = Self::bot_config_key(user_id, bot_id);

        let result = self
            .client
            .get_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
            .map_err(|e| format!("Failed to get bot config from S3: {:?}", e))?;

        let bytes = result
            .body
            .collect()
            .await
            .map_err(|e| format!("Failed to read bot config body: {:?}", e))?
            .into_bytes();

        let json_value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Failed to parse bot config JSON: {:?}", e))?;

        let template_name = json_value.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let created_at = json_value.get("created_at")
            .and_then(|v| v.as_i64())
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
            });

        let updated_at = json_value.get("updated_at")
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
            template_version:Option::from("".to_string()),
            config_data:json_value,
            created_at,
            updated_at,
        })
    }

    async fn save(&self, config: &BotConfig) -> Result<(), String> {
        let key = Self::bot_config_key(&config.user_id, &config.bot_id);

        let json = serde_json::to_vec_pretty(&config.config_data)
            .map_err(|e| format!("Failed to serialize bot config: {:?}", e))?;

        self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .body(ByteStream::from(json))
            .content_type("application/json")
            .send()
            .await
            .map_err(|e| format!("Failed to save bot config to S3: {:?}", e))?;

        Ok(())
    }

    async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), String> {
        let key = Self::bot_config_key(user_id, bot_id);

        self.client
            .delete_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
            .map_err(|e| format!("Failed to delete bot config from S3: {:?}", e))?;

        Ok(())
    }

    async fn exists(&self, user_id: &str, bot_id: &str) -> Result<bool, String> {
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
            Err(e) => {
                let error_msg = format!("{:?}", e);
                if error_msg.contains("NotFound") || error_msg.contains("404") {
                    Ok(false)
                } else {
                    Err(format!("Failed to check bot config existence: {:?}", e))
                }
            }
        }
    }
}