// src/config/ecs.rs
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct EcsConfig {
    pub region: String,
    pub cluster_arn: String,
    // The passivbot task-definition the bot launches. The name is version-agnostic
    // (family `…-passivbot`); the running passivbot version is a property of the
    // image tag, not these field names. Env: APP__ECS__TD_PASSIVBOT_ARN /
    // APP__ECS__TD_PASSIVBOT_CONTAINER_NAME.
    pub td_passivbot_arn: String,
    pub td_passivbot_container_name: String,
}
