use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CallbackData {
    Action(CallbackAction),
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CallbackAction {
    Hello,
}

impl CallbackData {
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "unknown".to_string())
    }

    pub fn decode(s: &str) -> Self {
        serde_json::from_str(s).unwrap_or(CallbackData::Unknown)
    }
}