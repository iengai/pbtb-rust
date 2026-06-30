use crate::domain::error::DomainError;
use crate::domain::runtime::{BotRuntime, BotRuntimeRepository};
use std::sync::Arc;

pub struct GetBotRuntimeUseCase {
    runtimes: Arc<dyn BotRuntimeRepository>,
}
impl GetBotRuntimeUseCase {
    pub fn new(runtimes: Arc<dyn BotRuntimeRepository>) -> Self {
        Self { runtimes }
    }
    pub async fn execute(
        &self,
        user_id: &str,
        bot_id: &str,
    ) -> Result<Option<BotRuntime>, DomainError> {
        self.runtimes.find(user_id, bot_id).await
    }
}
