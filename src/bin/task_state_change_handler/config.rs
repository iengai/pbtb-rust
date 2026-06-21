use pbtb_rust::config::dynamodb::DynamoDBConfig;
use pbtb_rust::config::ecs::EcsConfig;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TaskStateChangeConfig {
    pub ecs: EcsConfig,
    pub dynamodb: DynamoDBConfig,
}
