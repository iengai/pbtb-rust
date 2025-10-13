use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::domain::ConfigTemplate;

/// Bot type enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BotType {
    Passivbot,
}

impl Default for BotType {
    fn default() -> Self {
        BotType::Passivbot
    }
}

/// Risk level value object containing long and short exposure limits
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RiskLevel {
    pub long: f64,
    pub short: f64,
}

impl RiskLevel {
    pub fn new(long: f64, short: f64) -> Self {
        Self { long, short }
    }

    /// Validate risk levels are within acceptable range
    pub fn validate(&self) -> Result<(), String> {
        if self.long < 0.0 || self.long > 10.0 {
            return Err(format!("Long risk level {} is out of range [0.0, 10.0]", self.long));
        }
        if self.short < 0.0 || self.short > 10.0 {
            return Err(format!("Short risk level {} is out of range [0.0, 10.0]", self.short));
        }
        Ok(())
    }
}

/// Leverage value object containing long and short leverage values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Leverage {
    pub long: f64,
    pub short: f64,
}

impl Leverage {
    pub fn new(long: f64, short: f64) -> Self {
        Self { long, short }
    }

    /// Validate leverage values are within acceptable range
    pub fn validate(&self) -> Result<(), String> {
        if self.long < 1.0 || self.long > 125.0 {
            return Err(format!("Long leverage {} is out of range [1.0, 125.0]", self.long));
        }
        if self.short < 1.0 || self.short > 125.0 {
            return Err(format!("Short leverage {} is out of range [1.0, 125.0]", self.short));
        }
        Ok(())
    }
}

/// Coins value object containing approved coins for long and short positions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Coins {
    pub long: Vec<String>,
    pub short: Vec<String>,
}

impl Coins {
    pub fn new(long: Vec<String>, short: Vec<String>) -> Self {
        Self { long, short }
    }

    /// Get all unique coins from both long and short
    pub fn all_coins(&self) -> Vec<String> {
        let mut all: Vec<String> = self.long.iter().cloned()
            .chain(self.short.iter().cloned())
            .collect();
        all.sort();
        all.dedup();
        all
    }

    /// Check if a coin is approved for trading
    pub fn is_approved(&self, coin: &str) -> bool {
        self.long.contains(&coin.to_string()) || self.short.contains(&coin.to_string())
    }

    /// Check if empty (no coins approved)
    pub fn is_empty(&self) -> bool {
        self.long.is_empty() && self.short.is_empty()
    }
}

/// Bot configuration entity (user's bot-specific config)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    pub user_id: String,
    pub bot_id: String,

    /// Bot type (currently only passivbot)
    #[serde(rename = "type", default)]
    pub bot_type: BotType,

    /// Reference to the original template name
    pub template_name: String,

    /// Template version when applied
    pub template_version: Option<String>,

    /// Custom configuration data (can override template values)
    /// This contains the full passivbot configuration
    pub config_data: Value,

    pub created_at: i64,
    pub updated_at: i64,
}

impl BotConfig {
    /// Create a new BotConfig from a template
    pub fn from_template(
        user_id: String,
        bot_id: String,
        template: &ConfigTemplate,
        timestamp: i64,
    ) -> Self {
        Self {
            user_id,
            bot_id,
            bot_type: BotType::Passivbot,
            template_name: template.name.clone(),
            template_version: template.version.clone(),
            config_data: template.config_data.clone(),
            created_at: timestamp,
            updated_at: timestamp,
        }
    }

    /// Update config data while preserving template reference
    pub fn update_config_data(&mut self, new_data: Value, timestamp: i64) {
        self.config_data = new_data;
        self.updated_at = timestamp;
    }

    /// Get risk level from config data
    /// Extracts from config["bot"]["long"]["total_wallet_exposure_limit"] and
    /// config["bot"]["short"]["total_wallet_exposure_limit"]
    pub fn risk_level(&self) -> Result<RiskLevel, String> {
        let long = self.config_data
            .get("bot")
            .and_then(|bot| bot.get("long"))
            .and_then(|long| long.get("total_wallet_exposure_limit"))
            .and_then(|v| v.as_f64())
            .ok_or_else(|| "Missing bot.long.total_wallet_exposure_limit".to_string())?;

        let short = self.config_data
            .get("bot")
            .and_then(|bot| bot.get("short"))
            .and_then(|short| short.get("total_wallet_exposure_limit"))
            .and_then(|v| v.as_f64())
            .ok_or_else(|| "Missing bot.short.total_wallet_exposure_limit".to_string())?;

        let risk_level = RiskLevel::new(long, short);
        risk_level.validate()?;
        Ok(risk_level)
    }

    /// Get leverage from config data
    /// Extracts from config["live"]["leverage"]
    /// Uses the same value for both long and short
    pub fn leverage(&self) -> Result<Leverage, String> {
        let leverage_value = self.config_data
            .get("live")
            .and_then(|live| live.get("leverage"))
            .and_then(|v| v.as_f64())
            .ok_or_else(|| "Missing live.leverage".to_string())?;

        let leverage = Leverage::new(leverage_value, leverage_value);
        leverage.validate()?;
        Ok(leverage)
    }

    /// Get approved coins from config data
    /// Extracts from config["live"]["approved_coins"]["long"] and
    /// config["live"]["approved_coins"]["short"]
    pub fn coins(&self) -> Result<Coins, String> {
        let long_coins = self.config_data
            .get("live")
            .and_then(|live| live.get("approved_coins"))
            .and_then(|approved| approved.get("long"))
            .and_then(|v| v.as_array())
            .ok_or_else(|| "Missing live.approved_coins.long".to_string())?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect::<Vec<String>>();

        let short_coins = self.config_data
            .get("live")
            .and_then(|live| live.get("approved_coins"))
            .and_then(|approved| approved.get("short"))
            .and_then(|v| v.as_array())
            .ok_or_else(|| "Missing live.approved_coins.short".to_string())?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect::<Vec<String>>();

        Ok(Coins::new(long_coins, short_coins))
    }

    /// Update risk level in config data
    pub fn set_risk_level(&mut self, risk_level: &RiskLevel) -> Result<(), String> {
        risk_level.validate()?;

        let bot = self.config_data
            .get_mut("bot")
            .ok_or_else(|| "Missing bot section".to_string())?;

        if let Some(long) = bot.get_mut("long") {
            long["total_wallet_exposure_limit"] = serde_json::json!(risk_level.long);
        } else {
            return Err("Missing bot.long section".to_string());
        }

        if let Some(short) = bot.get_mut("short") {
            short["total_wallet_exposure_limit"] = serde_json::json!(risk_level.short);
        } else {
            return Err("Missing bot.short section".to_string());
        }

        Ok(())
    }

    /// Update leverage in config data
    pub fn set_leverage(&mut self, leverage: &Leverage) -> Result<(), String> {
        leverage.validate()?;

        // For passivbot, we typically use a single leverage value
        // We'll use the long value as the primary leverage
        let live = self.config_data
            .get_mut("live")
            .ok_or_else(|| "Missing live section".to_string())?;

        live["leverage"] = serde_json::json!(leverage.long);

        Ok(())
    }

    /// Get a summary of the configuration
    pub fn summary(&self) -> String {
        let risk_level = self.risk_level()
            .map(|r| format!("Long: {:.2}, Short: {:.2}", r.long, r.short))
            .unwrap_or_else(|_| "N/A".to_string());

        let leverage = self.leverage()
            .map(|l| format!("{:.1}x", l.long))
            .unwrap_or_else(|_| "N/A".to_string());

        let coins = self.coins()
            .map(|c| format!("Long: {}, Short: {}", c.long.join(", "), c.short.join(", ")))
            .unwrap_or_else(|_| "N/A".to_string());

        format!(
            "Bot Config Summary:\n\
            Type: {:?}\n\
            Template: {} (v{:?})\n\
            Risk Level: {}\n\
            Leverage: {}\n\
            Coins: {}",
            self.bot_type,
            self.template_name,
            self.template_version,
            risk_level,
            leverage,
            coins
        )
    }
}

/// Repository interface for bot configurations
#[async_trait]
pub trait BotConfigRepository: Send + Sync {
    /// Get bot-specific configuration
    async fn get(&self, user_id: &str, bot_id: &str) -> Result<BotConfig, String>;

    /// Save bot-specific configuration
    async fn save(&self, config: &BotConfig) -> Result<(), String>;

    /// Delete bot-specific configuration
    async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), String>;

    /// Check if bot has a configuration
    async fn exists(&self, user_id: &str, bot_id: &str) -> Result<bool, String>;
}