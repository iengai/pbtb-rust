use aws_config::BehaviorVersion;
use aws_sdk_dynamodb::{Client, types::AttributeDefinition, types::KeySchemaElement, types::KeyType, types::ScalarAttributeType, types::BillingMode};
use crate::config::configs::Configs;
use crate::config::dynamodb::DynamoDBConfig;
pub async fn create_dynamodb_client(config: &DynamoDBConfig) -> Client {
    let aws_config = aws_config::defaults(BehaviorVersion::latest())
        .region(aws_sdk_dynamodb::config::Region::new(config.region.clone()))
        .endpoint_url(&config.endpoint_url)
        .load()
        .await;

    Client::new(&aws_config)
}

pub async fn setup_dynamodb() -> Result<(Client, String), String> {
    let configs = match Configs::new() {
        Ok(s) => s,
        Err(e) => return Err(format!("Failed to load configs: {}", e)),
    };

    let client = create_dynamodb_client(&configs.dynamodb).await;
    let table_name = configs.dynamodb.table_name.clone();
    Ok((client, table_name))
}
