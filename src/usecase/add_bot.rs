use crate::domain::bot::{ApiKeyRepository, Bot, BotRepository};
use crate::domain::clock::Clock;
use crate::domain::error::DomainError;
use crate::domain::exchange::Exchange;
use std::sync::Arc;

/// Outcome of an add-bot attempt. A name collision is an expected business
/// branch (`AlreadyExists`), not a fault: the caller confirms an overwrite
/// before anything is written, so a re-add never silently clobbers or
/// duplicates an existing bot.
#[derive(Debug)]
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

    /// Credentials are checked and normalised for the exchange before any
    /// lookup, so a malformed key is refused whether or not the name is taken.
    pub async fn execute(
        &self,
        user_id: &str,
        exchange: Exchange,
        name: String,
        api_key: String,
        secret_key: String,
    ) -> Result<AddOutcome, DomainError> {
        Bot::validate_name(&name)?;
        let (api_key, secret_key) = exchange.validate_credentials(&api_key, &secret_key)?;
        let bots = self.bot_repository.find_by_user_id(user_id).await?;
        // Detect by name, not by id: a same-name bot whose id was not derived
        // from the name (an older row keyed on a numeric account id) would slip
        // past an id lookup and a second row would be created — the duplicate
        // this guards against.
        if let Some(existing) = bots.iter().find(|b| b.name == name) {
            return Ok(AddOutcome::AlreadyExists(existing.clone()));
        }
        Self::ensure_signer_unshared(&bots, None, exchange, &secret_key)?;

        let bot = Bot::create(
            user_id.to_string(),
            exchange,
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
    /// existing bot is changed in place, so everything but the name, the keys
    /// and `updated_at` survives — an overwrite rotates the keys, it does not
    /// reset the bot. Falls back to a fresh create when no bot by that name is
    /// found.
    ///
    /// The exchange is not among what an overwrite changes: the bot's config
    /// and the rest of its state belong to the exchange it was added on, so
    /// keys for another exchange are refused and the bot has to be deleted and
    /// added again.
    pub async fn overwrite(
        &self,
        user_id: &str,
        exchange: Exchange,
        name: String,
        api_key: String,
        secret_key: String,
    ) -> Result<Bot, DomainError> {
        Bot::validate_name(&name)?;
        let (api_key, secret_key) = exchange.validate_credentials(&api_key, &secret_key)?;
        let bots = self.bot_repository.find_by_user_id(user_id).await?;
        let existing = bots.iter().find(|b| b.name == name).cloned();
        if let Some(e) = &existing
            && e.exchange != exchange
        {
            return Err(DomainError::InvalidCredentials(format!(
                "bot {:?} trades on {}; a bot's exchange is fixed, so delete it and add a {} bot instead",
                e.name,
                e.exchange.label(),
                exchange.label()
            )));
        }
        Self::ensure_signer_unshared(
            &bots,
            existing.as_ref().map(|b| b.id.as_str()),
            exchange,
            &secret_key,
        )?;
        let now = self.clock.now();
        let bot = match existing {
            Some(mut existing) => {
                existing.name = name;
                existing.api_key = api_key;
                existing.secret_key = secret_key;
                existing.updated_at = now;
                existing
            }
            None => Bot::create(
                user_id.to_string(),
                exchange,
                name,
                api_key,
                secret_key,
                now,
            ),
        };
        self.persist(&bot).await?;
        Ok(bot)
    }

    /// Refuse a Hyperliquid API wallet another of the account's bots already
    /// signs with (`skip` is the bot being re-keyed). Orders signed by one
    /// wallet share its nonce sequence, so two bots on one wallet reject each
    /// other's orders; each bot needs a wallet of its own.
    fn ensure_signer_unshared(
        bots: &[Bot],
        skip: Option<&str>,
        exchange: Exchange,
        secret_key: &str,
    ) -> Result<(), DomainError> {
        let Some(signer) = exchange.signer_of(secret_key) else {
            return Ok(());
        };
        let shared = bots.iter().find(|b| {
            Some(b.id.as_str()) != skip
                && b.exchange == exchange
                && b.exchange.signer_of(&b.secret_key).as_deref() == Some(signer.as_str())
        });
        match shared {
            Some(other) => Err(DomainError::InvalidCredentials(format!(
                "bot {:?} already uses this API wallet; create another API wallet for this bot",
                other.name
            ))),
            None => Ok(()),
        }
    }

    async fn persist(&self, bot: &Bot) -> Result<(), DomainError> {
        // DynamoDB first, then S3: the bot row is the source of truth the rest
        // of the system reads; the api-keys object is downstream of it.
        self.bot_repository.save(bot).await?;
        self.api_keys_repository.save(bot).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::engine::Runtime;
    use crate::domain::error::DomainError;
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
        async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), DomainError> {
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
        async fn save(&self, bot: &Bot) -> Result<(), DomainError> {
            *self.saved.lock().unwrap() = Some(bot.clone());
            Ok(())
        }
        async fn delete(&self, _user_id: &str, _bot_id: &str) -> Result<(), DomainError> {
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
                Exchange::Bybit,
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
    async fn a_name_with_the_kind_separator_is_refused_before_anything_is_saved() {
        let bots = Arc::new(InMemoryBots::default());
        let api_keys = Arc::new(MockApiKeyRepository::default());
        let uc = AddBotUseCase::new(bots.clone(), api_keys.clone(), Arc::new(FixedClock));

        let err = uc
            .execute(
                "user-1",
                Exchange::Bybit,
                "my#bot".to_string(),
                "ak".to_string(),
                "sk".to_string(),
            )
            .await
            .expect_err("a '#' in the name is refused");
        assert!(matches!(err, DomainError::InvalidBotName(_)), "{err}");
        assert!(bots.get("user-1", "my#bot").is_none());
        assert!(api_keys.saved.lock().unwrap().is_none());

        let err = uc
            .overwrite(
                "user-1",
                Exchange::Bybit,
                "my#bot".to_string(),
                "ak".to_string(),
                "sk".to_string(),
            )
            .await
            .expect_err("overwrite applies the same rule");
        assert!(matches!(err, DomainError::InvalidBotName(_)), "{err}");
    }

    #[tokio::test]
    async fn execute_reports_existing_without_overwriting() {
        let bots = Arc::new(InMemoryBots::default());
        let api_keys = Arc::new(MockApiKeyRepository::default());
        let uc = AddBotUseCase::new(bots.clone(), api_keys, Arc::new(FixedClock));

        let first = uc
            .execute(
                "user-1",
                Exchange::Bybit,
                "dup".into(),
                "ak1".into(),
                "sk1".into(),
            )
            .await
            .unwrap();
        assert!(matches!(first, AddOutcome::Added(_)));

        // A second add with the same name is surfaced, not silently written.
        let second = uc
            .execute(
                "user-1",
                Exchange::Bybit,
                "dup".into(),
                "ak2".into(),
                "sk2".into(),
            )
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
            Runtime::Py,
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
            .execute(
                "user-1",
                Exchange::Bybit,
                "PaperTrader".into(),
                "ak2".into(),
                "sk2".into(),
            )
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
        let mut legacy = Bot::new(
            "452425891".into(),
            "user-1".into(),
            Exchange::Bybit,
            "PaperTrader".into(),
            "old-ak".into(),
            "old-sk".into(),
            true,
            Runtime::Rs,
            100,
            100,
        );
        legacy
            .set_public_url(Some("https://www.bybit.com/x".into()), 100)
            .unwrap();
        let bots = Arc::new(InMemoryBots::default());
        bots.save(&legacy).await.unwrap();
        let api_keys = Arc::new(MockApiKeyRepository::default());
        let uc = AddBotUseCase::new(bots.clone(), api_keys, Arc::new(FixedClock));

        let saved = uc
            .overwrite(
                "user-1",
                Exchange::Bybit,
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
        assert_eq!(row.runtime, Runtime::Rs, "runtime preserved");
        assert_eq!(
            row.public_url.as_deref(),
            Some("https://www.bybit.com/x"),
            "public link preserved"
        );
        assert_eq!(row.created_at, 100, "created_at preserved");
        assert_eq!(row.updated_at, 1_700_000_000, "updated_at bumped to now");
    }

    // Published Hardhat development keys: known to everyone, guarding nothing.
    const AGENT_1: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const AGENT_2: &str = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
    const ACCOUNT: &str = "0x1111111111111111111111111111111111111111";

    fn use_case() -> (Arc<InMemoryBots>, Arc<MockApiKeyRepository>, AddBotUseCase) {
        let bots = Arc::new(InMemoryBots::default());
        let api_keys = Arc::new(MockApiKeyRepository::default());
        let uc = AddBotUseCase::new(bots.clone(), api_keys.clone(), Arc::new(FixedClock));
        (bots, api_keys, uc)
    }

    #[tokio::test]
    async fn a_hyperliquid_bot_is_stored_with_normalised_credentials() {
        let (bots, _, uc) = use_case();
        let out = uc
            .execute(
                "user-1",
                Exchange::Hyperliquid,
                "hl".into(),
                format!(" {} ", &ACCOUNT[2..]),
                AGENT_1.to_ascii_uppercase().replacen("0X", "0x", 1),
            )
            .await
            .unwrap();
        assert!(matches!(out, AddOutcome::Added(_)));
        let row = bots.get("user-1", "hl").unwrap();
        assert_eq!(row.exchange, Exchange::Hyperliquid);
        assert_eq!(row.api_key, ACCOUNT);
        assert_eq!(row.secret_key, AGENT_1);
    }

    #[tokio::test]
    async fn malformed_credentials_are_refused_before_anything_is_saved() {
        let (bots, api_keys, uc) = use_case();
        let err = uc
            .execute(
                "user-1",
                Exchange::Hyperliquid,
                "hl".into(),
                ACCOUNT.into(),
                "sk".into(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::InvalidCredentials(_)), "{err}");
        assert!(bots.get("user-1", "hl").is_none());
        assert!(api_keys.saved.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn two_bots_may_not_sign_with_one_api_wallet() {
        let (_, _, uc) = use_case();
        uc.execute(
            "user-1",
            Exchange::Hyperliquid,
            "a".into(),
            ACCOUNT.into(),
            AGENT_1.into(),
        )
        .await
        .unwrap();
        let err = uc
            .execute(
                "user-1",
                Exchange::Hyperliquid,
                "b".into(),
                ACCOUNT.into(),
                AGENT_1.into(),
            )
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("\"a\" already uses this API wallet"),
            "{err}"
        );

        // A wallet of its own is fine, and re-keying a bot with its own wallet
        // is not a collision with itself.
        uc.execute(
            "user-1",
            Exchange::Hyperliquid,
            "b".into(),
            ACCOUNT.into(),
            AGENT_2.into(),
        )
        .await
        .unwrap();
        uc.overwrite(
            "user-1",
            Exchange::Hyperliquid,
            "a".into(),
            ACCOUNT.into(),
            AGENT_1.into(),
        )
        .await
        .unwrap();
        let err = uc
            .overwrite(
                "user-1",
                Exchange::Hyperliquid,
                "a".into(),
                ACCOUNT.into(),
                AGENT_2.into(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::InvalidCredentials(_)), "{err}");
    }

    #[tokio::test]
    async fn an_overwrite_may_not_move_a_bot_to_another_exchange() {
        let (bots, _, uc) = use_case();
        uc.execute(
            "user-1",
            Exchange::Bybit,
            "b".into(),
            "ak".into(),
            "sk".into(),
        )
        .await
        .unwrap();
        let err = uc
            .overwrite(
                "user-1",
                Exchange::Hyperliquid,
                "b".into(),
                ACCOUNT.into(),
                AGENT_1.into(),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("exchange is fixed"), "{err}");
        let row = bots.get("user-1", "b").unwrap();
        assert_eq!(row.exchange, Exchange::Bybit);
        assert_eq!(row.api_key, "ak");
    }
}
