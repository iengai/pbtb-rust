use crate::domain::configtemplate::ConfigTemplateRepository;
use crate::domain::error::DomainError;
use crate::domain::exchange::Exchange;
use crate::domain::user::Role;
use std::sync::Arc;

/// One template as a chooser shows it: its id, the title a reader picks it by,
/// the exchange it was made for, so a chooser for one bot offers only that
/// bot's exchange, the level it asks for, so a surface can mark what the
/// caller cannot apply yet without hiding it, and whether it is the
/// operator's alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateListing {
    pub name: String,
    pub title: Option<String>,
    pub exchange: Exchange,
    pub min_vip_level: u8,
    pub operator_only: bool,
}

pub struct ListTemplatesUseCase {
    template_repository: Arc<dyn ConfigTemplateRepository>,
}

impl ListTemplatesUseCase {
    pub fn new(template_repository: Arc<dyn ConfigTemplateRepository>) -> Self {
        Self {
            template_repository,
        }
    }

    /// Every template the caller may be offered. A level hides nothing: the
    /// list is a catalogue, and seeing what a higher level unlocks is part of
    /// what it is for. A template addressed to the operator is another matter:
    /// no member could ever apply it, so it is left out unless `role` is the
    /// operator's. Both marks sit inside each template's file, so the listing
    /// reads each one; the catalogue is a handful of objects and this is a
    /// per-click read.
    pub async fn execute(&self, role: Role) -> Result<Vec<TemplateListing>, DomainError> {
        listings(self.template_repository.as_ref(), role).await
    }
}

async fn listings(
    templates: &dyn ConfigTemplateRepository,
    role: Role,
) -> Result<Vec<TemplateListing>, DomainError> {
    let mut listings = Vec::new();
    for name in templates.list().await? {
        let template = templates.get(&name).await?;
        let operator_only = template.is_operator_only();
        if operator_only && !role.is_operator() {
            continue;
        }
        // A template marked for an exchange this service does not trade on
        // could be applied to no bot; it is left out rather than failing the
        // whole listing.
        let exchange = match template.exchange() {
            Ok(exchange) => exchange,
            Err(e) => {
                tracing::warn!(template = %name, "template left out of the listing: {e}");
                continue;
            }
        };
        listings.push(TemplateListing {
            title: template.title().map(str::to_owned),
            exchange,
            min_vip_level: template.min_vip_level(),
            operator_only,
            name,
        });
    }
    Ok(listings)
}

/// The ids of the templates offered to everyone: what a member is listed, and
/// so what the public catalogue may name. The listing is one page of the
/// bucket's keys, so past a thousand templates one missing here reads as
/// retired on the public pages.
pub(crate) async fn published_names(
    templates: &dyn ConfigTemplateRepository,
) -> Result<Vec<String>, DomainError> {
    Ok(listings(templates, Role::Member)
        .await?
        .into_iter()
        .map(|listing| listing.name)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::configtemplate::ConfigTemplate;
    use async_trait::async_trait;
    use serde_json::json;

    struct Templates(Vec<ConfigTemplate>);
    #[async_trait]
    impl ConfigTemplateRepository for Templates {
        async fn get(&self, name: &str) -> Result<ConfigTemplate, DomainError> {
            self.0
                .iter()
                .find(|t| t.name == name)
                .cloned()
                .ok_or_else(|| DomainError::InvalidConfig(format!("no template {name}")))
        }
        async fn list(&self) -> Result<Vec<String>, DomainError> {
            Ok(self.0.iter().map(|t| t.name.clone()).collect())
        }
        async fn exists(&self, name: &str) -> Result<bool, DomainError> {
            Ok(self.0.iter().any(|t| t.name == name))
        }
        async fn save(&self, _template: &ConfigTemplate) -> Result<(), DomainError> {
            unreachable!("listing never writes")
        }
    }

    fn template(name: &str, config_data: serde_json::Value) -> ConfigTemplate {
        ConfigTemplate {
            name: name.to_string(),
            description: None,
            config_data,
            version: None,
        }
    }

    fn catalogue() -> Arc<Templates> {
        Arc::new(Templates(vec![
            template("open", json!({ "pbtb": {} })),
            template(
                "gated",
                json!({ "pbtb": { "min_vip_level": 5, "title": "Gated" } }),
            ),
            template(
                "internal",
                json!({ "pbtb": { "audience": "operator", "title": "Internal" } }),
            ),
        ]))
    }

    fn open() -> TemplateListing {
        TemplateListing {
            name: "open".to_string(),
            title: None,
            exchange: Exchange::Bybit,
            min_vip_level: 0,
            operator_only: false,
        }
    }

    fn gated() -> TemplateListing {
        TemplateListing {
            name: "gated".to_string(),
            title: Some("Gated".to_string()),
            exchange: Exchange::Bybit,
            min_vip_level: 5,
            operator_only: false,
        }
    }

    fn internal() -> TemplateListing {
        TemplateListing {
            name: "internal".to_string(),
            title: Some("Internal".to_string()),
            exchange: Exchange::Bybit,
            min_vip_level: 0,
            operator_only: true,
        }
    }

    #[tokio::test]
    async fn a_member_is_listed_no_operator_only_template() {
        let uc = ListTemplatesUseCase::new(catalogue());

        let listed = uc.execute(Role::Member).await.expect("list");

        assert_eq!(
            listed,
            vec![open(), gated()],
            "a level hides nothing; the operator's template is not offered"
        );
    }

    #[tokio::test]
    async fn the_operator_is_listed_every_template() {
        let uc = ListTemplatesUseCase::new(catalogue());

        let listed = uc.execute(Role::Operator).await.expect("list");

        assert_eq!(listed, vec![open(), gated(), internal()]);
    }

    #[tokio::test]
    async fn each_listing_carries_its_exchange_and_an_unknown_one_is_left_out() {
        let uc = ListTemplatesUseCase::new(Arc::new(Templates(vec![
            template("hl", json!({ "pbtb": { "exchange": "hyperliquid" } })),
            template("by", json!({ "pbtb": { "exchange": "bybit" } })),
            template("xx", json!({ "pbtb": { "exchange": "binance" } })),
        ])));

        let listed = uc.execute(Role::Member).await.expect("list");

        let got: Vec<_> = listed
            .iter()
            .map(|l| (l.name.as_str(), l.exchange))
            .collect();
        assert_eq!(
            got,
            vec![("hl", Exchange::Hyperliquid), ("by", Exchange::Bybit)]
        );
    }
}
