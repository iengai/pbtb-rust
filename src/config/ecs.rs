// src/config/ecs.rs
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct EcsConfig {
    pub region: String,
    pub cluster_arn: String,
    pub td_passivbot_v741_arn: String,
    pub td_passivbot_v741_container_name: String,
}