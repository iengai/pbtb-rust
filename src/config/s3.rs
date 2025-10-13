use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct S3Config {
    pub endpoint_url: String,
    pub region: String,
    pub bucket_name: String,
}