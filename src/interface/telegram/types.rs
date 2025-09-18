// Rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", content = "p")]
pub enum CallbackData {
    Action(CallbackAction),
    // 可扩展其它类型，如分页、参数携带等
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CallbackAction {
    Hello,
}

impl CallbackData {
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
    pub fn decode(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str::<Self>(s)
    }
}