use crate::domain::Bot;
use crate::domain::bot::ApiKeyRepository;
use crate::domain::error::DomainError;
use crate::domain::exchange::Exchange;
use crate::infra::aws_error::{repo_err, sdk_err};
use async_trait::async_trait;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use serde_json::json;

/// A bot's exchange API credentials, read back from the encrypted store, in
/// `Bot::api_key` / `Bot::secret_key`'s meaning: for Hyperliquid, `key` is the
/// account address and `secret` the API wallet private key.
pub struct ApiCredentials {
    pub key: String,
    pub secret: String,
}

/// The field names passivbot's `api-keys.json` gives the two credentials on
/// an exchange: passivbot (and pb-runner, on Bybit) read the file as stored.
fn field_names(exchange: Exchange) -> (&'static str, &'static str) {
    match exchange {
        Exchange::Bybit => ("key", "secret"),
        Exchange::Hyperliquid => ("wallet_address", "private_key"),
    }
}

/// The bot's `api-keys.json`: `{ "<bot_id>": { "exchange", <key field>,
/// <secret field> } }`, with `is_vault: false` on Hyperliquid, where the
/// address is the trading account itself rather than a vault it leads.
fn document(bot: &Bot) -> serde_json::Value {
    let (key_field, secret_field) = field_names(bot.exchange);
    let mut entry = json!({
        "exchange": bot.exchange.as_str(),
        key_field: bot.api_key,
        secret_field: bot.secret_key,
    });
    if bot.exchange == Exchange::Hyperliquid {
        entry["is_vault"] = json!(false);
    }
    json!({ &bot.id: entry })
}

/// The credentials in a bot's `api-keys.json`; `None` when the entry lacks
/// either one. A file without an exchange was written for Bybit.
fn credentials(
    doc: &serde_json::Value,
    user_id: &str,
    bot_id: &str,
) -> Result<Option<ApiCredentials>, DomainError> {
    let entry = &doc[bot_id];
    let exchange = match entry.get("exchange").and_then(|v| v.as_str()) {
        None => Exchange::Bybit,
        Some(name) => Exchange::from_str(name).ok_or_else(|| {
            DomainError::CorruptRecord(format!(
                "api-keys.json for {user_id}/{bot_id} names an unknown exchange"
            ))
        })?,
    };
    let (key_field, secret_field) = field_names(exchange);
    Ok(
        match (
            entry.get(key_field).and_then(|v| v.as_str()),
            entry.get(secret_field).and_then(|v| v.as_str()),
        ) {
            (Some(k), Some(s)) => Some(ApiCredentials {
                key: k.to_string(),
                secret: s.to_string(),
            }),
            _ => None,
        },
    )
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
        let api_key = document(bot);

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
            .map_err(|e| sdk_err("Failed to save api-keys.json to S3", e))?;

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
            .map_err(|e| sdk_err("Failed to delete api-keys.json from S3", e))?;

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
                return Err(sdk_err("Failed to read api-keys.json from S3", e));
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

        credentials(&doc, user_id, bot_id)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn bot(exchange: Exchange, key: &str, secret: &str) -> Bot {
        Bot::create(
            "u".into(),
            exchange,
            "b".into(),
            key.into(),
            secret.into(),
            1,
        )
    }

    #[test]
    fn bybit_keys_are_stored_as_key_and_secret() {
        let doc = document(&bot(Exchange::Bybit, "ak", "sk"));
        assert_eq!(
            doc,
            json!({"b": {"exchange": "bybit", "key": "ak", "secret": "sk"}})
        );
        let creds = credentials(&doc, "u", "b").unwrap().unwrap();
        assert_eq!((creds.key.as_str(), creds.secret.as_str()), ("ak", "sk"));
    }

    #[test]
    fn hyperliquid_keys_are_stored_the_way_passivbot_reads_them() {
        let doc = document(&bot(Exchange::Hyperliquid, "0xacc", "0xkey"));
        assert_eq!(
            doc,
            json!({"b": {
                "exchange": "hyperliquid",
                "wallet_address": "0xacc",
                "private_key": "0xkey",
                "is_vault": false,
            }})
        );
        let creds = credentials(&doc, "u", "b").unwrap().unwrap();
        assert_eq!(
            (creds.key.as_str(), creds.secret.as_str()),
            ("0xacc", "0xkey")
        );
    }

    #[test]
    fn a_file_without_an_exchange_reads_as_bybit_and_an_unknown_one_is_corrupt() {
        let legacy = json!({"b": {"key": "ak", "secret": "sk"}});
        assert!(credentials(&legacy, "u", "b").unwrap().is_some());
        assert!(credentials(&json!({"b": {}}), "u", "b").unwrap().is_none());
        let unknown = json!({"b": {"exchange": "binance", "key": "ak", "secret": "sk"}});
        assert!(matches!(
            credentials(&unknown, "u", "b"),
            Err(DomainError::CorruptRecord(_))
        ));
    }
}
