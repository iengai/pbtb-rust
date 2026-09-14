//! In-memory stand-ins for the ports the end-to-end harness does not run for
//! real: object storage (templates, bot configs, API keys, return series) and
//! ECS.
//!
//! DynamoDB is deliberately NOT faked — the start lock's conditional writes are
//! the one thing a mock cannot validate, so the harness runs them against
//! DynamoDB Local (see `super::dynamo`).

use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use pbtb_rust::domain::bot::{ApiKeyRepository, Bot};
use pbtb_rust::domain::botconfig::{BotConfig, BotConfigRepository};
use pbtb_rust::domain::clock::Clock;
use pbtb_rust::domain::configtemplate::{ConfigTemplate, ConfigTemplateRepository};
use pbtb_rust::domain::error::{DomainError, Retryability};
use pbtb_rust::domain::returncurve::ReturnCurveRepository;
use pbtb_rust::domain::showcase::{Published, ShowcasePublisher};
use pbtb_rust::domain::templateaudience::{PublishedTemplates, TemplateAudiencePublisher};
use pbtb_rust::usecase::{TaskController, TaskLiveness, TaskRunner};

/// A clock frozen at a known instant, so timestamps in assertions are exact
/// rather than "roughly now".
pub struct FixedClock(pub i64);

impl Clock for FixedClock {
    fn now(&self) -> i64 {
        self.0
    }
}

#[derive(Default)]
pub struct InMemoryApiKeys {
    saved: Mutex<HashMap<String, (String, String)>>,
}

impl InMemoryApiKeys {
    /// The key pair stored for a bot, if any. Exists so a test can assert that
    /// secrets took the storage path and never the reply path.
    pub fn get(&self, user_id: &str, bot_id: &str) -> Option<(String, String)> {
        self.saved
            .lock()
            .unwrap()
            .get(&format!("{user_id}/{bot_id}"))
            .cloned()
    }
}

#[async_trait]
impl ApiKeyRepository for InMemoryApiKeys {
    async fn save(&self, bot: &Bot) -> Result<(), DomainError> {
        self.saved.lock().unwrap().insert(
            format!("{}/{}", bot.user_id, bot.id),
            (bot.api_key.clone(), bot.secret_key.clone()),
        );
        Ok(())
    }

    async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), DomainError> {
        self.saved
            .lock()
            .unwrap()
            .remove(&format!("{user_id}/{bot_id}"));
        Ok(())
    }
}

#[derive(Default)]
pub struct InMemoryTemplates {
    templates: Mutex<HashMap<String, ConfigTemplate>>,
}

impl InMemoryTemplates {
    pub fn add(&self, template: ConfigTemplate) {
        self.templates
            .lock()
            .unwrap()
            .insert(template.name.clone(), template);
    }
}

#[async_trait]
impl ConfigTemplateRepository for InMemoryTemplates {
    async fn get(&self, template_name: &str) -> Result<ConfigTemplate, DomainError> {
        self.templates
            .lock()
            .unwrap()
            .get(template_name)
            .cloned()
            .ok_or_else(|| DomainError::InvalidConfig(format!("no such template: {template_name}")))
    }

    async fn list(&self) -> Result<Vec<String>, DomainError> {
        let mut names: Vec<String> = self.templates.lock().unwrap().keys().cloned().collect();
        names.sort();
        Ok(names)
    }

    async fn exists(&self, template_name: &str) -> Result<bool, DomainError> {
        Ok(self.templates.lock().unwrap().contains_key(template_name))
    }

    async fn save(&self, template: &ConfigTemplate) -> Result<(), DomainError> {
        self.add(template.clone());
        Ok(())
    }
}

/// The collector's per-bot series, keyed by tenant and bot as the bucket is.
#[derive(Default)]
pub struct InMemoryReturnCurves {
    series: Mutex<HashMap<(String, String), serde_json::Value>>,
}

impl InMemoryReturnCurves {
    pub fn put(&self, user_id: &str, bot_id: &str, series: serde_json::Value) {
        self.series
            .lock()
            .unwrap()
            .insert((user_id.to_string(), bot_id.to_string()), series);
    }
}

#[async_trait]
impl ReturnCurveRepository for InMemoryReturnCurves {
    async fn get(
        &self,
        user_id: &str,
        bot_id: &str,
    ) -> Result<Option<serde_json::Value>, DomainError> {
        Ok(self
            .series
            .lock()
            .unwrap()
            .get(&(user_id.to_string(), bot_id.to_string()))
            .cloned())
    }
}

#[derive(Default)]
pub struct InMemoryBotConfigs {
    configs: Mutex<HashMap<String, BotConfig>>,
}

impl InMemoryBotConfigs {
    fn key(user_id: &str, bot_id: &str) -> String {
        format!("{user_id}/{bot_id}")
    }

    pub fn put(&self, config: BotConfig) {
        self.configs
            .lock()
            .unwrap()
            .insert(Self::key(&config.user_id, &config.bot_id), config);
    }

    pub fn get_saved(&self, user_id: &str, bot_id: &str) -> Option<BotConfig> {
        self.configs
            .lock()
            .unwrap()
            .get(&Self::key(user_id, bot_id))
            .cloned()
    }
}

#[async_trait]
impl BotConfigRepository for InMemoryBotConfigs {
    async fn get(&self, user_id: &str, bot_id: &str) -> Result<BotConfig, DomainError> {
        self.configs
            .lock()
            .unwrap()
            .get(&Self::key(user_id, bot_id))
            .cloned()
            .ok_or_else(|| DomainError::InvalidConfig(format!("no config for {bot_id}")))
    }

    async fn save(&self, config: &BotConfig) -> Result<(), DomainError> {
        self.put(config.clone());
        Ok(())
    }

    async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), DomainError> {
        self.configs
            .lock()
            .unwrap()
            .remove(&Self::key(user_id, bot_id));
        Ok(())
    }

    async fn exists(&self, user_id: &str, bot_id: &str) -> Result<bool, DomainError> {
        Ok(self
            .configs
            .lock()
            .unwrap()
            .contains_key(&Self::key(user_id, bot_id)))
    }
}

/// One RunTask call, as the launch path issued it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchedTask {
    pub user_id: String,
    pub bot_id: String,
    pub cluster_arn: String,
    pub td_arn: String,
    pub container_name: String,
}

/// Records launches instead of calling ECS, and hands back a task id.
///
/// The count is the whole point of several tests: a bot that gets two live
/// tasks is the one failure this system must never have, so "how many times was
/// RunTask called" is an assertion, not a detail.
#[derive(Default)]
pub struct RecordingEcs {
    launches: Mutex<Vec<LaunchedTask>>,
    /// `(cluster_arn, task_id, reason)` per StopTask; the reason is what tells
    /// a restart's stop from a user's on the STOPPED event.
    stops: Mutex<Vec<(String, String, String)>>,
    liveness: Mutex<HashMap<String, TaskLiveness>>,
}

impl RecordingEcs {
    pub fn launches(&self) -> Vec<LaunchedTask> {
        self.launches.lock().unwrap().clone()
    }

    pub fn stops(&self) -> Vec<(String, String, String)> {
        self.stops.lock().unwrap().clone()
    }

    /// Declare a task gone, so a stale-lock reclaim can be exercised.
    pub fn set_gone(&self, task_id: &str) {
        self.liveness
            .lock()
            .unwrap()
            .insert(task_id.to_string(), TaskLiveness::Gone);
    }
}

#[async_trait]
impl TaskRunner for RecordingEcs {
    async fn run(
        &self,
        user_id: &str,
        bot_id: &str,
        cluster_arn: &str,
        td_arn: &str,
        container_name: &str,
    ) -> anyhow::Result<String> {
        let mut launches = self.launches.lock().unwrap();
        launches.push(LaunchedTask {
            user_id: user_id.to_string(),
            bot_id: bot_id.to_string(),
            cluster_arn: cluster_arn.to_string(),
            td_arn: td_arn.to_string(),
            container_name: container_name.to_string(),
        });
        Ok(format!("task-{}", launches.len()))
    }
}

#[async_trait]
impl TaskController for RecordingEcs {
    async fn stop(&self, cluster_arn: &str, task_id: &str, reason: &str) -> anyhow::Result<()> {
        self.stops.lock().unwrap().push((
            cluster_arn.to_string(),
            task_id.to_string(),
            reason.to_string(),
        ));
        Ok(())
    }

    async fn liveness(&self, _cluster_arn: &str, task_id: &str) -> anyhow::Result<TaskLiveness> {
        Ok(self
            .liveness
            .lock()
            .unwrap()
            .get(task_id)
            .cloned()
            .unwrap_or(TaskLiveness::Alive))
    }
}

/// A launch that always fails, for asserting that the start lock is released
/// rather than left held when ECS refuses.
pub struct FailingEcs;

#[async_trait]
impl TaskRunner for FailingEcs {
    async fn run(
        &self,
        _user_id: &str,
        _bot_id: &str,
        _cluster_arn: &str,
        _td_arn: &str,
        _container_name: &str,
    ) -> anyhow::Result<String> {
        Err(anyhow::anyhow!("RunTask refused"))
    }
}

pub type SharedEcs = Arc<RecordingEcs>;

/// The showcase publisher, recording what it was asked to do. `add_curve`
/// marks a bot as having a collected curve; `fail_with` makes every later
/// call fail with that retryability.
#[derive(Default)]
pub struct InMemoryShowcasePublisher {
    curves: Mutex<HashSet<(String, String)>>,
    calls: Mutex<Vec<String>>,
    fault: Mutex<Option<Retryability>>,
}

impl InMemoryShowcasePublisher {
    pub fn add_curve(&self, user_id: &str, bot_id: &str) {
        self.curves
            .lock()
            .unwrap()
            .insert((user_id.to_string(), bot_id.to_string()));
    }

    pub fn fail_with(&self, retry: Retryability) {
        *self.fault.lock().unwrap() = Some(retry);
    }

    /// Every call so far, as `publish <bot_id>` / `withdraw <bot_id>`.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, call: &str, bot: &Bot) -> Result<(), DomainError> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{call} {}", bot.id));
        match *self.fault.lock().unwrap() {
            Some(retry) => Err(DomainError::repository_with(
                "writing the public showcase",
                retry,
                std::io::Error::other("the fake is failing"),
            )),
            None => Ok(()),
        }
    }
}

#[async_trait]
impl ShowcasePublisher for InMemoryShowcasePublisher {
    async fn publish(&self, bot: &Bot) -> Result<Published, DomainError> {
        self.record("publish", bot)?;
        let key = (bot.user_id.clone(), bot.id.clone());
        Ok(if self.curves.lock().unwrap().contains(&key) {
            Published::Live
        } else {
            Published::NoCurveYet
        })
    }

    async fn withdraw(&self, bot: &Bot) -> Result<(), DomainError> {
        self.record("withdraw", bot)
    }
}

/// The template audience publisher, recording every set it was given;
/// `fail_with` makes every later call fail with that retryability.
#[derive(Default)]
pub struct InMemoryTemplateAudiencePublisher {
    published: Mutex<Vec<PublishedTemplates>>,
    fault: Mutex<Option<Retryability>>,
}

impl InMemoryTemplateAudiencePublisher {
    pub fn fail_with(&self, retry: Retryability) {
        *self.fault.lock().unwrap() = Some(retry);
    }

    /// Every set published so far, oldest first.
    pub fn published(&self) -> Vec<PublishedTemplates> {
        self.published.lock().unwrap().clone()
    }
}

#[async_trait]
impl TemplateAudiencePublisher for InMemoryTemplateAudiencePublisher {
    async fn publish(&self, published: &PublishedTemplates) -> Result<(), DomainError> {
        self.published.lock().unwrap().push(published.clone());
        match *self.fault.lock().unwrap() {
            Some(retry) => Err(DomainError::repository_with(
                "writing the template audiences",
                retry,
                std::io::Error::other("the fake is failing"),
            )),
            None => Ok(()),
        }
    }
}
