//! The chart bucket's showcase halves, shared by the collector and the API so
//! the two cannot disagree on a key or on how the listing is built:
//!
//! - `<showcase prefix>/bots/{pid}.json`: the public artifact of every bot on
//!   the operator's account, shown or not, written by each collector run.
//!   Private: nothing outside the collector and the API reads it.
//! - `<public prefix>/bots/{pid}.json` and `<public prefix>/index.json`: what
//!   the public pages read. Written by the collector's run and by the
//!   showcase switch.

use std::sync::Arc;

use async_trait::async_trait;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;

use crate::config::chart::ChartConfig;
use crate::domain::bot::Bot;
use crate::domain::clock::Clock;
use crate::domain::error::DomainError;
use crate::domain::showcase::{
    PublicBotSeries, PublicIndex, Published, ShowcasePublisher, public_id,
};
use crate::infra::aws_error::{repo_err, sdk_err};

/// How long a cache may reuse a public object. Hiding a bot takes effect only
/// as fast as this expires; the CDN's cache policy caps it at 60 s whatever
/// an object carries.
pub const PUBLIC_CACHE_CONTROL: &str = "public, max-age=30";

fn bot_key(prefix: &str, pid: &str) -> String {
    format!("{prefix}/bots/{pid}.json")
}

fn index_key(prefix: &str) -> String {
    format!("{prefix}/index.json")
}

/// The public id a listed key names, when it is a bot artifact under `prefix`.
fn pid_of(prefix: &str, key: &str) -> Option<String> {
    key.strip_prefix(&format!("{prefix}/bots/"))?
        .strip_suffix(".json")
        .map(str::to_string)
}

/// The object operations the showcase layout is built on. A seam of its own so
/// the publish, the withdraw and the listing rebuild run in tests exactly as
/// they run against the bucket.
#[async_trait]
trait Objects: Send + Sync {
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

struct S3Objects {
    client: Client,
    bucket: String,
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
            Err(e) => return Err(sdk_err("Failed to read a showcase artifact", e)),
        };
        let bytes = output
            .body
            .collect()
            .await
            .map_err(|e| repo_err("Failed to read a showcase artifact body", e))?
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
            .map_err(|e| sdk_err("Failed to write a showcase artifact", e))?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), DomainError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| sdk_err("Failed to remove a showcase artifact", e))?;
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
            let page = page.map_err(|e| sdk_err("Failed to list the public showcase", e))?;
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

pub struct S3ShowcaseStore {
    objects: Box<dyn Objects>,
    public_prefix: String,
    showcase_prefix: String,
    clock: Arc<dyn Clock>,
}

impl S3ShowcaseStore {
    pub fn new(client: Client, chart: &ChartConfig, clock: Arc<dyn Clock>) -> Self {
        let objects = S3Objects {
            client,
            bucket: chart.bucket_name.clone(),
        };
        Self::over(
            Box::new(objects),
            &chart.public_prefix,
            &chart.showcase_prefix,
            clock,
        )
    }

    fn over(
        objects: Box<dyn Objects>,
        public_prefix: &str,
        showcase_prefix: &str,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            objects,
            public_prefix: public_prefix.trim_matches('/').to_string(),
            showcase_prefix: showcase_prefix.trim_matches('/').to_string(),
            clock,
        }
    }

    async fn get_series(&self, key: &str) -> Result<Option<PublicBotSeries>, DomainError> {
        let Some(bytes) = self.objects.get(key).await? else {
            return Ok(None);
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| repo_err("Failed to parse a showcase artifact", e))
    }

    fn encode(series: &PublicBotSeries) -> Result<Vec<u8>, DomainError> {
        serde_json::to_vec(series)
            .map_err(|e| repo_err("Failed to serialize a showcase artifact", e))
    }

    /// Keep a bot's artifact where the showcase switch can publish it from.
    pub async fn put_shadow(&self, series: &PublicBotSeries) -> Result<(), DomainError> {
        let key = bot_key(&self.showcase_prefix, &series.id);
        self.objects.put(&key, Self::encode(series)?, None).await
    }

    /// Put a bot's artifact on the public prefix.
    pub async fn put_public(&self, series: &PublicBotSeries) -> Result<(), DomainError> {
        let key = bot_key(&self.public_prefix, &series.id);
        self.objects
            .put(&key, Self::encode(series)?, Some(PUBLIC_CACHE_CONTROL))
            .await
    }

    /// Take a bot's artifact off the public prefix. Removing one that is not
    /// there succeeds.
    pub async fn delete_public(&self, pid: &str) -> Result<(), DomainError> {
        self.objects
            .delete(&bot_key(&self.public_prefix, pid))
            .await
    }

    /// The public ids with an artifact on the public prefix.
    pub async fn list_public(&self) -> Result<Vec<String>, DomainError> {
        let prefix = format!("{}/bots/", self.public_prefix);
        Ok(self
            .objects
            .list(&prefix)
            .await?
            .iter()
            .filter_map(|key| pid_of(&self.public_prefix, key))
            .collect())
    }

    /// Rebuild the listing from the artifacts on the public prefix, so it lists
    /// exactly what is published whoever wrote last. Returns how many it lists.
    pub async fn rebuild_index(&self) -> Result<usize, DomainError> {
        let mut series = Vec::new();
        for pid in self.list_public().await? {
            // An artifact removed between the listing and the read is simply
            // not listed.
            if let Some(s) = self.get_series(&bot_key(&self.public_prefix, &pid)).await? {
                series.push(s);
            }
        }
        let index = PublicIndex::new(&series, self.clock.now());
        let body = serde_json::to_vec(&index)
            .map_err(|e| repo_err("Failed to serialize the showcase index", e))?;
        self.objects
            .put(
                &index_key(&self.public_prefix),
                body,
                Some(PUBLIC_CACHE_CONTROL),
            )
            .await?;
        Ok(series.len())
    }
}

#[async_trait]
impl ShowcasePublisher for S3ShowcaseStore {
    async fn publish(&self, bot: &Bot) -> Result<Published, DomainError> {
        let pid = public_id(&bot.user_id, &bot.id);
        let Some(mut series) = self
            .get_series(&bot_key(&self.showcase_prefix, &pid))
            .await?
        else {
            return Ok(Published::NoCurveYet);
        };
        series.refresh_from(bot);
        self.put_public(&series).await?;
        self.rebuild_index().await?;
        Ok(Published::Live)
    }

    async fn withdraw(&self, bot: &Bot) -> Result<(), DomainError> {
        self.delete_public(&public_id(&bot.user_id, &bot.id))
            .await?;
        self.rebuild_index().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::showcase::PublicPoint;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    const NOW: i64 = 1_700_000_000;
    const LINK: &str = "https://www.bybit.com/copyTrade/x";

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> i64 {
            NOW
        }
    }

    type Stored = (Vec<u8>, Option<&'static str>);

    /// A bucket in a map, shared with the test so it can look inside.
    #[derive(Clone, Default)]
    struct Memory(Arc<Mutex<BTreeMap<String, Stored>>>);

    impl Memory {
        fn keys(&self) -> Vec<String> {
            self.0.lock().unwrap().keys().cloned().collect()
        }

        fn object<T: serde::de::DeserializeOwned>(&self, key: &str) -> (T, Option<&'static str>) {
            let (bytes, cache) = self.0.lock().unwrap().get(key).cloned().expect(key);
            (serde_json::from_slice(&bytes).unwrap(), cache)
        }

        fn seed(&self, key: &str, series: &PublicBotSeries) {
            self.0
                .lock()
                .unwrap()
                .insert(key.to_string(), (serde_json::to_vec(series).unwrap(), None));
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

    fn store(memory: &Memory) -> S3ShowcaseStore {
        S3ShowcaseStore::over(
            Box::new(memory.clone()),
            "/public/",
            "showcase",
            Arc::new(FixedClock),
        )
    }

    fn a_bot(name: &str) -> Bot {
        let mut bot = Bot::create("u-1".into(), name.into(), "ak".into(), "sk".into(), 1);
        bot.set_showcase(true, 2);
        bot
    }

    fn artifact(bot: &Bot, name: &str) -> PublicBotSeries {
        PublicBotSeries {
            id: public_id(&bot.user_id, &bot.id),
            name: name.into(),
            exchange: "bybit".into(),
            public_url: Some(LINK.into()),
            generated_at: 0,
            current_return_pct: 2.0,
            points: vec![PublicPoint {
                ts: 0,
                index: 102.0,
                return_pct: 2.0,
            }],
            config_switches: vec![],
            capital_resets: vec![],
        }
    }

    #[test]
    fn a_bot_artifact_sits_under_its_prefix_by_public_id() {
        assert_eq!(
            bot_key("public", "0a1b2c3d4e5f"),
            "public/bots/0a1b2c3d4e5f.json"
        );
        assert_eq!(
            bot_key("showcase", "0a1b2c3d4e5f"),
            "showcase/bots/0a1b2c3d4e5f.json"
        );
        assert_eq!(index_key("public"), "public/index.json");
    }

    #[test]
    fn a_listed_key_names_its_public_id_and_nothing_else_does() {
        assert_eq!(
            pid_of("public", "public/bots/0a1b2c3d4e5f.json").as_deref(),
            Some("0a1b2c3d4e5f")
        );
        assert_eq!(pid_of("public", "public/index.json"), None);
        assert_eq!(pid_of("public", "showcase/bots/0a1b2c3d4e5f.json"), None);
        assert_eq!(pid_of("public", "public/bots/readme.txt"), None);
    }

    #[tokio::test]
    async fn publishing_writes_the_rows_name_and_link_over_the_copy_and_rebuilds_the_listing() {
        let memory = Memory::default();
        let mut bot = a_bot("renamed");
        let pid = public_id(&bot.user_id, &bot.id);
        memory.seed(
            &format!("showcase/bots/{pid}.json"),
            &artifact(&bot, "old name"),
        );
        bot.set_public_url(None, 3).unwrap();

        let published = store(&memory).publish(&bot).await.unwrap();

        assert_eq!(published, Published::Live);
        let (public, cache): (PublicBotSeries, _) =
            memory.object(&format!("public/bots/{pid}.json"));
        assert_eq!(public.name, "renamed", "the row's name, not the copy's");
        assert_eq!(public.public_url, None, "a cleared link is not republished");
        assert_eq!(public.points, artifact(&bot, "old name").points);
        assert_eq!(cache, Some(PUBLIC_CACHE_CONTROL));
        let (index, cache): (PublicIndex, _) = memory.object("public/index.json");
        assert_eq!(index.generated_at, NOW);
        assert_eq!(
            index.bots.iter().map(|b| b.id.clone()).collect::<Vec<_>>(),
            vec![pid.clone()]
        );
        assert_eq!(index.bots[0].name, "renamed");
        assert_eq!(cache, Some(PUBLIC_CACHE_CONTROL));
        let (copy, _): (PublicBotSeries, _) = memory.object(&format!("showcase/bots/{pid}.json"));
        assert_eq!(
            copy.name, "old name",
            "the private copy is left as the run wrote it"
        );
    }

    #[tokio::test]
    async fn withdrawing_removes_the_public_file_and_its_listing_entry() {
        let memory = Memory::default();
        let (gone, stays) = (a_bot("gone"), a_bot("stays"));
        let (gone_pid, stays_pid) = (
            public_id(&gone.user_id, &gone.id),
            public_id(&stays.user_id, &stays.id),
        );
        memory.seed(
            &format!("public/bots/{gone_pid}.json"),
            &artifact(&gone, "gone"),
        );
        memory.seed(
            &format!("public/bots/{stays_pid}.json"),
            &artifact(&stays, "stays"),
        );

        store(&memory).withdraw(&gone).await.unwrap();

        assert!(
            !memory
                .keys()
                .contains(&format!("public/bots/{gone_pid}.json"))
        );
        let (index, _): (PublicIndex, _) = memory.object("public/index.json");
        assert_eq!(
            index.bots.iter().map(|b| b.id.clone()).collect::<Vec<_>>(),
            vec![stays_pid]
        );
    }

    #[tokio::test]
    async fn a_bot_without_a_private_copy_is_not_published_and_nothing_is_written() {
        let memory = Memory::default();

        let published = store(&memory).publish(&a_bot("new")).await.unwrap();

        assert_eq!(published, Published::NoCurveYet);
        assert!(memory.keys().is_empty(), "{:?}", memory.keys());
    }

    #[tokio::test]
    async fn the_listing_is_written_empty_when_nothing_is_public() {
        let memory = Memory::default();
        memory.seed(
            "showcase/bots/0a1b2c3d4e5f.json",
            &artifact(&a_bot("hidden"), "hidden"),
        );

        let listed = store(&memory).rebuild_index().await.unwrap();

        assert_eq!(listed, 0);
        let (index, _): (PublicIndex, _) = memory.object("public/index.json");
        assert!(index.bots.is_empty(), "a private copy is never listed");
    }
}
