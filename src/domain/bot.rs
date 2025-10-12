use async_trait::async_trait;

pub struct Bot {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub api_key: String,
    pub secret_key: String,
    pub enabled: bool,
    pub created_at: i64,  // Unix timestamp in seconds
    pub updated_at: i64,  // Unix timestamp in seconds
}

impl Bot {
    pub fn new(
        id: String,
        user_id: String,
        name: String,
        api_key: String,
        secret_key: String,
        enabled: bool,
        created_at: i64,
        updated_at: i64,
    ) -> Self {
        Self {
            id,
            user_id,
            name,
            api_key,
            secret_key,
            enabled,
            created_at,
            updated_at,
        }
    }
}

#[async_trait]
pub trait BotRepository: Send + Sync {
    async fn find_by_id(&self, id: &str) -> Option<Bot>;
    async fn save(&self, bot: &Bot);
    async fn find_by_user_id(&self, user_id: &str) -> Vec<Bot>;
}