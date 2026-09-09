use crate::domain::error::DomainError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Configuration template entity (predefined templates)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigTemplate {
    pub name: String,
    pub description: Option<String>,
    pub config_data: serde_json::Value,
    pub version: Option<String>,
}

impl ConfigTemplate {
    /// The lowest VIP level that may apply this template, read from
    /// `pbtb.min_vip_level` in the template JSON. Absent, or not a
    /// non-negative integer, means open to everyone; a value past `u8` is kept
    /// at the ceiling, which no account reaches, so an operator can retire a
    /// template from use by setting it absurdly high.
    ///
    /// It lives in the template file because that is the one place a template
    /// carries metadata already, and an operator can change it by editing the
    /// object in S3 without a deploy. Reading a template is never gated; only
    /// applying it is.
    pub fn min_vip_level(&self) -> u8 {
        self.config_data
            .get("pbtb")
            .and_then(|m| m.get("min_vip_level"))
            .and_then(|v| v.as_u64())
            .map(|v| u8::try_from(v).unwrap_or(u8::MAX))
            .unwrap_or(0)
    }
}

/// Repository interface for configuration templates
#[async_trait]
pub trait ConfigTemplateRepository: Send + Sync {
    /// Get a predefined template by name
    async fn get(&self, template_name: &str) -> Result<ConfigTemplate, DomainError>;

    /// List all available template names
    async fn list(&self) -> Result<Vec<String>, DomainError>;

    /// Check if template exists
    async fn exists(&self, template_name: &str) -> Result<bool, DomainError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn template(config_data: serde_json::Value) -> ConfigTemplate {
        ConfigTemplate {
            name: "t".to_string(),
            description: None,
            config_data,
            version: None,
        }
    }

    #[test]
    fn min_level_defaults_to_open() {
        assert_eq!(template(json!({})).min_vip_level(), 0);
        assert_eq!(template(json!({ "pbtb": {} })).min_vip_level(), 0);
        assert_eq!(
            template(json!({ "pbtb": { "min_vip_level": "3" } })).min_vip_level(),
            0,
            "a string is not a level"
        );
        assert_eq!(
            template(json!({ "pbtb": { "min_vip_level": -1 } })).min_vip_level(),
            0
        );
    }

    #[test]
    fn min_level_reads_the_pbtb_block_only() {
        assert_eq!(
            template(json!({ "pbtb": { "min_vip_level": 3 } })).min_vip_level(),
            3
        );
        assert_eq!(
            template(json!({ "min_vip_level": 3 })).min_vip_level(),
            0,
            "the gate is ours, and ours lives under pbtb"
        );
    }

    #[test]
    fn an_absurd_min_level_locks_the_template_for_everyone() {
        assert_eq!(
            template(json!({ "pbtb": { "min_vip_level": 1000 } })).min_vip_level(),
            u8::MAX
        );
    }
}
