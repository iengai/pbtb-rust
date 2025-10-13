use async_trait::async_trait;
use serde::{Deserialize, Serialize};


/// Configuration template entity (predefined templates)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigTemplate {
    pub name: String,
    pub description: Option<String>,
    pub config_data: serde_json::Value,
    pub version: String,
}

/// Repository interface for configuration templates
#[async_trait]
pub trait ConfigTemplateRepository: Send + Sync {
    /// Get a predefined template by name
    async fn get(&self, template_name: &str) -> Result<ConfigTemplate, String>;

    /// List all available template names
    async fn list(&self) -> Result<Vec<String>, String>;

    /// Check if template exists
    async fn exists(&self, template_name: &str) -> Result<bool, String>;
}