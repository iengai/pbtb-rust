use std::env;
use config::{Config, ConfigError, File};
use serde::Deserialize;
use super::s3::S3Config;
use super::dynamodb::DynamoDBConfig;

#[derive(Debug, Deserialize)]
pub struct Configs {
    pub dynamodb: DynamoDBConfig,
    pub s3: S3Config,
}

impl Configs {
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