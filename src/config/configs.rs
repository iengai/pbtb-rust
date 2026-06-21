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

    // `config` v0.13 gives later-added sources higher priority. File layers are added
    // first (each later file overriding the earlier), then `APP__*` env on top so it
    // wins over every file value.
    let cfg = config::Config::builder()
        .add_source(config::File::with_name("config/default").required(false))
        .add_source(config::File::with_name(&format!("config/{}", run_mode)).required(false))
        .add_source(config::File::with_name("config/local").required(false))
        .add_source(
            config::Environment::with_prefix("APP")
                .separator("__")
                .try_parsing(true),
        )
        .build()
        .with_context(|| format!("Failed to build config. run_mode={}, search_path=config/", run_mode))?;

    cfg.try_deserialize::<T>()
        .with_context(|| format!("Failed to deserialize config into struct: {}", std::any::type_name::<T>()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // `config/default.toml` sets `dynamodb.table_name = "bots"`, so this exercises the
    // file-vs-env precedence: the file layer must load, and an `APP__*` override must win.
    #[test]
    fn app_env_overrides_config_file() {
        // SAFETY: env is process-global; all mutation is confined to this single test,
        // and no other test reads config, so there is no cross-thread race on this key.
        unsafe { std::env::remove_var("APP__DYNAMODB__TABLE_NAME") };
        let from_file = load_config::<Configs>().expect("load config from file");
        assert_eq!(from_file.dynamodb.table_name, "bots");

        unsafe { std::env::set_var("APP__DYNAMODB__TABLE_NAME", "env_wins") };
        let with_env = load_config::<Configs>().expect("load config with env override");
        assert_eq!(with_env.dynamodb.table_name, "env_wins");

        unsafe { std::env::remove_var("APP__DYNAMODB__TABLE_NAME") };
    }
}