use aws_config::BehaviorVersion;
use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_s3::Client as S3Client;
use crate::config::configs::Configs;
use crate::config::dynamodb::DynamoDBConfig;
use crate::config::s3::S3Config;

pub async fn create_dynamodb_client(config: &DynamoDBConfig) -> DynamoDbClient {
    let aws_config = aws_config::defaults(BehaviorVersion::latest())
        .region(aws_sdk_dynamodb::config::Region::new(config.region.clone()))
        .endpoint_url(&config.endpoint_url)
        .load()
        .await;

    DynamoDbClient::new(&aws_config)
}

pub async fn create_s3_client(config: &S3Config) -> S3Client {
    let mut config_builder = aws_config::defaults(BehaviorVersion::latest())
        .region(aws_sdk_s3::config::Region::new(config.region.clone()));

    // Only set endpoint_url if it's not the default AWS endpoint
    // This allows AWS SDK to use credentials from ~/.aws/credentials
    if !config.endpoint_url.is_empty()
        && !config.endpoint_url.starts_with("https://s3.")
        && !config.endpoint_url.starts_with("https://s3-") {
        config_builder = config_builder.endpoint_url(&config.endpoint_url);
    }

    let aws_config = config_builder.load().await;

    S3Client::new(&aws_config)
}

pub async fn setup_dynamodb() -> Result<(DynamoDbClient, String), String> {
    let configs = match Configs::new() {
        Ok(s) => s,
        Err(e) => return Err(format!("Failed to load configs: {}", e)),
    };

    let client = create_dynamodb_client(&configs.dynamodb).await;
    let table_name = configs.dynamodb.table_name.clone();
    Ok((client, table_name))
}

pub async fn setup_s3() -> Result<(S3Client, String), String> {
    let configs = match Configs::new() {
        Ok(s) => s,
        Err(e) => return Err(format!("Failed to load configs: {}", e)),
    };

    let client = create_s3_client(&configs.s3).await;
    let bucket_name = configs.s3.bucket_name.clone();
    Ok((client, bucket_name))
}