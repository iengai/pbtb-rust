use anyhow::Context;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use super::s3::S3Config;
use super::dynamodb::DynamoDBConfig;
use super::ecs::EcsConfig;

#[derive(Debug, Deserialize)]
pub struct Configs {
    pub dynamodb: DynamoDBConfig,
    pub s3: S3Config,
    pub ecs: EcsConfig,
}

pub fn load_config<T>() -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let run_mode = std::env::var("APP__RUN_MODE")
        .or_else(|_| std::env::var("RUN_MODE"))
        .unwrap_or_else(|_| "default".into());

    let cfg = config::Config::builder()
        // 1) load environment variables
        .add_source(
            config::Environment::with_prefix("APP")
                .separator("__")
                .try_parsing(true),
        )
        // 2) load config files
        .add_source(config::File::with_name("config/default").required(false))
        .add_source(config::File::with_name(&format!("config/{}", run_mode)).required(false))
        .add_source(config::File::with_name("config/local").required(false))
        .build()
        .with_context(|| format!("Failed to build config. run_mode={}, search_path=config/", run_mode))?;

    cfg.try_deserialize::<T>()
        .with_context(|| format!("Failed to deserialize config into struct: {}", std::any::type_name::<T>()))
}