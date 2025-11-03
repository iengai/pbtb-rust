use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Exchange {
    Bybit,
}

impl Default for Exchange {
    fn default() -> Self {
        Exchange::Bybit
    }
}

impl Exchange {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "bybit" => Some(Exchange::Bybit),
            _ => None,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Exchange::Bybit => "bybit",
        }
    }
}