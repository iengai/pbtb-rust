use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use serde_json::{json, Value};
use std::collections::HashMap;

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

    /// S3 key for api-keys.json
    fn api_keys_key() -> String {
        "api-keys.json".to_string()
    }

    /// Get existing api-keys.json content, or create empty structure
    async fn get_api_keys(&self) -> Result<Value, String> {
        let key = Self::api_keys_key();

        match self
            .client
            .get_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
        {
            Ok(result) => {
                let bytes = result
                    .body
                    .collect()
                    .await
                    .map_err(|e| format!("Failed to read api-keys.json: {:?}", e))?
                    .into_bytes();

                serde_json::from_slice(&bytes)
                    .map_err(|e| format!("Failed to parse api-keys.json: {:?}", e))
            }
            Err(e) => {
                let error_msg = format!("{:?}", e);
                if error_msg.contains("NotFound") || error_msg.contains("404") {
                    // File doesn't exist, return empty object
                    Ok(json!({}))
                } else {
                    Err(format!("Failed to get api-keys.json from S3: {:?}", e))
                }
            }
        }
    }

    /// Save api-keys.json to S3
    async fn save_api_keys(&self, api_keys: &Value) -> Result<(), String> {
        let key = Self::api_keys_key();

        let json_bytes = serde_json::to_vec_pretty(api_keys)
            .map_err(|e| format!("Failed to serialize api-keys: {:?}", e))?;

        self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .body(ByteStream::from(json_bytes))
            .content_type("application/json")
            .send()
            .await
            .map_err(|e| format!("Failed to save api-keys.json to S3: {:?}", e))?;

        Ok(())
    }

    /// Add or update bot API key
    pub async fn upsert_bot_key(
        &self,
        bot_id: &str,
        api_key: &str,
        secret_key: &str,
    ) -> Result<(), String> {
        // Get existing api-keys
        let mut api_keys = self.get_api_keys().await?;

        // Ensure api_keys is an object
        if !api_keys.is_object() {
            api_keys = json!({});
        }

        // Add/Update bot entry
        let bot_entry = json!({
            "exchange": "bybit",
            "key": api_key,
            "secret": secret_key,
        });

        if let Some(obj) = api_keys.as_object_mut() {
            obj.insert(bot_id.to_string(), bot_entry);
        }

        // Save back to S3
        self.save_api_keys(&api_keys).await
    }

    /// Remove bot API key
    pub async fn remove_bot_key(&self, bot_id: &str) -> Result<(), String> {
        // Get existing api-keys
        let mut api_keys = self.get_api_keys().await?;

        // Remove bot entry
        if let Some(obj) = api_keys.as_object_mut() {
            obj.remove(bot_id);
        }

        // Save back to S3
        self.save_api_keys(&api_keys).await
    }

    /// Get bot API key
    pub async fn get_bot_key(&self, bot_id: &str) -> Result<Option<BotApiKey>, String> {
        let api_keys = self.get_api_keys().await?;

        if let Some(bot_entry) = api_keys.get(bot_id) {
            let exchange = bot_entry.get("exchange")
                .and_then(|v| v.as_str())
                .unwrap_or("bybit")
                .to_string();

            let key = bot_entry.get("key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing key field".to_string())?
                .to_string();

            let secret = bot_entry.get("secret")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing secret field".to_string())?
                .to_string();

            Ok(Some(BotApiKey {
                bot_id: bot_id.to_string(),
                exchange,
                key,
                secret,
            }))
        } else {
            Ok(None)
        }
    }
}

/// Bot API Key structure
#[derive(Debug, Clone)]
pub struct BotApiKey {
    pub bot_id: String,
    pub exchange: String,
    pub key: String,
    pub secret: String,
}