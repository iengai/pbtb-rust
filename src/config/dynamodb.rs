use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct DynamoDBConfig {
    pub endpoint_url: String,
    pub region: String,
    pub table_name: String,
}
