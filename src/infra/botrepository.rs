use std::collections::HashMap;
use aws_sdk_dynamodb::{Client};
use aws_sdk_dynamodb::types::AttributeValue;
use crate::domain::bot::{Bot, BotRepository, Status};
use async_trait::async_trait;
use crate::domain::exchange::Exchange;

/// Storage model for the infra layer
pub struct BotItem {
    pub pk: String,        // user_id#<user_id>
    pub sk: String,        // <bot_id>
    pub name: String,
    pub exchange: String,
    pub api_key: String,
    pub secret_key: String,
    pub enabled: bool,
    pub status: String,
    pub created_at: i64,   // Unix timestamp in seconds
    pub updated_at: i64,   // Unix timestamp in seconds
}

fn parse_status(item: &HashMap<String, AttributeValue>) -> String {
    item.get("status")
        .and_then(|v| v.as_s().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Status::default().as_str().to_string())
}

impl BotItem {
    /// Extract user_id from PK format: "user_id#<user_id>"
    fn extract_user_id_from_pk(pk: &str) -> Option<String> {
        pk.strip_prefix("user_id#").map(|s| s.to_string())
    }

    /// Construct PK from user_id
    fn construct_pk(user_id: &str) -> String {
        format!("user_id#{}", user_id)
    }

    fn from_item(item: &HashMap<String, AttributeValue>) -> Option<Self> {
        Some(Self {
            pk: item.get("pk")?.as_s().ok()?.to_string(),
            sk: item.get("sk")?.as_s().ok()?.to_string(),
            exchange: item.get("exchange")?.as_s().ok()?.to_string(),
            name: item.get("name")?.as_s().ok()?.to_string(),
            api_key: item.get("api_key")?.as_s().ok()?.to_string(),
            secret_key: item.get("secret_key")?.as_s().ok()?.to_string(),
            enabled: item.get("enabled")?.as_bool().ok().copied()?,
            status: parse_status(item),
            created_at: item.get("created_at")?.as_n().ok()?.parse().ok()?,
            updated_at: item.get("updated_at")?.as_n().ok()?.parse().ok()?,
        })
    }

    fn to_item(&self) -> HashMap<String, AttributeValue> {
        let mut map = HashMap::new();
        map.insert("pk".to_string(), AttributeValue::S(self.pk.clone()));
        map.insert("sk".to_string(), AttributeValue::S(self.sk.clone()));
        map.insert("exchange".to_string(), AttributeValue::S(self.exchange.clone()));
        map.insert("name".to_string(), AttributeValue::S(self.name.clone()));
        map.insert("api_key".to_string(), AttributeValue::S(self.api_key.clone()));
        map.insert("secret_key".to_string(), AttributeValue::S(self.secret_key.clone()));
        map.insert("enabled".to_string(), AttributeValue::Bool(self.enabled));
        map.insert("created_at".to_string(), AttributeValue::N(self.created_at.to_string()));
        map.insert("updated_at".to_string(), AttributeValue::N(self.updated_at.to_string()));
        map
    }

    fn to_domain(&self) -> Option<Bot> {
        let user_id = Self::extract_user_id_from_pk(&self.pk)?;
        let exchange = Exchange::from_str(self.exchange.as_str())?;
        let status = Status::from_str(self.status.as_str())?;
        Some(Bot {
            id: self.sk.clone(),  // bot_id from SK
            user_id,
            exchange,
            name: self.name.clone(),
            api_key: self.api_key.clone(),
            secret_key: self.secret_key.clone(),
            enabled: self.enabled,
            status,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }

    fn from_domain(bot: &Bot) -> Self {
        Self {
            pk: Self::construct_pk(&bot.user_id),
            sk: bot.id.clone(),
            exchange: bot.exchange.as_str().to_string(),
            name: bot.name.clone(),
            api_key: bot.api_key.clone(),
            secret_key: bot.secret_key.clone(),
            enabled: bot.enabled,
            status: bot.status.as_str().to_string(),
            created_at: bot.created_at,
            updated_at: bot.updated_at,
        }
    }
}

pub struct BotECSTaskMetadata {
    pub pk: String,        // user_id#<user_id>
    pub sk: String,
    // ecs task metadata
    pub status: String,
    pub task_id: String,
    pub updated_at: i64,
    pub task_current_version: i64,
}

impl BotECSTaskMetadata {
    fn construct_sk(bot_id: &str) -> String {
        format!("ecs_task_metadata#{}", bot_id)
    }

    fn get_bot_id(&self) -> String {
        self.sk.strip_prefix("ecs_task_metadata#").unwrap().to_string()
    }
    fn from_item(item: &HashMap<String, AttributeValue>) -> Option<Self> {
        Some(Self{
            pk: item.get("pk")?.as_s().ok()?.to_string(),
            sk: item.get("sk")?.as_s().ok()?.to_string(),
            status: parse_status(item),
            task_id: item.get("task_id")?.as_s().ok()?.to_string(),
            updated_at: item.get("task_updated_at")?.as_n().ok()?.parse().ok()?,
            task_current_version: item.get("task_current_version")?.as_n().ok()?.parse().ok()?
        })
    }
    fn to_item(&self) -> HashMap<String, AttributeValue> {
        let mut map = HashMap::new();
        map.insert("pk".to_string(), AttributeValue::S(self.pk.clone()));
        map.insert("sk".to_string(), AttributeValue::S(self.sk.clone()));
        map.insert("status".to_string(), AttributeValue::S(self.status.clone()));
        map.insert("task_id".to_string(), AttributeValue::S(self.task_id.clone()));
        map.insert("task_updated_at".to_string(), AttributeValue::N(self.updated_at.to_string()));
        map.insert("task_current_version".to_string(), AttributeValue::N(self.task_current_version.to_string()));
        map
    }
}
#[async_trait]
pub trait BotECSTaskMetadataRepository {
    async fn find_task_meta_data(&self, user_id: &str, bot_id: &str) -> Option<BotECSTaskMetadata>;
    async fn save_task_meta_data(&self, metadata: &BotECSTaskMetadata);
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
impl BotECSTaskMetadataRepository for DynamoBotRepository {
    async fn find_task_meta_data(&self, user_id: &str, bot_id: &str) -> Option<BotECSTaskMetadata> {
        let result = self.client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(BotItem::construct_pk(user_id)))
            .key("sk", AttributeValue::S(BotECSTaskMetadata::construct_sk(bot_id)))
            .send()
            .await
            .ok()?;

        let item = result.item()?;
        Option::from(BotECSTaskMetadata::from_item(item))
    }

    async fn save_task_meta_data(&self, metadata: &BotECSTaskMetadata) {
        use aws_sdk_dynamodb::types::{TransactWriteItem, Put, Update};

        let metadata_put = Put::builder()
            .table_name(&self.table_name)
            .set_item(Some(metadata.to_item()))
            .build()
            .unwrap();

        let bot_id = metadata.get_bot_id();
        let bot_update = Update::builder()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(metadata.pk.clone()))
            .key("sk", AttributeValue::S(bot_id.clone()))
            .update_expression("SET #stat = :stat")
            .expression_attribute_names("#stat", "status")
            .expression_attribute_values(":stat", AttributeValue::S(metadata.status.clone()))
            .build()
            .unwrap();

        let result = self.client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(metadata_put).build())
            .transact_items(TransactWriteItem::builder().update(bot_update).build())
            .send()
            .await;

        if let Err(e) = result {
            eprintln!("Failed to execute DynamoDB transaction: {:?}", e);
        }
    }
}

#[async_trait]
impl BotRepository for DynamoBotRepository {
    async fn find(&self, user_id: &str, bot_id: &str) -> Option<Bot> {
        // Note: We need user_id to construct PK, so this method uses scan (not efficient)
        // In production, consider adding bot_id as GSI or passing user_id as well
        let result = self.client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(BotItem::construct_pk(user_id)))
            .key("sk", AttributeValue::S(bot_id.to_string()))
            .send()
            .await
            .ok()?;

        let item = result.item()?;
        let bot_item = BotItem::from_item(item)?;
        bot_item.to_domain()
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

    async fn find_by_user_id(&self, user_id: &str) -> Vec<Bot> {
        let pk_value = BotItem::construct_pk(user_id);

        let result = self.client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("pk = :pk")
            .expression_attribute_values(":pk", AttributeValue::S(pk_value))
            .send()
            .await;

        match result {
            Ok(output) => {
                output.items()
                    .iter()
                    .filter_map(|item| BotItem::from_item(item))
                    .filter_map(|bot_item| bot_item.to_domain())
                    .collect()
            }
            Err(e) => {
                eprintln!("DynamoDB query error: {:?}", e);
                Vec::new()
            }
        }
    }
    
    async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), String> {
        let pk_value = BotItem::construct_pk(user_id);

        self.client
            .delete_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk_value))
            .key("sk", AttributeValue::S(bot_id.to_string()))
            .send()
            .await
            .map_err(|e| format!("Failed to delete bot: {:?}", e))?;

        Ok(())
    }
}