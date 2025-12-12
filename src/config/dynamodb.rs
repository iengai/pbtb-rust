use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct DynamoDBConfig {
    pub endpoint_url: Option<String>,
    pub region: String,
    pub table_name: String,
}
