use serde::Deserialize;
use pbtb_rust::config::ecs::EcsConfig;

#[derive(Debug, Deserialize)]
pub struct TaskStoppedConfig {
    pub ecs: EcsConfig,
}