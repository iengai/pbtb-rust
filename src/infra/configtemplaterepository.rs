use crate::domain::configtemplate::{ConfigTemplate, ConfigTemplateRepository};
use crate::domain::error::DomainError;
use crate::infra::aws_error::{repo_err, sdk_err};
use async_trait::async_trait;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use serde::Serialize;

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
            .map_err(|e| sdk_err("Failed to get template from S3", e))?;

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
            .map_err(|e| sdk_err("Failed to list templates from S3", e))?;

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
                    Err(sdk_err("Failed to check template existence", e))
                }
            }
        }
    }

    async fn save(&self, template: &ConfigTemplate) -> Result<(), DomainError> {
        let body = template_body(&template.config_data)?;
        self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(Self::template_key(&template.name))
            .body(ByteStream::from(body))
            .content_type("application/json")
            .send()
            .await
            .map_err(|e| sdk_err("Failed to write template to S3", e))?;
        Ok(())
    }
}

/// The bytes a template is written as, indented as the catalogue scripts
/// indent the objects they write.
fn template_body(config_data: &serde_json::Value) -> Result<Vec<u8>, DomainError> {
    let mut body = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut body, formatter);
    config_data
        .serialize(&mut serializer)
        .map_err(|e| repo_err("Failed to serialize template", e))?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A strategy parameter must come back from a read-then-write bit for bit:
    /// the catalogue's `trading_sha` covers it, and a float one ULP off would
    /// read as a changed strategy and re-run its backtest.
    #[test]
    fn a_written_template_keeps_every_float_bit_exact() {
        let python = r#"{"bot": {"long": {"a": 0.30000000000000004, "b": 1e-05, "c": 2.2250738585072014e-308, "n": 3}}}"#;
        let read: serde_json::Value = serde_json::from_str(python).unwrap();
        let written: serde_json::Value =
            serde_json::from_slice(&template_body(&read).unwrap()).unwrap();

        let long = &written["bot"]["long"];
        assert_eq!(
            long["a"].as_f64().unwrap().to_bits(),
            0.30000000000000004_f64.to_bits()
        );
        assert_eq!(long["b"].as_f64().unwrap().to_bits(), 1e-5_f64.to_bits());
        assert_eq!(
            long["c"].as_f64().unwrap().to_bits(),
            2.2250738585072014e-308_f64.to_bits()
        );
        assert_eq!(
            long["n"],
            serde_json::json!(3),
            "an integer stays an integer"
        );
        assert_eq!(written, read);
    }
}
