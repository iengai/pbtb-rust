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

pub async fn ensure_table_exists(client: &Client, table_name: &str) -> Result<(), String> {
    // Check if table exists
    let tables = match client.list_tables().send().await {
        Ok(t) => t,
        Err(e) => return Err(format!("Failed to list tables: {}", e)),
    };

    // Check if table exists in the list
    let names = tables.table_names();
    if names.contains(&table_name.to_string()) {
        return Ok(());
    }

    // Create table if it doesn't exist
    let key_schema = match KeySchemaElement::builder()
        .attribute_name("id")
        .key_type(KeyType::Hash)
        .build() {
        Ok(ks) => ks,
        Err(e) => return Err(format!("Failed to build key schema: {}", e)),
    };

    let attr_def = match AttributeDefinition::builder()
        .attribute_name("id")
        .attribute_type(ScalarAttributeType::S)
        .build() {
        Ok(ad) => ad,
        Err(e) => return Err(format!("Failed to build attribute definition: {}", e)),
    };

    match client.create_table()
        .table_name(table_name)
        .key_schema(key_schema)
        .attribute_definitions(attr_def)
        .billing_mode(BillingMode::PayPerRequest)
        .send()
        .await {
        Ok(_) => {},
        Err(e) => return Err(format!("Failed to create table: {}", e)),
    };

    // Wait for table to be created by polling
    let max_attempts = 10;
    for attempt in 1..=max_attempts {
        println!("Waiting for table to be created (attempt {}/{})", attempt, max_attempts);

        let describe_result = client.describe_table()
            .table_name(table_name)
            .send()
            .await;

        match describe_result {
            Ok(output) => {
                if let Some(table_description) = output.table() {
                    if let Some(status) = table_description.table_status() {
                        if status == &aws_sdk_dynamodb::types::TableStatus::Active {
                            println!("Table is now active");
                            return Ok(());
                        }
                    }
                }
            },
            Err(e) => {
                println!("Error checking table status: {:?}", e);
            }
        }

        // Wait before next attempt
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }

    Err(format!("Table {} did not become active after {} attempts", table_name, max_attempts))
}

pub async fn setup_dynamodb() -> Result<(Client, String), String> {
    let configs = match Configs::new() {
        Ok(s) => s,
        Err(e) => return Err(format!("Failed to load configs: {}", e)),
    };

    let client = create_dynamodb_client(&configs.dynamodb).await;
    let table_name = configs.dynamodb.table_name.clone();
    println!("{}", table_name);

    ensure_table_exists(&client, &table_name).await?;

    Ok((client, table_name))
}
