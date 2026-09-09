use crate::domain::error::DomainError;
use crate::domain::returncurve::ReturnCurveRepository;
use serde_json::Value;
use std::sync::Arc;

/// A bot's return series for its owner. Tenancy is the key: the series is
/// looked up under the caller's own `user_id`, so another account's bot of the
/// same name is simply not there.
pub struct GetBotReturnsUseCase {
    curves: Arc<dyn ReturnCurveRepository>,
}

impl GetBotReturnsUseCase {
    pub fn new(curves: Arc<dyn ReturnCurveRepository>) -> Self {
        Self { curves }
    }

    /// `None` when the collector has not written a series yet.
    pub async fn execute(&self, user_id: &str, bot_id: &str) -> Result<Option<Value>, DomainError> {
        self.curves.get(user_id, bot_id).await
    }
}
