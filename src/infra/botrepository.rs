use std::collections::HashMap;
use aws_sdk_dynamodb::{Client};
use aws_sdk_dynamodb::types::AttributeValue;
use crate::domain::bot::{Bot, BotRepository};
use async_trait::async_trait;

/// infra 层的存储模型
pub struct BotItem {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub api_key: String,
    pub secret_key: String,
    pub enabled: bool,
}

impl BotItem {
    pub fn from_item(item: &HashMap<String, AttributeValue>) -> Option<Self> {
        Some(Self {
            id: item.get("id")?.as_s().ok()?.to_string(),
            user_id: item.get("user_id")?.as_s().ok()?.to_string(),
            name: item.get("name")?.as_s().ok()?.to_string(),
            api_key: item.get("api_key")?.as_s().ok()?.to_string(),
            secret_key: item.get("secret_key")?.as_s().ok()?.to_string(),
            enabled: item.get("enabled")?.as_bool().ok().copied()?,
        })
    }

    pub fn to_item(&self) -> HashMap<String, AttributeValue> {
        let mut map = HashMap::new();
        map.insert("id".to_string(), AttributeValue::S(self.id.clone()));
        map.insert("user_id".to_string(), AttributeValue::S(self.user_id.clone()));
        map.insert("name".to_string(), AttributeValue::S(self.name.clone()));
        map.insert("api_key".to_string(), AttributeValue::S(self.api_key.clone()));
        map.insert("secret_key".to_string(), AttributeValue::S(self.secret_key.clone()));
        map.insert("enabled".to_string(), AttributeValue::Bool(self.enabled));
        map
    }

    pub fn to_domain(&self) -> Bot {
        Bot {
            id: self.id.clone(),
            user_id: self.user_id.clone(),
            name: self.name.clone(),
            api_key: self.api_key.clone(),
            secret_key: self.secret_key.clone(),
            enabled: self.enabled,
        }
    }

    pub fn from_domain(bot: &Bot) -> Self {
        Self {
            id: bot.id.clone(),
            user_id: bot.user_id.clone(),
            name: bot.name.clone(),
            api_key: bot.api_key.clone(),
            secret_key: bot.secret_key.clone(),
            enabled: bot.enabled,
        }
    }
}

pub struct DynamoBotRepository {
    client: Client,
    table_name: String,
}

impl DynamoBotRepository {
    pub fn new(client: Client, table_name: String) -> Self {
        Self { client, table_name }
    }
}

#[async_trait]
impl BotRepository for DynamoBotRepository {
    async fn find_by_id(&self, id: &str) -> Option<Bot> {
        let output = self.client
            .get_item()
            .table_name(&self.table_name)
            .key("id", AttributeValue::S(id.to_string()))
            .send()
            .await
            .ok()?;

        let item = output.item?;
        let bot_item = BotItem::from_item(&item)?;
        Some(bot_item.to_domain())
    }

    async fn save(&self, bot: &Bot) {
        let bot_item = BotItem::from_domain(bot);
        let item = bot_item.to_item();

        if let Err(e) = self.client
            .put_item()
            .table_name(&self.table_name)
            .set_item(Some(item))
            .send()
            .await
        {
            eprintln!("DynamoDB put_item error: {:?}", e);
        }
    }
}
