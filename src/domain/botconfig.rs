use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::domain::ConfigTemplate;
use crate::domain::error::DomainError;

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

/// Risk level value object containing long and short exposure limits.
/// Once constructed it is always within range [0.0, 10.0].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RiskLevel {
    pub long: f64,
    pub short: f64,
}

impl RiskLevel {
    /// Construct-validate: both long and short must be in [0.0, 10.0].
    pub fn new(long: f64, short: f64) -> Result<Self, DomainError> {
        Self::check(long)?;
        Self::check(short)?;
        Ok(Self { long, short })
    }

    fn check(value: f64) -> Result<(), DomainError> {
        if value < 0.0 || value > 10.0 {
            return Err(DomainError::RiskOutOfRange { value, min: 0.0, max: 10.0 });
        }
        Ok(())
    }
}

/// Leverage value object containing long and short leverage values.
/// Once constructed it is always within range [1.0, 125.0].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Leverage {
    pub long: f64,
    pub short: f64,
}

impl Leverage {
    /// Construct-validate: both long and short must be in [1.0, 125.0].
    pub fn new(long: f64, short: f64) -> Result<Self, DomainError> {
        Self::check(long)?;
        Self::check(short)?;
        Ok(Self { long, short })
    }

    /// Construct a leverage with long == short == v.
    pub fn uniform(v: f64) -> Result<Self, DomainError> {
        Self::new(v, v)
    }

    fn check(value: f64) -> Result<(), DomainError> {
        if value < 1.0 || value > 125.0 {
            return Err(DomainError::LeverageOutOfRange { value, min: 1.0, max: 125.0 });
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
    /// Create a new BotConfig from a template.
    /// The live.user field is overridden with the bot_id so the running task
    /// reports under the correct identity.
    pub fn from_template(
        user_id: String,
        bot_id: String,
        template: &ConfigTemplate,
        timestamp: i64,
    ) -> Result<Self, DomainError> {
        let mut config = Self {
            user_id,
            bot_id: bot_id.clone(),
            bot_type: BotType::Passivbot,
            template_name: template.name.clone(),
            template_version: template.version.clone(),
            config_data: template.config_data.clone(),
            created_at: timestamp,
            updated_at: timestamp,
        };
        config.set_live_user(&bot_id)?;
        Ok(config)
    }

    /// Update config data while preserving template reference
    pub fn update_config_data(&mut self, new_data: Value, timestamp: i64) {
        self.config_data = new_data;
        self.updated_at = timestamp;
    }

    /// Get risk level from config data
    /// Extracts from config["bot"]["long"]["total_wallet_exposure_limit"] and
    /// config["bot"]["short"]["total_wallet_exposure_limit"]
    pub fn risk_level(&self) -> Result<RiskLevel, DomainError> {
        let long = self.config_data
            .get("bot")
            .and_then(|bot| bot.get("long"))
            .and_then(|long| long.get("total_wallet_exposure_limit"))
            .and_then(|v| v.as_f64())
            .ok_or(DomainError::MissingConfigPath("bot.long.total_wallet_exposure_limit"))?;

        let short = self.config_data
            .get("bot")
            .and_then(|bot| bot.get("short"))
            .and_then(|short| short.get("total_wallet_exposure_limit"))
            .and_then(|v| v.as_f64())
            .ok_or(DomainError::MissingConfigPath("bot.short.total_wallet_exposure_limit"))?;

        RiskLevel::new(long, short)
    }

    /// Get leverage from config data
    /// Extracts from config["live"]["leverage"]
    /// Uses the same value for both long and short
    pub fn leverage(&self) -> Result<Leverage, DomainError> {
        let leverage_value = self.config_data
            .get("live")
            .and_then(|live| live.get("leverage"))
            .and_then(|v| v.as_f64())
            .ok_or(DomainError::MissingConfigPath("live.leverage"))?;

        Leverage::uniform(leverage_value)
    }

    /// Get approved coins from config data
    /// Extracts from config["live"]["approved_coins"]["long"] and
    /// config["live"]["approved_coins"]["short"]
    pub fn coins(&self) -> Result<Coins, DomainError> {
        let long_coins = self.config_data
            .get("live")
            .and_then(|live| live.get("approved_coins"))
            .and_then(|approved| approved.get("long"))
            .and_then(|v| v.as_array())
            .ok_or(DomainError::MissingConfigPath("live.approved_coins.long"))?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect::<Vec<String>>();

        let short_coins = self.config_data
            .get("live")
            .and_then(|live| live.get("approved_coins"))
            .and_then(|approved| approved.get("short"))
            .and_then(|v| v.as_array())
            .ok_or(DomainError::MissingConfigPath("live.approved_coins.short"))?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect::<Vec<String>>();

        Ok(Coins::new(long_coins, short_coins))
    }

    /// Update risk level in config data
    pub fn set_risk_level(&mut self, risk_level: &RiskLevel) -> Result<(), DomainError> {
        let bot = self.config_data
            .get_mut("bot")
            .ok_or(DomainError::MissingConfigPath("bot"))?;

        if let Some(long) = bot.get_mut("long") {
            long["total_wallet_exposure_limit"] = serde_json::json!(risk_level.long);
        } else {
            return Err(DomainError::MissingConfigPath("bot.long"));
        }

        if let Some(short) = bot.get_mut("short") {
            short["total_wallet_exposure_limit"] = serde_json::json!(risk_level.short);
        } else {
            return Err(DomainError::MissingConfigPath("bot.short"));
        }

        Ok(())
    }

    /// Update leverage in config data
    pub fn set_leverage(&mut self, leverage: &Leverage) -> Result<(), DomainError> {
        // For passivbot, we typically use a single leverage value
        // We'll use the long value as the primary leverage
        let live = self.config_data
            .get_mut("live")
            .ok_or(DomainError::MissingConfigPath("live"))?;

        live["leverage"] = serde_json::json!(leverage.long);

        Ok(())
    }

    /// Apply a risk level and derive the leverage from it.
    ///
    /// This is the single place the "leverage = max(long, short) + 1" policy
    /// lives. It sets the risk level, derives a uniform leverage, sets it, and
    /// bumps `updated_at`.
    pub fn apply_risk_level(&mut self, risk: &RiskLevel, now: i64) -> Result<(), DomainError> {
        self.set_risk_level(risk)?;
        let leverage = Leverage::uniform(risk.long.max(risk.short) + 1.0)?;
        self.set_leverage(&leverage)?;
        self.updated_at = now;
        Ok(())
    }

    /// Override the `live.user` field with the given bot id.
    /// Errors if the `live` section is missing or is not a JSON object.
    pub fn set_live_user(&mut self, bot_id: &str) -> Result<(), DomainError> {
        let live = self.config_data
            .get_mut("live")
            .ok_or(DomainError::MissingConfigPath("live"))?;

        let obj = live
            .as_object_mut()
            .ok_or_else(|| DomainError::InvalidConfig("config.live is not an object".to_string()))?;

        obj.insert("user".to_string(), Value::String(bot_id.to_string()));
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn risk_level_ok_and_out_of_range() {
        assert!(RiskLevel::new(3.0, 5.0).is_ok());
        match RiskLevel::new(11.0, 5.0) {
            Err(DomainError::RiskOutOfRange { value, min, max }) => {
                assert_eq!(value, 11.0);
                assert_eq!(min, 0.0);
                assert_eq!(max, 10.0);
            }
            other => panic!("expected RiskOutOfRange, got {:?}", other),
        }
        // short out of range (negative)
        match RiskLevel::new(1.0, -0.5) {
            Err(DomainError::RiskOutOfRange { value, .. }) => assert_eq!(value, -0.5),
            other => panic!("expected RiskOutOfRange, got {:?}", other),
        }
    }

    #[test]
    fn leverage_ok_out_of_range_and_uniform() {
        assert!(Leverage::new(5.0, 10.0).is_ok());
        match Leverage::new(0.5, 5.0) {
            Err(DomainError::LeverageOutOfRange { value, min, max }) => {
                assert_eq!(value, 0.5);
                assert_eq!(min, 1.0);
                assert_eq!(max, 125.0);
            }
            other => panic!("expected LeverageOutOfRange, got {:?}", other),
        }
        match Leverage::new(5.0, 200.0) {
            Err(DomainError::LeverageOutOfRange { value, .. }) => assert_eq!(value, 200.0),
            other => panic!("expected LeverageOutOfRange, got {:?}", other),
        }
        let u = Leverage::uniform(7.0).unwrap();
        assert_eq!(u.long, 7.0);
        assert_eq!(u.short, 7.0);
    }

    fn sample_config(now: i64) -> BotConfig {
        BotConfig {
            user_id: "u".into(),
            bot_id: "b".into(),
            bot_type: BotType::Passivbot,
            template_name: "t".into(),
            template_version: None,
            config_data: json!({
                "bot": {
                    "long": { "total_wallet_exposure_limit": 1.0 },
                    "short": { "total_wallet_exposure_limit": 1.0 }
                },
                "live": { "leverage": 2.0 }
            }),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn apply_risk_level_derives_leverage() {
        let mut config = sample_config(0);
        let risk = RiskLevel::new(3.0, 5.0).unwrap();
        config.apply_risk_level(&risk, 999).unwrap();

        let stored_risk = config.risk_level().unwrap();
        assert_eq!(stored_risk.long, 3.0);
        assert_eq!(stored_risk.short, 5.0);

        // leverage = max(3,5) + 1 = 6.0
        let lev = config.leverage().unwrap();
        assert_eq!(lev.long, 6.0);
        assert_eq!(lev.short, 6.0);

        assert_eq!(config.updated_at, 999);
    }

    #[test]
    fn set_live_user_sets_and_validates() {
        let mut config = sample_config(0);
        config.set_live_user("bot-xyz").unwrap();
        assert_eq!(
            config.config_data["live"]["user"].as_str(),
            Some("bot-xyz")
        );

        // missing live
        let mut bad = sample_config(0);
        bad.config_data = json!({});
        match bad.set_live_user("x") {
            Err(DomainError::MissingConfigPath("live")) => {}
            other => panic!("expected MissingConfigPath(live), got {:?}", other),
        }

        // live not an object
        let mut bad2 = sample_config(0);
        bad2.config_data = json!({ "live": 5 });
        match bad2.set_live_user("x") {
            Err(DomainError::InvalidConfig(_)) => {}
            other => panic!("expected InvalidConfig, got {:?}", other),
        }
    }

    #[test]
    fn from_template_sets_live_user() {
        let template = ConfigTemplate {
            name: "t".into(),
            description: None,
            config_data: json!({
                "bot": {
                    "long": { "total_wallet_exposure_limit": 1.0 },
                    "short": { "total_wallet_exposure_limit": 1.0 }
                },
                "live": { "leverage": 2.0 }
            }),
            version: None,
        };
        let config = BotConfig::from_template("u".into(), "bot-id".into(), &template, 10).unwrap();
        assert_eq!(config.config_data["live"]["user"].as_str(), Some("bot-id"));
    }
}
