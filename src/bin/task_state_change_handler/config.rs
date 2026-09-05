use pbtb_rust::config::dynamodb::DynamoDBConfig;
use pbtb_rust::config::ecs::EcsConfig;
use pbtb_rust::config::s3::S3Config;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TaskStateChangeConfig {
    pub ecs: EcsConfig,
    pub dynamodb: DynamoDBConfig,
    /// The bot-config bucket: the restart path reads the bot's config to pick
    /// the engine line it must relaunch on.
    pub s3: S3Config,
}
