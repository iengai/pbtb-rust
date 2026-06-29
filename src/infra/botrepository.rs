use crate::domain::bot::{Bot, BotRepository};
use crate::domain::configswitch::{ConfigSwitchEvent, ConfigSwitchKind, ConfigSwitchRepository};
use crate::domain::error::DomainError;
use crate::domain::exchange::Exchange;
use crate::domain::runtime::{
    BotRuntime, BotRuntimeRepository, RuntimePhase, StartClaim, StartLockRepository,
};
use crate::infra::aws_error::repo_err;
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
        Err(e) => Err(repo_err("DynamoDB update_item failed", e)),
    }
}

/// Parse a present DynamoDB item into a domain `Bot`. A row that fails to map
/// (unknown exchange, unparseable timestamp, …) is a `CorruptRecord` fault, not
/// an absence: collapsing it into `None` would let a corrupt live bot read back
/// as "not found" (see docs/conventions.md § Error Handling). The message names
/// only the keys — never the api_key/secret_key the row carries.
fn parse_bot_row(
    item: &HashMap<String, AttributeValue>,
    user_id: &str,
) -> Result<Bot, DomainError> {
    BotItem::from_item(item)
        .and_then(|bot_item| bot_item.to_domain())
        .ok_or_else(|| {
            DomainError::CorruptRecord(format!("bot row pk=user_id#{user_id} did not parse"))
        })
}

/// A bot's partition (`pk = user_id#<user_id>`) also holds its observed-runtime /
/// start-lock row (`sk = ecs_task_metadata#<bot_id>`). That row is not a bot, so a
/// `find_by_user_id` query must skip it rather than try to parse it as one (which
/// would mis-read an expected non-bot row as a `CorruptRecord` and fail the whole
/// list). A genuinely corrupt *bot* row still fails loud — it is not skipped here.
fn is_runtime_row(item: &HashMap<String, AttributeValue>) -> bool {
    item.get("sk")
        .and_then(|v| v.as_s().ok())
        .is_some_and(|sk| sk.starts_with(ECS_TASK_METADATA_SK_PREFIX))
}

/// A bot's partition also holds its config-switch timeline rows
/// (`sk = config_switch#<bot_id>#<applied_at>`). Like the runtime row they are
/// not bots, so a `find_by_user_id` query must skip them rather than mis-parse
/// one as a `CorruptRecord` and fail the whole list.
fn is_config_switch_row(item: &HashMap<String, AttributeValue>) -> bool {
    item.get("sk")
        .and_then(|v| v.as_s().ok())
        .is_some_and(|sk| sk.starts_with(CONFIG_SWITCH_SK_PREFIX))
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
        format!("user_id#{user_id}")
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

/// Sort-key prefix that marks a row as a bot's observed ECS runtime / start-lock
/// row rather than the bot row itself. Both live under the same `pk`.
const ECS_TASK_METADATA_SK_PREFIX: &str = "ecs_task_metadata#";

/// Sort-key prefix that marks a row as a config-switch timeline event for a bot
/// (`sk = config_switch#<bot_id>#<applied_at>`), sharing the bot's `pk`.
const CONFIG_SWITCH_SK_PREFIX: &str = "config_switch#";

impl BotECSTaskMetadata {
    fn construct_sk(bot_id: &str) -> String {
        format!("{ECS_TASK_METADATA_SK_PREFIX}{bot_id}")
    }

    fn get_bot_id(&self) -> String {
        self.sk
            .strip_prefix(ECS_TASK_METADATA_SK_PREFIX)
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

/// Storage/mapping struct for a bot's config-switch timeline event.
/// Item shape: pk = user_id#<user_id>, sk = config_switch#<bot_id>#<applied_at>,
/// attributes: bot_id (String), kind (String), template_name (String),
/// template_version (String, only when present), applied_at (Number, unix
/// seconds). `applied_at` is fixed-width in the sort key, so the natural key
/// order is chronological.
struct ConfigSwitchItem {
    pk: String, // user_id#<user_id>
    sk: String, // config_switch#<bot_id>#<applied_at>
    bot_id: String,
    kind: String,
    template_name: String,
    template_version: Option<String>,
    applied_at: i64,
}

impl ConfigSwitchItem {
    fn construct_sk(bot_id: &str, applied_at: i64) -> String {
        format!("{CONFIG_SWITCH_SK_PREFIX}{bot_id}#{applied_at}")
    }

    /// Sort-key prefix selecting every switch event of one bot, for a
    /// `begins_with` range query.
    fn sk_prefix_for_bot(bot_id: &str) -> String {
        format!("{CONFIG_SWITCH_SK_PREFIX}{bot_id}#")
    }

    fn from_domain(event: &ConfigSwitchEvent) -> Self {
        Self {
            pk: BotItem::construct_pk(&event.user_id),
            sk: Self::construct_sk(&event.bot_id, event.applied_at),
            bot_id: event.bot_id.clone(),
            kind: event.kind.as_str().to_string(),
            template_name: event.template_name.clone(),
            template_version: event.template_version.clone(),
            applied_at: event.applied_at,
        }
    }

    fn to_item(&self) -> HashMap<String, AttributeValue> {
        let mut map = HashMap::new();
        map.insert("pk".to_string(), AttributeValue::S(self.pk.clone()));
        map.insert("sk".to_string(), AttributeValue::S(self.sk.clone()));
        map.insert("bot_id".to_string(), AttributeValue::S(self.bot_id.clone()));
        map.insert("kind".to_string(), AttributeValue::S(self.kind.clone()));
        map.insert(
            "template_name".to_string(),
            AttributeValue::S(self.template_name.clone()),
        );
        if let Some(version) = &self.template_version {
            map.insert(
                "template_version".to_string(),
                AttributeValue::S(version.clone()),
            );
        }
        map.insert(
            "applied_at".to_string(),
            AttributeValue::N(self.applied_at.to_string()),
        );
        map
    }

    fn from_item(item: &HashMap<String, AttributeValue>) -> Option<Self> {
        Some(Self {
            pk: item.get("pk")?.as_s().ok()?.to_string(),
            sk: item.get("sk")?.as_s().ok()?.to_string(),
            bot_id: item.get("bot_id")?.as_s().ok()?.to_string(),
            kind: item.get("kind")?.as_s().ok()?.to_string(),
            template_name: item.get("template_name")?.as_s().ok()?.to_string(),
            template_version: item
                .get("template_version")
                .and_then(|v| v.as_s().ok())
                .map(|s| s.to_string()),
            applied_at: item.get("applied_at")?.as_n().ok()?.parse().ok()?,
        })
    }

    fn to_domain(&self) -> Option<ConfigSwitchEvent> {
        let user_id = BotItem::extract_user_id_from_pk(&self.pk)?;
        let kind = ConfigSwitchKind::from_str(&self.kind)?;
        Some(ConfigSwitchEvent {
            user_id,
            bot_id: self.bot_id.clone(),
            kind,
            template_name: self.template_name.clone(),
            template_version: self.template_version.clone(),
            applied_at: self.applied_at,
        })
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

    /// Enumerate every bot across all users via a paginated Scan. The return-curve
    /// collector has no per-user context, so it cannot use the single-partition
    /// `find_by_user_id` Query. Non-bot rows sharing a partition (runtime and
    /// config-switch rows) are skipped; a genuinely unparseable bot row still
    /// fails loud, exactly as in `find_by_user_id`.
    pub async fn find_all(&self) -> Result<Vec<Bot>, DomainError> {
        let mut bots = Vec::new();
        let mut start_key: Option<HashMap<String, AttributeValue>> = None;

        loop {
            let mut req = self.client.scan().table_name(&self.table_name);
            if let Some(key) = start_key.take() {
                req = req.set_exclusive_start_key(Some(key));
            }
            let output = req
                .send()
                .await
                .map_err(|e| repo_err("DynamoDB scan failed", e))?;

            for item in output.items() {
                if is_runtime_row(item) || is_config_switch_row(item) {
                    continue;
                }
                let user_id = item
                    .get("pk")
                    .and_then(|v| v.as_s().ok())
                    .and_then(|pk| BotItem::extract_user_id_from_pk(pk))
                    .unwrap_or_default();
                bots.push(parse_bot_row(item, &user_id)?);
            }

            match output.last_evaluated_key() {
                Some(key) if !key.is_empty() => start_key = Some(key.clone()),
                _ => break,
            }
        }

        Ok(bots)
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
            .map_err(|e| repo_err("DynamoDB get_item failed", e))?;

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
            .map_err(|e| repo_err("DynamoDB get_item failed", e))?;

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
        // `:observed_at` is added per-arm (only the timestamp-ordered arms use it);
        // DynamoDB rejects a request whose ExpressionAttributeValues are not all
        // referenced, so the identity-guarded Stopping arm must NOT define it.
        let observed_at = AttributeValue::N(runtime.observed_at.to_string());
        let put = self
            .client
            .put_item()
            .table_name(&self.table_name)
            .set_item(Some(metadata.to_item()));

        let put = match runtime.phase {
            // A terminal STOPPED always settles the row, INCLUDING a `stopping`
            // one stamped slightly in the future by a clock-ahead telebot — without
            // the `#st = :stopping` clause a dropped settlement could leave the row
            // stuck `stopping` forever (there is no periodic sweeper). It only ever
            // fires while the row is actually `stopping`, so it cannot clobber a
            // newer `running`.
            RuntimePhase::Stopped => put
                .condition_expression("attribute_not_exists(task_updated_at) OR task_updated_at <= :observed_at OR #st = :stopping")
                .expression_attribute_names("#st", "status")
                .expression_attribute_values(":observed_at", observed_at)
                .expression_attribute_values(":stopping", AttributeValue::S("stopping".to_string())),
            // A same-second RUNNING must not resurrect a terminal Stopped NOR a
            // just-written Stopping (the task is on its way down), so the tie-break
            // excludes both. A strictly-newer RUNNING still wins (a real relaunch).
            RuntimePhase::Running => put
                .condition_expression("attribute_not_exists(task_updated_at) OR task_updated_at < :observed_at OR (task_updated_at = :observed_at AND #st <> :stopped AND #st <> :stopping)")
                .expression_attribute_names("#st", "status")
                .expression_attribute_values(":observed_at", observed_at)
                .expression_attribute_values(":stopped", AttributeValue::S("stopped".to_string()))
                .expression_attribute_values(":stopping", AttributeValue::S("stopping".to_string())),
            // Stopping is stamped synchronously by StopBot the instant StopTask is
            // issued. IDENTITY-GUARDED: it may only mark the row while it still
            // refers to the exact task we stopped (`task_id = :expected_task`) and
            // that task is still running/starting. It must NEVER clobber a newer
            // generation — e.g. a restart lock a concurrent reconcile just claimed
            // (which clears task_id and bumps the version) — or the relaunched
            // task's id would be silently orphaned, breaking the runtime-row mirror
            // the no-double-run invariant depends on. This arm is purely identity-
            // ordered, so it does NOT reference :observed_at.
            RuntimePhase::Stopping => put
                .condition_expression("(#st = :running OR #st = :starting) AND task_id = :expected_task")
                .expression_attribute_names("#st", "status")
                .expression_attribute_values(":running", AttributeValue::S("running".to_string()))
                .expression_attribute_values(":starting", AttributeValue::S("starting".to_string()))
                .expression_attribute_values(":expected_task", AttributeValue::S(metadata.task_id.clone())),
            // The starting lock is normally written via StartLockRepository's
            // conditional CAS, not this observed-write path. Keep the arm total
            // and conservative: a transient `starting` may only win if strictly
            // newer, so it never overwrites a same-second running/stopped.
            RuntimePhase::Starting => put
                .condition_expression("attribute_not_exists(task_updated_at) OR task_updated_at < :observed_at")
                .expression_attribute_values(":observed_at", observed_at),
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
                    Err(repo_err("DynamoDB put_item failed", e))
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
            .map_err(|e| repo_err("DynamoDB get_item failed", e))?;

        if let Some(meta) = existing.item().and_then(BotECSTaskMetadata::from_item) {
            match meta.status.as_str() {
                "running" => return Ok(StartClaim::AlreadyRunning),
                "starting" if meta.updated_at > stale_cutoff => {
                    return Ok(StartClaim::AlreadyStarting);
                }
                // A freshly-stopping task is still winding down — refuse to launch
                // and tell the caller to retry once it settles. A stale `stopping`
                // (a dropped STOPPED event) falls through to the CAS, whose reclaim
                // is gated by the ECS-liveness check in start_bot.
                "stopping" if meta.updated_at > stale_cutoff => {
                    return Ok(StartClaim::AlreadyStopping);
                }
                _ => {} // stopped, missing, or a stale starting/stopping lock -> attempt to claim
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
            .condition_expression("attribute_not_exists(#st) OR #st = :stopped OR (#st = :starting AND task_updated_at <= :stale_cutoff) OR (#st = :stopping AND task_updated_at <= :stale_cutoff)")
            .expression_attribute_names("#st", "status")
            .expression_attribute_values(":starting", AttributeValue::S("starting".to_string()))
            .expression_attribute_values(":stopped", AttributeValue::S("stopped".to_string()))
            .expression_attribute_values(":stopping", AttributeValue::S("stopping".to_string()))
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
    async fn find(&self, user_id: &str, bot_id: &str) -> Result<Option<Bot>, DomainError> {
        let result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(BotItem::construct_pk(user_id)))
            .key("sk", AttributeValue::S(bot_id.to_string()))
            .send()
            .await
            .map_err(|e| repo_err("DynamoDB get_item failed", e))?;

        // Absent row -> Ok(None); a present-but-unparseable row is a CorruptRecord
        // fault, never collapsed into None, so "bot does not exist" stays distinct
        // from "the row is corrupt" and from "the read failed".
        match result.item() {
            None => Ok(None),
            Some(item) => parse_bot_row(item, user_id).map(Some),
        }
    }

    async fn find_consistent(
        &self,
        user_id: &str,
        bot_id: &str,
    ) -> Result<Option<Bot>, DomainError> {
        let result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(BotItem::construct_pk(user_id)))
            .key("sk", AttributeValue::S(bot_id.to_string()))
            .consistent_read(true)
            .send()
            .await
            .map_err(|e| repo_err("DynamoDB get_item failed", e))?;

        match result.item() {
            None => Ok(None),
            Some(item) => parse_bot_row(item, user_id).map(Some),
        }
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
            .map_err(|e| repo_err("DynamoDB put_item failed", e))?;

        Ok(())
    }

    async fn find_by_user_id(&self, user_id: &str) -> Result<Vec<Bot>, DomainError> {
        let pk_value = BotItem::construct_pk(user_id);

        let output = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("pk = :pk")
            .expression_attribute_values(":pk", AttributeValue::S(pk_value))
            .send()
            .await
            .map_err(|e| repo_err("DynamoDB query failed", e))?;

        // The query returns the whole partition, which mixes bot rows with each
        // bot's `ecs_task_metadata#` runtime row and its `config_switch#` timeline
        // rows; those non-bot rows are skipped. Among the rows that SHOULD be bots,
        // a single unparseable one fails the whole list rather than silently
        // dropping out of it — a partial list that looks complete is the same
        // swallowed-fault trap as a collapsed `None` (docs/conventions.md § Error
        // Handling).
        output
            .items()
            .iter()
            .filter(|item| !is_runtime_row(item) && !is_config_switch_row(item))
            .map(|item| parse_bot_row(item, user_id))
            .collect()
    }

    async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), DomainError> {
        let pk_value = BotItem::construct_pk(user_id);

        self.client
            .delete_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk_value))
            .key("sk", AttributeValue::S(bot_id.to_string()))
            .send()
            .await
            .map_err(|e| repo_err("Failed to delete bot", e))?;

        Ok(())
    }
}

#[async_trait]
impl ConfigSwitchRepository for DynamoBotRepository {
    async fn record(&self, event: &ConfigSwitchEvent) -> Result<(), DomainError> {
        // Append-only: each event has a distinct sort key (…#<applied_at>), so an
        // unconditional put adds to the timeline rather than overwriting it.
        let item = ConfigSwitchItem::from_domain(event).to_item();

        self.client
            .put_item()
            .table_name(&self.table_name)
            .set_item(Some(item))
            .send()
            .await
            .map_err(|e| repo_err("DynamoDB put_item failed", e))?;

        Ok(())
    }

    async fn list_for_bot(
        &self,
        user_id: &str,
        bot_id: &str,
    ) -> Result<Vec<ConfigSwitchEvent>, DomainError> {
        let output = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("pk = :pk AND begins_with(sk, :prefix)")
            .expression_attribute_values(":pk", AttributeValue::S(BotItem::construct_pk(user_id)))
            .expression_attribute_values(
                ":prefix",
                AttributeValue::S(ConfigSwitchItem::sk_prefix_for_bot(bot_id)),
            )
            .send()
            .await
            .map_err(|e| repo_err("DynamoDB query failed", e))?;

        // Every row under this prefix is a switch event; an unparseable one is a
        // CorruptRecord fault, never dropped, so a broken timeline fails loud
        // rather than reading back as a silently short one (docs/conventions.md
        // § Error Handling).
        let mut events = output
            .items()
            .iter()
            .map(|item| {
                ConfigSwitchItem::from_item(item)
                    .and_then(|ci| ci.to_domain())
                    .ok_or_else(|| {
                        DomainError::CorruptRecord(format!(
                            "config_switch row pk=user_id#{user_id} did not parse"
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;

        events.sort_by_key(|e| e.applied_at);
        Ok(events)
    }
}
