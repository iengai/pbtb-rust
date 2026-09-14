//! The public catalogue's audience overlay. The committed backtest snapshot the
//! public pages list templates from carries each one's audience as it was when
//! the snapshot was built; the overlay says which templates are published now,
//! so a publish or retire reaches those pages without a rebuild.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::error::DomainError;

/// The ids of the templates offered to everyone: exactly what a member is
/// listed, so publishing it tells a reader nothing a signed-in member cannot
/// already see. A template it does not name is retired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedTemplates {
    pub generated_at: i64,
    pub published: Vec<String>,
}

#[async_trait]
pub trait TemplateAudiencePublisher: Send + Sync {
    /// Replace the public overlay with this set.
    async fn publish(&self, published: &PublishedTemplates) -> Result<(), DomainError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_overlay_is_the_shape_the_site_and_the_scripts_read() {
        let overlay = PublishedTemplates {
            generated_at: 1_700_000_000,
            published: vec!["tpl-a".into(), "tpl-b".into()],
        };
        assert_eq!(
            serde_json::to_value(&overlay).unwrap(),
            json!({ "generated_at": 1_700_000_000, "published": ["tpl-a", "tpl-b"] })
        );
    }
}
