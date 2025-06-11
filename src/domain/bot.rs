use async_trait::async_trait;

pub struct Bot {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub api_key: String,
    pub secret_key: String,
    pub enabled: bool,
}

impl Bot {
    pub fn new(
        id: String,
        user_id: String,
        name: String,
        api_key: String,
        secret_key: String,
        enabled: bool,
    ) -> Self {
        Self {
            id,
            user_id,
            name,
            api_key,
            secret_key,
            enabled,
        }
    }
}
#[async_trait]
pub trait BotRepository {
    async fn find_by_id(&self, id: &str) -> Option<Bot>;
    async fn save(&self, bot: &Bot);
}