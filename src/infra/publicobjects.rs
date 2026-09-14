//! The object operations the chart bucket's public prefix is written with,
//! shared by the showcase store and the template audience store. A seam of its
//! own so each store runs in tests exactly as it runs against the bucket.

use async_trait::async_trait;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;

use crate::domain::error::DomainError;
use crate::infra::aws_error::{repo_err, sdk_err};

/// How long a cache may reuse a public object. A switch reaches the public
/// pages only as fast as this expires; the CDN's cache policy caps it at 60 s
/// whatever an object carries.
pub const PUBLIC_CACHE_CONTROL: &str = "public, max-age=30";

#[async_trait]
pub(crate) trait Objects: Send + Sync {
    /// The object's bytes; `None` when there is no such key.
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, DomainError>;
    async fn put(
        &self,
        key: &str,
        body: Vec<u8>,
        cache: Option<&'static str>,
    ) -> Result<(), DomainError>;
    /// Removing a key that is not there succeeds.
    async fn delete(&self, key: &str) -> Result<(), DomainError>;
    /// Every key under `prefix`, every page of the listing.
    async fn list(&self, prefix: &str) -> Result<Vec<String>, DomainError>;
}

pub(crate) struct S3Objects {
    client: Client,
    bucket: String,
}

impl S3Objects {
    pub(crate) fn new(client: Client, bucket: &str) -> Self {
        Self {
            client,
            bucket: bucket.to_string(),
        }
    }
}

#[async_trait]
impl Objects for S3Objects {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, DomainError> {
        let output = match self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(o) => o,
            Err(e) if e.as_service_error().is_some_and(|se| se.is_no_such_key()) => {
                return Ok(None);
            }
            Err(e) => return Err(sdk_err("Failed to read a chart bucket object", e)),
        };
        let bytes = output
            .body
            .collect()
            .await
            .map_err(|e| repo_err("Failed to read a chart bucket object body", e))?
            .into_bytes();
        Ok(Some(bytes.to_vec()))
    }

    async fn put(
        &self,
        key: &str,
        body: Vec<u8>,
        cache: Option<&'static str>,
    ) -> Result<(), DomainError> {
        let mut request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(ByteStream::from(body))
            .content_type("application/json");
        if let Some(cache) = cache {
            request = request.cache_control(cache);
        }
        request
            .send()
            .await
            .map_err(|e| sdk_err("Failed to write a chart bucket object", e))?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), DomainError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| sdk_err("Failed to remove a chart bucket object", e))?;
        Ok(())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>, DomainError> {
        let mut keys = Vec::new();
        let mut pages = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(prefix)
            .into_paginator()
            .send();
        while let Some(page) = pages.next().await {
            let page = page.map_err(|e| sdk_err("Failed to list the chart bucket", e))?;
            keys.extend(
                page.contents()
                    .iter()
                    .filter_map(|o| o.key())
                    .map(str::to_string),
            );
        }
        Ok(keys)
    }
}

#[cfg(test)]
pub(crate) mod memory {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    type Stored = (Vec<u8>, Option<&'static str>);

    /// A bucket in a map, shared with the test so it can look inside.
    #[derive(Clone, Default)]
    pub(crate) struct Memory(Arc<Mutex<BTreeMap<String, Stored>>>);

    impl Memory {
        pub(crate) fn keys(&self) -> Vec<String> {
            self.0.lock().unwrap().keys().cloned().collect()
        }

        pub(crate) fn object<T: serde::de::DeserializeOwned>(
            &self,
            key: &str,
        ) -> (T, Option<&'static str>) {
            let (bytes, cache) = self.0.lock().unwrap().get(key).cloned().expect(key);
            (serde_json::from_slice(&bytes).unwrap(), cache)
        }

        pub(crate) fn seed<T: serde::Serialize>(&self, key: &str, value: &T) {
            self.0
                .lock()
                .unwrap()
                .insert(key.to_string(), (serde_json::to_vec(value).unwrap(), None));
        }
    }

    #[async_trait]
    impl Objects for Memory {
        async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, DomainError> {
            Ok(self.0.lock().unwrap().get(key).map(|(b, _)| b.clone()))
        }
        async fn put(
            &self,
            key: &str,
            body: Vec<u8>,
            cache: Option<&'static str>,
        ) -> Result<(), DomainError> {
            self.0
                .lock()
                .unwrap()
                .insert(key.to_string(), (body, cache));
            Ok(())
        }
        async fn delete(&self, key: &str) -> Result<(), DomainError> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
        async fn list(&self, prefix: &str) -> Result<Vec<String>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect())
        }
    }
}
