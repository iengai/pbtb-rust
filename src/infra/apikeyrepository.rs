use crate::domain::Bot;
use crate::domain::bot::ApiKeyRepository;
use crate::domain::error::DomainError;
use crate::infra::aws_error::repo_err;
use async_trait::async_trait;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use serde_json::json;

/// A bot's exchange API credentials, read back from the encrypted store.
pub struct ApiCredentials {
    pub key: String,
    pub secret: String,
}

pub struct S3ApiKeyRepository {
    client: Client,
    bucket_name: String,
}

impl S3ApiKeyRepository {
    pub fn new(client: Client, bucket_name: String) -> Self {
        Self {
            client,
            bucket_name,
        }
    }
    fn api_key_path(user_id: &str, bot_id: &str) -> String {
        format!("{user_id}/{bot_id}/api-keys.json")
    }

    pub async fn save(&self, bot: &Bot) -> Result<(), DomainError> {
        let key = Self::api_key_path(&bot.user_id, &bot.id);
        let api_key = json!({
            &bot.id: {
                "exchange": bot.exchange.as_str(),
                "key": bot.api_key,
                "secret": bot.secret_key,
            }
        });

        let json_bytes = serde_json::to_vec_pretty(&api_key)
            .map_err(|e| repo_err("Failed to serialize api-keys", e))?;

        self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .body(ByteStream::from(json_bytes))
            .content_type("application/json")
            .send()
            .await
            .map_err(|e| repo_err("Failed to save api-keys.json to S3", e))?;

        Ok(())
    }

    /// Remove bot API key
    pub async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), DomainError> {
        let key = Self::api_key_path(user_id, bot_id);

        self.client
            .delete_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
            .map_err(|e| repo_err("Failed to delete api-keys.json from S3", e))?;

        Ok(())
    }

    /// Read a bot's exchange credentials from `{user_id}/{bot_id}/api-keys.json`.
    /// `Ok(None)` is a genuine absence (no keys stored for the bot); an I/O or
    /// parse fault is an `Err`, never collapsed into `None`. The key/secret are
    /// never logged.
    pub async fn get(
        &self,
        user_id: &str,
        bot_id: &str,
    ) -> Result<Option<ApiCredentials>, DomainError> {
        let key = Self::api_key_path(user_id, bot_id);

        let output = match self
            .client
            .get_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
        {
            Ok(o) => o,
            Err(e) => {
                if e.as_service_error().is_some_and(|se| se.is_no_such_key()) {
                    return Ok(None);
                }
                return Err(repo_err("Failed to read api-keys.json from S3", e));
            }
        };

        let bytes = output
            .body
            .collect()
            .await
            .map_err(|e| repo_err("Failed to read api-keys.json body", e))?
            .into_bytes();

        let doc: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| repo_err("Failed to parse api-keys.json", e))?;

        // Shape: { "<bot_id>": { "exchange", "key", "secret" } }.
        let entry = &doc[bot_id];
        match (
            entry.get("key").and_then(|v| v.as_str()),
            entry.get("secret").and_then(|v| v.as_str()),
        ) {
            (Some(k), Some(s)) => Ok(Some(ApiCredentials {
                key: k.to_string(),
                secret: s.to_string(),
            })),
            _ => Ok(None),
        }
    }
}

#[async_trait]
impl ApiKeyRepository for S3ApiKeyRepository {
    async fn save(&self, bot: &Bot) -> Result<(), DomainError> {
        S3ApiKeyRepository::save(self, bot).await
    }

    async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), DomainError> {
        S3ApiKeyRepository::delete(self, user_id, bot_id).await
    }
}
