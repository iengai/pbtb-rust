use std::sync::Arc;
use crate::domain::runtime::{BotRuntime, BotRuntimeRepository};

pub struct GetBotRuntimeUseCase { runtimes: Arc<dyn BotRuntimeRepository> }
impl GetBotRuntimeUseCase {
    pub fn new(runtimes: Arc<dyn BotRuntimeRepository>) -> Self { Self { runtimes } }
    pub async fn execute(&self, user_id: &str, bot_id: &str) -> Result<Option<BotRuntime>, String> {
        self.runtimes.find(user_id, bot_id).await.map_err(|e| e.to_string())
    }
}
