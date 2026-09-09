use crate::domain::error::DomainError;
use crate::domain::returncurve::ReturnCurveRepository;
use crate::infra::aws_error::{repo_err, sdk_err};
use async_trait::async_trait;
use aws_sdk_s3::Client;
use serde_json::Value;

/// The object a bot's series lives at: `<prefix>/{user_id}/{bot_id}.json` in
/// the chart bucket. Shared by the collector that writes it and the API that
/// reads it, so the two can never disagree on the layout; the tenant prefix
/// is what keeps one account's curves out of another's reach.
pub fn series_key(prefix: &str, user_id: &str, bot_id: &str) -> String {
    format!("{}/{user_id}/{bot_id}.json", prefix.trim_matches('/'))
}

/// Reads the collector's per-bot series from the chart bucket.
pub struct S3ReturnCurveRepository {
    client: Client,
    bucket_name: String,
    key_prefix: String,
}

impl S3ReturnCurveRepository {
    pub fn new(client: Client, bucket_name: String, key_prefix: String) -> Self {
        Self {
            client,
            bucket_name,
            key_prefix,
        }
    }
}

#[async_trait]
impl ReturnCurveRepository for S3ReturnCurveRepository {
    async fn get(&self, user_id: &str, bot_id: &str) -> Result<Option<Value>, DomainError> {
        let key = series_key(&self.key_prefix, user_id, bot_id);
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
                return Err(sdk_err("Failed to read the return series from S3", e));
            }
        };
        let bytes = output
            .body
            .collect()
            .await
            .map_err(|e| repo_err("Failed to read the return series body", e))?
            .into_bytes();
        let series = serde_json::from_slice(&bytes)
            .map_err(|e| repo_err("Failed to parse the return series JSON", e))?;
        Ok(Some(series))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_series_sits_under_the_tenant() {
        assert_eq!(
            series_key("charts", "u-1", "alpha"),
            "charts/u-1/alpha.json"
        );
        assert_eq!(
            series_key("/charts/", "u-1", "alpha"),
            "charts/u-1/alpha.json",
            "a prefix is a directory name, however it was configured"
        );
    }
}
