use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::domain::exchange::Exchange;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Stopped,
}

impl Default for Status {
    fn default() -> Self {
        Status::Stopped
    }
}

impl Status {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "running" => Some(Status::Running),
            "stopped" => Some(Status::Stopped),
            _ => None,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Running => "running",
            Status::Stopped => "stopped",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Bot {
    pub id: String,
    pub user_id: String,
    pub exchange: Exchange,
    pub name: String,
    pub api_key: String,
    pub secret_key: String,
    pub enabled: bool,
    pub status: Status,
    pub created_at: i64,  // Unix timestamp in seconds
    pub updated_at: i64,  // Unix timestamp in seconds
}

impl Bot {
    pub fn new(
        id: String,
        user_id: String,
        exchange: Exchange,
        name: String,
        api_key: String,
        secret_key: String,
        enabled: bool,
        status: Status,
        created_at: i64,
        updated_at: i64,
    ) -> Self {
        Self {
            id,
            user_id,
            exchange,
            name,
            api_key,
            secret_key,
            enabled,
            status,
            created_at,
            updated_at,
        }
    }
}

#[async_trait]
pub trait BotRepository: Send + Sync {
    async fn find(&self, user_id: &str, bot_id: &str) -> Option<Bot>;
    async fn save(&self, bot: &Bot);
    async fn find_by_user_id(&self, user_id: &str) -> Vec<Bot>;
    async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), String>;
}