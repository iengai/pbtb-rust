use crate::domain::Bot;
use crate::domain::bot::ApiKeyRepository;
use async_trait::async_trait;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use serde_json::json;

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

    pub async fn save(&self, bot: &Bot) -> Result<(), String> {
        let key = Self::api_key_path(&bot.user_id, &bot.id);
        let api_key = json!({
            &bot.id: {
                "exchange": bot.exchange.as_str(),
                "key": bot.api_key,
                "secret": bot.secret_key,
            }
        });

        let json_bytes = serde_json::to_vec_pretty(&api_key)
            .map_err(|e| format!("Failed to serialize api-keys: {e:?}"))?;

        self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .body(ByteStream::from(json_bytes))
            .content_type("application/json")
            .send()
            .await
            .map_err(|e| format!("Failed to save api-keys.json to S3: {e:?}"))?;

        Ok(())
    }

    /// Remove bot API key
    pub async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), String> {
        let key = Self::api_key_path(user_id, bot_id);

        self.client
            .delete_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
            .map_err(|e| format!("Failed to delete bot config from S3: {e:?}"))?;

        Ok(())
    }
}

#[async_trait]
impl ApiKeyRepository for S3ApiKeyRepository {
    async fn save(&self, bot: &Bot) -> Result<(), String> {
        S3ApiKeyRepository::save(self, bot).await
    }

    async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), String> {
        S3ApiKeyRepository::delete(self, user_id, bot_id).await
    }
}
