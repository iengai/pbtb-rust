use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Exchange {
    #[default]
    Bybit,
}

impl Exchange {
    // Deliberate inherent parser: returns Option, so it is not std::str::FromStr (which returns Result).
    #[allow(clippy::should_implement_trait)]
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
