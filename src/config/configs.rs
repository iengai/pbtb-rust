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

/// Build the config from `APP__*` environment variables, the single config
/// source in every environment. The `APP` prefix and `__` separator map onto the
/// nested structs: `APP__DYNAMODB__TABLE_NAME` -> `[dynamodb] table_name`.
///
/// Local dev supplies these from `config/dev.env` (the Dev Container loads it via
/// compose `env_file`; on the host you `source` it); the deployed binaries get
/// them from SSM `base-env` (telebot) and Terraform (Lambda). No config file
/// ships inside the images, so env is the only source.
pub fn load_config<T>() -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let cfg = config::Config::builder()
        .add_source(
            config::Environment::with_prefix("APP")
                .separator("__")
                .try_parsing(true),
        )
        .build()
        .context("Failed to build config from APP__* environment")?;

    cfg.try_deserialize::<T>()
        .with_context(|| format!("Failed to deserialize config into struct: {}", std::any::type_name::<T>()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // `APP__*` env deserializes into the nested config struct via the `APP`
    // prefix and `__` nesting separator.
    #[test]
    fn app_env_populates_nested_config() {
        #[derive(Debug, serde::Deserialize)]
        struct Mini {
            dynamodb: MiniDynamo,
        }
        #[derive(Debug, serde::Deserialize)]
        struct MiniDynamo {
            table_name: String,
        }

        // SAFETY: env is process-global; this is the only test that mutates this
        // key and the only one that calls load_config, so there is no cross-thread
        // race on it.
        unsafe { std::env::set_var("APP__DYNAMODB__TABLE_NAME", "from_env") };
        let cfg = load_config::<Mini>().expect("load config from APP__* env");
        assert_eq!(cfg.dynamodb.table_name, "from_env");
        unsafe { std::env::remove_var("APP__DYNAMODB__TABLE_NAME") };
    }
}
