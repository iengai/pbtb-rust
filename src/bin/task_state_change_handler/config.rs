use serde::Deserialize;
use pbtb_rust::config::ecs::EcsConfig;
use pbtb_rust::config::dynamodb::DynamoDBConfig;

#[derive(Debug, Deserialize)]
pub struct TaskStateChangeConfig {
    pub ecs: EcsConfig,
    pub dynamodb: DynamoDBConfig,
}
