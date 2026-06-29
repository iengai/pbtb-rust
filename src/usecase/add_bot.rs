use crate::domain::bot::{ApiKeyRepository, Bot, BotRepository};
use crate::domain::clock::Clock;
use std::sync::Arc;

/// Outcome of an add-bot attempt. A name collision is an expected business
/// branch (`AlreadyExists`), not a fault: the caller confirms an overwrite
/// before anything is written, so a re-add never silently clobbers or
/// duplicates an existing bot.
pub enum AddOutcome {
    Added(Bot),
    /// A bot with the requested name already exists; nothing was written.
    /// Carries the existing bot so the caller can describe what an overwrite
    /// would replace.
    AlreadyExists(Bot),
}

pub struct AddBotUseCase {
    bot_repository: Arc<dyn BotRepository + Send + Sync>,
    api_keys_repository: Arc<dyn ApiKeyRepository>,
    clock: Arc<dyn Clock>,
}

impl AddBotUseCase {
    pub fn new(
        bot_repository: Arc<dyn BotRepository + Send + Sync>,
        api_keys_repository: Arc<dyn ApiKeyRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            bot_repository,
            api_keys_repository,
            clock,
        }
    }

    pub async fn execute(
        &self,
        user_id: &str,
        name: String,
        api_key: String,
        secret_key: String,
    ) -> Result<AddOutcome, String> {
        // Detect by name, not by id: a same-name bot whose id was not derived
        // from the name (an older row keyed on a numeric account id) would slip
        // past an id lookup and a second row would be created — the duplicate
        // this guards against.
        if let Some(existing) = self.find_by_name(user_id, &name).await? {
            return Ok(AddOutcome::AlreadyExists(existing));
        }

        let bot = Bot::create(
            user_id.to_string(),
            name,
            api_key,
            secret_key,
            self.clock.now(),
        );
        self.persist(&bot).await?;
        Ok(AddOutcome::Added(bot))
    }

    /// Force-save after the user confirmed overwriting an existing bot. Reuses
    /// the existing bot's id so the existing row is updated in place; saving
    /// under a name-derived id instead would leave a same-name row whose id is
    /// a numeric account id untouched and spawn yet another duplicate. The
    /// desired state (`enabled`) and `created_at` are preserved — an overwrite
    /// rotates the keys, it does not reset the bot. Falls back to a fresh
    /// create when no bot by that name is found.
    pub async fn overwrite(
        &self,
        user_id: &str,
        name: String,
        api_key: String,
        secret_key: String,
    ) -> Result<Bot, String> {
        let now = self.clock.now();
        let bot = match self.find_by_name(user_id, &name).await? {
            Some(existing) => Bot::new(
                existing.id,
                user_id.to_string(),
                existing.exchange,
                name,
                api_key,
                secret_key,
                existing.enabled,
                existing.created_at,
                now,
            ),
            None => Bot::create(user_id.to_string(), name, api_key, secret_key, now),
        };
        self.persist(&bot).await?;
        Ok(bot)
    }

    async fn find_by_name(&self, user_id: &str, name: &str) -> Result<Option<Bot>, String> {
        let bots = self
            .bot_repository
            .find_by_user_id(user_id)
            .await
            .map_err(|e| format!("Failed to look up existing bots: {e}"))?;
        Ok(bots.into_iter().find(|b| b.name == name))
    }

    async fn persist(&self, bot: &Bot) -> Result<(), String> {
        // DynamoDB first, then S3: the bot row is the source of truth the rest
        // of the system reads; the api-keys object is downstream of it.
        self.bot_repository
            .save(bot)
            .await
            .map_err(|e| format!("Failed to save bot: {e}"))?;
        self.api_keys_repository
            .save(bot)
            .await
            .map_err(|e| format!("Failed to save API keys to S3: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::DomainError;
    use crate::domain::exchange::Exchange;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> i64 {
            1_700_000_000
        }
    }

    #[derive(Default)]
    struct InMemoryBots {
        bots: Mutex<HashMap<(String, String), Bot>>,
    }
    impl InMemoryBots {
        fn get(&self, user_id: &str, bot_id: &str) -> Option<Bot> {
            self.bots
                .lock()
                .unwrap()
                .get(&(user_id.to_string(), bot_id.to_string()))
                .cloned()
        }
    }
    #[async_trait]
    impl BotRepository for InMemoryBots {
        async fn find(&self, user_id: &str, bot_id: &str) -> Result<Option<Bot>, DomainError> {
            Ok(self.get(user_id, bot_id))
        }
        async fn save(&self, bot: &Bot) -> Result<(), DomainError> {
            self.bots
                .lock()
                .unwrap()
                .insert((bot.user_id.clone(), bot.id.clone()), bot.clone());
            Ok(())
        }
        async fn find_by_user_id(&self, user_id: &str) -> Result<Vec<Bot>, DomainError> {
            Ok(self
                .bots
                .lock()
                .unwrap()
                .values()
                .filter(|b| b.user_id == user_id)
                .cloned()
                .collect())
        }
        async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), String> {
            self.bots
                .lock()
                .unwrap()
                .remove(&(user_id.to_string(), bot_id.to_string()));
            Ok(())
        }
    }

    /// In-memory ApiKeyRepository whose save/delete always succeed, capturing
    /// the last saved bot so the test can exercise the full success path.
    #[derive(Default)]
    struct MockApiKeyRepository {
        saved: Mutex<Option<Bot>>,
    }
    #[async_trait]
    impl ApiKeyRepository for MockApiKeyRepository {
        async fn save(&self, bot: &Bot) -> Result<(), String> {
            *self.saved.lock().unwrap() = Some(bot.clone());
            Ok(())
        }
        async fn delete(&self, _user_id: &str, _bot_id: &str) -> Result<(), String> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn add_bot_saves_disabled_bot_with_id_equal_to_name() {
        let bots = Arc::new(InMemoryBots::default());
        let api_keys = Arc::new(MockApiKeyRepository::default());
        let uc = AddBotUseCase::new(bots.clone(), api_keys.clone(), Arc::new(FixedClock));

        // Full success path: both DynamoDB and S3 saves succeed.
        let bot = match uc
            .execute(
                "user-1",
                "my-bot".to_string(),
                "ak".to_string(),
                "sk".to_string(),
            )
            .await
            .expect("execute succeeds when both repos succeed")
        {
            AddOutcome::Added(bot) => bot,
            AddOutcome::AlreadyExists(_) => panic!("first add must not collide"),
        };

        // Returned bot reflects the construction policy.
        assert_eq!(bot.id, "my-bot", "id is derived from name");
        assert_eq!(bot.name, "my-bot");
        assert_eq!(bot.user_id, "user-1");
        assert!(!bot.enabled, "new bots start disabled");
        assert_eq!(bot.exchange, Exchange::Bybit);
        assert_eq!(bot.created_at, 1_700_000_000);
        assert_eq!(bot.updated_at, 1_700_000_000);

        // The bot was persisted to the bot repo.
        let saved = bots.get("user-1", "my-bot").expect("bot saved to bot repo");
        assert_eq!(saved.id, "my-bot");
        assert!(!saved.enabled);

        // The api keys repo received the same bot.
        let api_saved = api_keys
            .saved
            .lock()
            .unwrap()
            .clone()
            .expect("api keys saved");
        assert_eq!(api_saved.id, "my-bot");
        assert_eq!(api_saved.exchange, Exchange::Bybit);
    }

    #[tokio::test]
    async fn execute_reports_existing_without_overwriting() {
        let bots = Arc::new(InMemoryBots::default());
        let api_keys = Arc::new(MockApiKeyRepository::default());
        let uc = AddBotUseCase::new(bots.clone(), api_keys, Arc::new(FixedClock));

        let first = uc
            .execute("user-1", "dup".into(), "ak1".into(), "sk1".into())
            .await
            .unwrap();
        assert!(matches!(first, AddOutcome::Added(_)));

        // A second add with the same name is surfaced, not silently written.
        let second = uc
            .execute("user-1", "dup".into(), "ak2".into(), "sk2".into())
            .await
            .unwrap();
        match second {
            AddOutcome::AlreadyExists(existing) => assert_eq!(existing.name, "dup"),
            AddOutcome::Added(_) => panic!("a duplicate name must not be added"),
        }

        // The stored bot still carries the original keys — no overwrite happened.
        assert_eq!(bots.get("user-1", "dup").unwrap().api_key, "ak1");
    }

    #[tokio::test]
    async fn execute_detects_existing_bot_even_when_id_differs_from_name() {
        // A legacy row whose id is a numeric account id, not the name — the
        // exact shape that produced two "PaperTrader" entries.
        let legacy = Bot::new(
            "452425891".into(),
            "user-1".into(),
            Exchange::Bybit,
            "PaperTrader".into(),
            "ak".into(),
            "sk".into(),
            false,
            1,
            1,
        );
        let bots = Arc::new(InMemoryBots::default());
        bots.save(&legacy).await.unwrap();
        let uc = AddBotUseCase::new(
            bots.clone(),
            Arc::new(MockApiKeyRepository::default()),
            Arc::new(FixedClock),
        );

        let out = uc
            .execute("user-1", "PaperTrader".into(), "ak2".into(), "sk2".into())
            .await
            .unwrap();
        assert!(
            matches!(out, AddOutcome::AlreadyExists(_)),
            "name collision detected despite id != name"
        );
        assert_eq!(
            bots.find_by_user_id("user-1").await.unwrap().len(),
            1,
            "no duplicate row created"
        );
    }

    #[tokio::test]
    async fn overwrite_updates_in_place_reusing_existing_id() {
        let legacy = Bot::new(
            "452425891".into(),
            "user-1".into(),
            Exchange::Bybit,
            "PaperTrader".into(),
            "old-ak".into(),
            "old-sk".into(),
            true,
            100,
            100,
        );
        let bots = Arc::new(InMemoryBots::default());
        bots.save(&legacy).await.unwrap();
        let api_keys = Arc::new(MockApiKeyRepository::default());
        let uc = AddBotUseCase::new(bots.clone(), api_keys, Arc::new(FixedClock));

        let saved = uc
            .overwrite(
                "user-1",
                "PaperTrader".into(),
                "new-ak".into(),
                "new-sk".into(),
            )
            .await
            .unwrap();

        assert_eq!(saved.id, "452425891", "reuses the existing id");
        assert_eq!(
            bots.find_by_user_id("user-1").await.unwrap().len(),
            1,
            "overwrites in place rather than spawning a duplicate"
        );
        let row = bots.get("user-1", "452425891").unwrap();
        assert_eq!(row.api_key, "new-ak", "keys rotated");
        assert!(row.enabled, "desired state preserved");
        assert_eq!(row.created_at, 100, "created_at preserved");
        assert_eq!(row.updated_at, 1_700_000_000, "updated_at bumped to now");
    }
}
