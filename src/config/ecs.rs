// src/config/ecs.rs
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct EcsConfig {
    pub region: String,
    pub cluster_arn: String,
    /// The passivbot task definition registered for each engine line, as
    /// `<major>=<task-def arn>` pairs, comma-separated (`7=arn:…,8=arn:…`). A
    /// bot launches on the entry matching its config's `config_version` — see
    /// `usecase::EngineTaskDefinitions`, which parses this at process start so a
    /// bad table fails boot rather than a launch. Env:
    /// APP__ECS__TD_PASSIVBOT_BY_ENGINE / APP__ECS__TD_PASSIVBOT_CONTAINER_NAME.
    pub td_passivbot_by_engine: String,
    pub td_passivbot_container_name: String,
}
