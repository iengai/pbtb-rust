use async_trait::async_trait;

/// Data Transfer Object for bot list display
#[derive(Debug, Clone)]
pub struct BotListItemDto {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub has_config: bool,
}

/// Query interface for bot read operations
/// Defined in use case layer, implemented in infrastructure layer
#[async_trait]
pub trait BotQuery: Send + Sync {
    /// List bots for display (lightweight, no config data)
    async fn list_bots(&self, user_id: &str) -> Result<Vec<BotListItemDto>, String>;

    /// Check if bot exists
    async fn bot_exists(&self, user_id: &str, bot_id: &str) -> Result<bool, String>;
}