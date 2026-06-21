use crate::domain::bot::{Bot, BotRepository};
use crate::domain::error::DomainError;
use crate::domain::exchange::Exchange;
use crate::domain::runtime::{
    BotRuntime, BotRuntimeRepository, RuntimePhase, StartClaim, StartLockRepository,
};
use async_trait::async_trait;
use aws_sdk_dynamodb::Client;
use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::update_item::{UpdateItemError, UpdateItemOutput};
use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

/// Collapse a conditional UpdateItem result: `acquired` on success, `contended`
/// when the condition failed (we lost the race, or it is a benign no-op),
/// otherwise a repository error.
fn cas_result<T>(
    res: Result<UpdateItemOutput, SdkError<UpdateItemError>>,
    acquired: T,
    contended: T,
) -> Result<T, DomainError> {
    match res {
        Ok(_) => Ok(acquired),
        Err(e)
            if e.as_service_error()
                .map(|se| se.is_conditional_check_failed_exception())
                .unwrap_or(false) =>
        {
            Ok(contended)
        }
        Err(e) => Err(DomainError::Repository(format!(
            "DynamoDB update_item failed: {e}"
        ))),
    }
}

/// Storage model for the infra layer
pub struct BotItem {
    pub pk: String, // user_id#<user_id>
    pub sk: String, // <bot_id>
    pub name: String,
    pub exchange: String,
    pub api_key: String,
    pub secret_key: String,
    pub enabled: bool,
    pub created_at: i64, // Unix timestamp in seconds
    pub updated_at: i64, // Unix timestamp in seconds
}

impl BotItem {
    /// Extract user_id from PK format: "user_id#<user_id>"
    fn extract_user_id_from_pk(pk: &str) -> Option<String> {
        pk.strip_prefix("user_id#").map(|s| s.to_string())
    }

    /// Construct PK from user_id
    fn construct_pk(user_id: &str) -> String {
        format!("user_id#{}", user_id)
    }

    fn from_item(item: &HashMap<String, AttributeValue>) -> Option<Self> {
        Some(Self {
            pk: item.get("pk")?.as_s().ok()?.to_string(),
            sk: item.get("sk")?.as_s().ok()?.to_string(),
            exchange: item.get("exchange")?.as_s().ok()?.to_string(),
            name: item.get("name")?.as_s().ok()?.to_string(),
            api_key: item.get("api_key")?.as_s().ok()?.to_string(),
            secret_key: item.get("secret_key")?.as_s().ok()?.to_string(),
            enabled: item.get("enabled")?.as_bool().ok().copied()?,
            created_at: item.get("created_at")?.as_n().ok()?.parse().ok()?,
            updated_at: item.get("updated_at")?.as_n().ok()?.parse().ok()?,
        })
    }

    fn to_item(&self) -> HashMap<String, AttributeValue> {
        let mut map = HashMap::new();
        map.insert("pk".to_string(), AttributeValue::S(self.pk.clone()));
        map.insert("sk".to_string(), AttributeValue::S(self.sk.clone()));
        map.insert(
            "exchange".to_string(),
            AttributeValue::S(self.exchange.clone()),
        );
        map.insert("name".to_string(), AttributeValue::S(self.name.clone()));
        map.insert(
            "api_key".to_string(),
            AttributeValue::S(self.api_key.clone()),
        );
        map.insert(
            "secret_key".to_string(),
            AttributeValue::S(self.secret_key.clone()),
        );
        map.insert("enabled".to_string(), AttributeValue::Bool(self.enabled));
        map.insert(
            "created_at".to_string(),
            AttributeValue::N(self.created_at.to_string()),
        );
        map.insert(
            "updated_at".to_string(),
            AttributeValue::N(self.updated_at.to_string()),
        );
        map
    }

    fn to_domain(&self) -> Option<Bot> {
        let user_id = Self::extract_user_id_from_pk(&self.pk)?;
        let exchange = Exchange::from_str(self.exchange.as_str())?;
        Some(Bot {
            id: self.sk.clone(), // bot_id from SK
            user_id,
            exchange,
            name: self.name.clone(),
            api_key: self.api_key.clone(),
            secret_key: self.secret_key.clone(),
            enabled: self.enabled,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }

    fn from_domain(bot: &Bot) -> Self {
        Self {
            pk: Self::construct_pk(&bot.user_id),
            sk: bot.id.clone(),
            exchange: bot.exchange.as_str().to_string(),
            name: bot.name.clone(),
            api_key: bot.api_key.clone(),
            secret_key: bot.secret_key.clone(),
            enabled: bot.enabled,
            created_at: bot.created_at,
            updated_at: bot.updated_at,
        }
    }
}

/// Private storage/mapping struct for the observed ECS runtime of a bot.
/// Item shape: pk = user_id#<user_id>, sk = ecs_task_metadata#<bot_id>,
/// attributes: status (String), task_id (String), task_updated_at (Number),
/// task_current_version (Number).
struct BotECSTaskMetadata {
    pk: String, // user_id#<user_id>
    sk: String, // ecs_task_metadata#<bot_id>
    status: String,
    task_id: String,
    updated_at: i64,
    task_current_version: i64,
}

impl BotECSTaskMetadata {
    fn construct_sk(bot_id: &str) -> String {
        format!("ecs_task_metadata#{}", bot_id)
    }

    fn get_bot_id(&self) -> String {
        self.sk
            .strip_prefix("ecs_task_metadata#")
            .unwrap_or(&self.sk)
            .to_string()
    }

    fn from_item(item: &HashMap<String, AttributeValue>) -> Option<Self> {
        Some(Self {
            pk: item.get("pk")?.as_s().ok()?.to_string(),
            sk: item.get("sk")?.as_s().ok()?.to_string(),
            status: item
                .get("status")
                .and_then(|v| v.as_s().ok())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            task_id: item
                .get("task_id")
                .and_then(|v| v.as_s().ok())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            updated_at: item
                .get("task_updated_at")
                .and_then(|v| v.as_n().ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            task_current_version: item
                .get("task_current_version")
                .and_then(|v| v.as_n().ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
        })
    }

    fn to_item(&self) -> HashMap<String, AttributeValue> {
        let mut map = HashMap::new();
        map.insert("pk".to_string(), AttributeValue::S(self.pk.clone()));
        map.insert("sk".to_string(), AttributeValue::S(self.sk.clone()));
        map.insert("status".to_string(), AttributeValue::S(self.status.clone()));
        map.insert(
            "task_id".to_string(),
            AttributeValue::S(self.task_id.clone()),
        );
        map.insert(
            "task_updated_at".to_string(),
            AttributeValue::N(self.updated_at.to_string()),
        );
        map.insert(
            "task_current_version".to_string(),
            AttributeValue::N(self.task_current_version.to_string()),
        );
        map
    }

    fn from_domain(runtime: &BotRuntime) -> Self {
        Self {
            pk: BotItem::construct_pk(&runtime.user_id),
            sk: Self::construct_sk(&runtime.bot_id),
            status: runtime.phase.as_str().to_string(),
            task_id: runtime.task_id.clone().unwrap_or_default(),
            updated_at: runtime.observed_at,
            task_current_version: runtime.version,
        }
    }

    fn to_domain(&self) -> BotRuntime {
        let user_id = BotItem::extract_user_id_from_pk(&self.pk).unwrap_or_default();
        let bot_id = self.get_bot_id();
        let task_id = if self.task_id.is_empty() {
            None
        } else {
            Some(self.task_id.clone())
        };
        let phase = RuntimePhase::from_str(&self.status).unwrap_or(RuntimePhase::Stopped);
        BotRuntime {
            user_id,
            bot_id,
            task_id,
            phase,
            version: self.task_current_version,
            observed_at: self.updated_at,
        }
    }
}

pub struct DynamoBotRepository {
    client: Client,
    table_name: String,
}

impl DynamoBotRepository {
    pub fn new(client: Client, table_name: String) -> Self {
        Self { client, table_name }
    }
}

#[async_trait]
impl BotRuntimeRepository for DynamoBotRepository {
    async fn find(&self, user_id: &str, bot_id: &str) -> Result<Option<BotRuntime>, DomainError> {
        let result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(BotItem::construct_pk(user_id)))
            .key(
                "sk",
                AttributeValue::S(BotECSTaskMetadata::construct_sk(bot_id)),
            )
            .send()
            .await
            .map_err(|e| DomainError::Repository(format!("DynamoDB get_item failed: {e}")))?;

        match result.item() {
            Some(item) => Ok(BotECSTaskMetadata::from_item(item).map(|m| m.to_domain())),
            None => Ok(None),
        }
    }

    async fn find_consistent(
        &self,
        user_id: &str,
        bot_id: &str,
    ) -> Result<Option<BotRuntime>, DomainError> {
        let result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(BotItem::construct_pk(user_id)))
            .key(
                "sk",
                AttributeValue::S(BotECSTaskMetadata::construct_sk(bot_id)),
            )
            .consistent_read(true)
            .send()
            .await
            .map_err(|e| DomainError::Repository(format!("DynamoDB get_item failed: {e}")))?;

        match result.item() {
            Some(item) => Ok(BotECSTaskMetadata::from_item(item).map(|m| m.to_domain())),
            None => Ok(None),
        }
    }

    async fn record(&self, runtime: &BotRuntime) -> Result<(), DomainError> {
        let metadata = BotECSTaskMetadata::from_domain(runtime);

        // Monotonic, phase-aware write. Both writers (reconcile + record-running)
        // stamp the EventBridge event time in whole seconds, so RUNNING and STOPPED
        // of one bot can land on the same second; events can also arrive out of
        // order or concurrently. Rules, enforced atomically at commit so a
        // non-atomic read-then-write can't lose to a race:
        //   - a strictly newer observation always wins;
        //   - at an equal second a STOPPED wins the tie (terminal state), so a
        //     RUNNING must not overwrite a STOPPED stamped the same second (RUNNING
        //     always precedes STOPPED for a task, so it is the reordered/stale one).
        let put = self
            .client
            .put_item()
            .table_name(&self.table_name)
            .set_item(Some(metadata.to_item()))
            .expression_attribute_values(
                ":observed_at",
                AttributeValue::N(runtime.observed_at.to_string()),
            );

        let put = match runtime.phase {
            RuntimePhase::Stopped => put
                .condition_expression("attribute_not_exists(task_updated_at) OR task_updated_at <= :observed_at"),
            RuntimePhase::Running => put
                .condition_expression("attribute_not_exists(task_updated_at) OR task_updated_at < :observed_at OR (task_updated_at = :observed_at AND #st <> :stopped)")
                .expression_attribute_names("#st", "status")
                .expression_attribute_values(":stopped", AttributeValue::S("stopped".to_string())),
            // The starting lock is normally written via StartLockRepository's
            // conditional CAS, not this observed-write path. Keep the arm total
            // and conservative: a transient `starting` may only win if strictly
            // newer, so it never overwrites a same-second running/stopped.
            RuntimePhase::Starting => put
                .condition_expression("attribute_not_exists(task_updated_at) OR task_updated_at < :observed_at"),
        };

        match put.send().await {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.as_service_error()
                    .map(|se| se.is_conditional_check_failed_exception())
                    .unwrap_or(false)
                {
                    // A newer (or tie-winning) observation already holds the row; dropping this one is correct.
                    tracing::info!(
                        "runtime write skipped (stale observed_at={}, phase={}): pk={}, sk={}",
                        runtime.observed_at,
                        runtime.phase.as_str(),
                        metadata.pk,
                        metadata.sk
                    );
                    Ok(())
                } else {
                    Err(DomainError::Repository(format!(
                        "DynamoDB put_item failed: {e}"
                    )))
                }
            }
        }
    }
}

#[async_trait]
impl StartLockRepository for DynamoBotRepository {
    async fn try_acquire_start(
        &self,
        user_id: &str,
        bot_id: &str,
        now: i64,
        stale_after: i64,
    ) -> Result<StartClaim, DomainError> {
        let pk = BotItem::construct_pk(user_id);
        let sk = BotECSTaskMetadata::construct_sk(bot_id);
        let stale_cutoff = now - stale_after;

        // Strongly-consistent pre-read: lets us report AlreadyRunning /
        // AlreadyStarting precisely and skip a doomed write. It is NOT the safety
        // gate — two callers can both read `stopped` here. The conditional
        // UpdateItem below is the gate.
        let existing = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk.clone()))
            .key("sk", AttributeValue::S(sk.clone()))
            .consistent_read(true)
            .send()
            .await
            .map_err(|e| DomainError::Repository(format!("DynamoDB get_item failed: {e}")))?;

        if let Some(meta) = existing.item().and_then(BotECSTaskMetadata::from_item) {
            match meta.status.as_str() {
                "running" => return Ok(StartClaim::AlreadyRunning),
                "starting" if meta.updated_at > stale_cutoff => {
                    return Ok(StartClaim::AlreadyStarting);
                }
                _ => {} // stopped, missing, or a stale starting lock -> attempt to claim
            }
        }

        // Authoritative compare-and-set. DynamoDB serializes conditional writes
        // per item, so concurrent claimers cannot both satisfy the condition.
        // Clearing task_id marks "launch in flight, id not yet known".
        let res = self.client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S(sk))
            .update_expression("SET #st = :starting, task_updated_at = :now, task_id = :empty")
            .condition_expression("attribute_not_exists(#st) OR #st = :stopped OR (#st = :starting AND task_updated_at <= :stale_cutoff)")
            .expression_attribute_names("#st", "status")
            .expression_attribute_values(":starting", AttributeValue::S("starting".to_string()))
            .expression_attribute_values(":stopped", AttributeValue::S("stopped".to_string()))
            .expression_attribute_values(":now", AttributeValue::N(now.to_string()))
            .expression_attribute_values(":empty", AttributeValue::S(String::new()))
            .expression_attribute_values(":stale_cutoff", AttributeValue::N(stale_cutoff.to_string()))
            .send()
            .await;

        // A conditional failure means the row went running, or another fresh lock
        // was claimed, between the pre-read and the CAS — skipping is correct.
        cas_result(res, StartClaim::Acquired, StartClaim::AlreadyStarting)
    }

    async fn try_acquire_restart(
        &self,
        user_id: &str,
        bot_id: &str,
        stopped_task_id: &str,
        now: i64,
    ) -> Result<StartClaim, DomainError> {
        // Authoritative CAS, no pre-read: claim only while the stopped task is
        // still the row's current task, bumping the restart counter. Concurrent or
        // duplicate STOPPED events serialize per item — the first clears task_id,
        // so the rest fail the condition. A conditional failure means the stopped
        // task is no longer current (already replaced/stopped); the caller treats
        // any non-`Acquired` outcome as "nothing to restart".
        let res = self.client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(BotItem::construct_pk(user_id)))
            .key("sk", AttributeValue::S(BotECSTaskMetadata::construct_sk(bot_id)))
            .update_expression("SET #st = :starting, task_updated_at = :now, task_id = :empty, task_current_version = if_not_exists(task_current_version, :zero) + :one")
            .condition_expression("task_id = :stopped AND (#st = :running OR #st = :starting)")
            .expression_attribute_names("#st", "status")
            .expression_attribute_values(":starting", AttributeValue::S("starting".to_string()))
            .expression_attribute_values(":running", AttributeValue::S("running".to_string()))
            .expression_attribute_values(":stopped", AttributeValue::S(stopped_task_id.to_string()))
            .expression_attribute_values(":now", AttributeValue::N(now.to_string()))
            .expression_attribute_values(":empty", AttributeValue::S(String::new()))
            .expression_attribute_values(":zero", AttributeValue::N("0".to_string()))
            .expression_attribute_values(":one", AttributeValue::N("1".to_string()))
            .send()
            .await;

        cas_result(res, StartClaim::Acquired, StartClaim::AlreadyStarting)
    }

    async fn attach_started_task(
        &self,
        user_id: &str,
        bot_id: &str,
        task_id: &str,
    ) -> Result<(), DomainError> {
        // Only the task_id is set; the claim timestamp is left intact so the
        // stale-lock clock keeps running from when the launch began. Conditional
        // on the row still being `starting` so a RUNNING/STOPPED observation that
        // already won the row is never reverted.
        let res = self
            .client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(BotItem::construct_pk(user_id)))
            .key(
                "sk",
                AttributeValue::S(BotECSTaskMetadata::construct_sk(bot_id)),
            )
            .update_expression("SET task_id = :tid")
            .condition_expression("#st = :starting")
            .expression_attribute_names("#st", "status")
            .expression_attribute_values(":starting", AttributeValue::S("starting".to_string()))
            .expression_attribute_values(":tid", AttributeValue::S(task_id.to_string()))
            .send()
            .await;

        cas_result(res, (), ())
    }

    async fn release_start(
        &self,
        user_id: &str,
        bot_id: &str,
        now: i64,
    ) -> Result<(), DomainError> {
        // Roll a held lock back to stopped after a failed launch. Conditional on
        // the row still being `starting` so a concurrent real observation wins.
        let res = self
            .client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(BotItem::construct_pk(user_id)))
            .key(
                "sk",
                AttributeValue::S(BotECSTaskMetadata::construct_sk(bot_id)),
            )
            .update_expression("SET #st = :stopped, task_updated_at = :now, task_id = :empty")
            .condition_expression("#st = :starting")
            .expression_attribute_names("#st", "status")
            .expression_attribute_values(":stopped", AttributeValue::S("stopped".to_string()))
            .expression_attribute_values(":starting", AttributeValue::S("starting".to_string()))
            .expression_attribute_values(":now", AttributeValue::N(now.to_string()))
            .expression_attribute_values(":empty", AttributeValue::S(String::new()))
            .send()
            .await;

        cas_result(res, (), ())
    }
}

#[async_trait]
impl BotRepository for DynamoBotRepository {
    async fn find(&self, user_id: &str, bot_id: &str) -> Option<Bot> {
        // Note: We need user_id to construct PK, so this method uses scan (not efficient)
        // In production, consider adding bot_id as GSI or passing user_id as well
        let result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(BotItem::construct_pk(user_id)))
            .key("sk", AttributeValue::S(bot_id.to_string()))
            .send()
            .await
            .ok()?;

        let item = result.item()?;
        let bot_item = BotItem::from_item(item)?;
        bot_item.to_domain()
    }

    async fn find_consistent(&self, user_id: &str, bot_id: &str) -> Option<Bot> {
        let result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(BotItem::construct_pk(user_id)))
            .key("sk", AttributeValue::S(bot_id.to_string()))
            .consistent_read(true)
            .send()
            .await
            .ok()?;

        let item = result.item()?;
        let bot_item = BotItem::from_item(item)?;
        bot_item.to_domain()
    }

    async fn save(&self, bot: &Bot) -> Result<(), DomainError> {
        let bot_item = BotItem::from_domain(bot);
        let item = bot_item.to_item();

        self.client
            .put_item()
            .table_name(&self.table_name)
            .set_item(Some(item))
            .send()
            .await
            .map_err(|e| DomainError::Repository(format!("DynamoDB put_item failed: {e}")))?;

        Ok(())
    }

    async fn find_by_user_id(&self, user_id: &str) -> Vec<Bot> {
        let pk_value = BotItem::construct_pk(user_id);

        let result = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("pk = :pk")
            .expression_attribute_values(":pk", AttributeValue::S(pk_value))
            .send()
            .await;

        match result {
            Ok(output) => output
                .items()
                .iter()
                .filter_map(|item| BotItem::from_item(item))
                .filter_map(|bot_item| bot_item.to_domain())
                .collect(),
            Err(e) => {
                tracing::error!("DynamoDB query error: {:?}", e);
                Vec::new()
            }
        }
    }

    async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), String> {
        let pk_value = BotItem::construct_pk(user_id);

        self.client
            .delete_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk_value))
            .key("sk", AttributeValue::S(bot_id.to_string()))
            .send()
            .await
            .map_err(|e| format!("Failed to delete bot: {:?}", e))?;

        Ok(())
    }
}
