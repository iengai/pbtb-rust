use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_s3::Client as S3Client;
use aws_sdk_ecs::Client as EcsClient;
use crate::config::configs::{load_config, Configs};
use crate::config::dynamodb::DynamoDBConfig;
use crate::config::s3::S3Config;
use crate::config::ecs::EcsConfig;

pub async fn create_dynamodb_client(config: &DynamoDBConfig) -> DynamoDbClient {
    let mut builder = aws_config::defaults(BehaviorVersion::latest())
        .region(aws_sdk_dynamodb::config::Region::new(config.region.clone()));

    if let Some(endpoint) = config.endpoint_url.as_deref() {
        builder = builder.endpoint_url(endpoint);
    }

    let aws_config = builder.load().await;
    DynamoDbClient::new(&aws_config)
}

pub async fn setup_dynamodb_with_configs(configs: &Configs) -> (DynamoDbClient, String) {
    let client = create_dynamodb_client(&configs.dynamodb).await;
    let table_name = configs.dynamodb.table_name.clone();
    (client, table_name)
}

pub async fn setup_dynamodb() -> Result<(DynamoDbClient, String)> {
    let configs: Configs = load_config().context("Failed to load configs")?;
    Ok(setup_dynamodb_with_configs(&configs).await)
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

pub async fn setup_s3_with_configs(configs: &Configs) -> (S3Client, String) {
    let client = create_s3_client(&configs.s3).await;
    let bucket_name = configs.s3.bucket_name.clone();
    (client, bucket_name)
}

pub async fn setup_s3() -> Result<(S3Client, String)> {
    let configs: Configs = load_config().context("Failed to load configs")?;
    Ok(setup_s3_with_configs(&configs).await)
}

pub async fn create_ecs_client(config: &EcsConfig) -> EcsClient {
    let aws_config = aws_config::defaults(BehaviorVersion::latest())
        .region(aws_sdk_ecs::config::Region::new(config.region.clone()))
        .load()
        .await;

    EcsClient::new(&aws_config)
}

pub async fn setup_ecs_with_configs(configs: &Configs) -> (EcsClient, String, String) {
    let client = create_ecs_client(&configs.ecs).await;
    (
        client,
        configs.ecs.cluster_arn.clone(),
        configs.ecs.td_passivbot_v741_arn.clone(),
    )
}

pub async fn setup_ecs() -> Result<(EcsClient, String, String)> {
    let configs: Configs = load_config().context("Failed to load configs")?;
    Ok(setup_ecs_with_configs(&configs).await)
}