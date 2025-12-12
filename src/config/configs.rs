use anyhow::Context;
use serde::Deserialize;
use super::s3::S3Config;
use super::dynamodb::DynamoDBConfig;

#[derive(Debug, Deserialize)]
pub struct Configs {
    pub dynamodb: DynamoDBConfig,
    pub s3: S3Config,
}

impl Configs {
    pub fn new() -> anyhow::Result<Self> {
        let run_mode = std::env::var("APP__RUN_MODE")
            .or_else(|_| std::env::var("RUN_MODE"))
            .unwrap_or_else(|_| "default".into());

        let cfg = config::Config::builder()
            .add_source(config::File::with_name("config/default"))
            .add_source(config::File::with_name(&format!("config/{}", run_mode)).required(false))
            .add_source(config::File::with_name("config/local").required(false))
            .add_source(
                config::Environment::with_prefix("APP")
                    .separator("__")
                    .try_parsing(true)
            )
            .build()
            .context("build config")?;

        cfg.try_deserialize().context("deserialize config")
    }
}