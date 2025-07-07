use config::{Config, ConfigError, File};
use serde::Deserialize;
use std::env;

#[derive(Debug, Deserialize)]
pub struct DynamoDBConfig {
    pub endpoint_url: String,
    pub region: String,
    pub table_name: String,
}

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub dynamodb: DynamoDBConfig,
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let run_mode = env::var("RUN_MODE").unwrap_or_else(|_| "default".into());
        
        let s = Config::builder()
            // Start with default settings
            .add_source(File::with_name("config/default"))
            // Add environment-specific settings if specified
            .add_source(File::with_name(&format!("config/{}", run_mode)).required(false))
            // Add local settings file
            .add_source(File::with_name("config/local").required(false))
            // Add environment variables with prefix "APP"
            .add_source(config::Environment::with_prefix("APP").separator("__"))
            .build()?;

        s.try_deserialize()
    }
}