use crate::domain::bot::{ApiKeyRepository, BotRepository};
use crate::domain::error::DomainError;
use std::sync::Arc;

pub struct DeleteBotUseCase {
    bot_repository: Arc<dyn BotRepository + Send + Sync>,
    api_keys_repository: Arc<dyn ApiKeyRepository>,
}

impl DeleteBotUseCase {
    pub fn new(
        bot_repository: Arc<dyn BotRepository + Send + Sync>,
        api_keys_repository: Arc<dyn ApiKeyRepository>,
    ) -> Self {
        Self {
            bot_repository,
            api_keys_repository,
        }
    }

    /// Delete a bot and the exchange credentials stored for it.
    ///
    /// Two stores, so a fault can land between them. The credentials go first,
    /// which decides which half survives a partial failure: a bot row with no
    /// keys is visible to the user and deleting it again finishes the job, while
    /// keys with no bot row are invisible — nothing lists them and no retry is
    /// prompted, so an exchange secret outlives the bot it belonged to.
    pub async fn execute(&self, user_id: &str, bot_id: &str) -> Result<(), DomainError> {
        self.api_keys_repository.delete(user_id, bot_id).await?;
        self.bot_repository.delete(user_id, bot_id).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::bot::Bot;
    use crate::domain::error::Retryability;
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Store {
        bot_deleted: Mutex<bool>,
        keys_deleted: Mutex<bool>,
        keys_fail: bool,
        bot_fail: bool,
    }

    fn fault(what: &str) -> DomainError {
        DomainError::Repository {
            context: format!("{what} store unavailable"),
            retry: Retryability::Transient,
            source: "boom".into(),
        }
    }

    #[async_trait]
    impl BotRepository for Store {
        async fn find(&self, _u: &str, _b: &str) -> Result<Option<Bot>, DomainError> {
            Ok(None)
        }
        async fn save(&self, _bot: &Bot) -> Result<(), DomainError> {
            Ok(())
        }
        async fn find_by_user_id(&self, _u: &str) -> Result<Vec<Bot>, DomainError> {
            Ok(vec![])
        }
        async fn delete(&self, _u: &str, _b: &str) -> Result<(), DomainError> {
            if self.bot_fail {
                return Err(fault("bot"));
            }
            *self.bot_deleted.lock().unwrap() = true;
            Ok(())
        }
    }

    #[async_trait]
    impl ApiKeyRepository for Store {
        async fn save(&self, _bot: &Bot) -> Result<(), DomainError> {
            Ok(())
        }
        async fn delete(&self, _u: &str, _b: &str) -> Result<(), DomainError> {
            if self.keys_fail {
                return Err(fault("key"));
            }
            *self.keys_deleted.lock().unwrap() = true;
            Ok(())
        }
    }

    fn usecase(store: Arc<Store>) -> DeleteBotUseCase {
        DeleteBotUseCase::new(store.clone(), store)
    }

    #[tokio::test]
    async fn deletes_both_the_bot_and_its_keys() {
        let store = Arc::new(Store::default());
        usecase(store.clone())
            .execute("u", "b")
            .await
            .expect("delete");

        assert!(*store.bot_deleted.lock().unwrap());
        assert!(*store.keys_deleted.lock().unwrap());
    }

    #[tokio::test]
    async fn a_failed_key_delete_leaves_the_bot_row_to_retry_from() {
        let store = Arc::new(Store {
            keys_fail: true,
            ..Default::default()
        });
        usecase(store.clone())
            .execute("u", "b")
            .await
            .expect_err("the key store refused");

        assert!(
            !*store.bot_deleted.lock().unwrap(),
            "the bot must survive a failed key delete: it is the only thing that still \
             points at the orphaned keys, and the user's retry is what removes them"
        );
    }

    #[tokio::test]
    async fn a_failed_bot_delete_has_already_removed_the_keys() {
        let store = Arc::new(Store {
            bot_fail: true,
            ..Default::default()
        });
        usecase(store.clone())
            .execute("u", "b")
            .await
            .expect_err("the bot store refused");

        assert!(
            *store.keys_deleted.lock().unwrap(),
            "an exchange secret must never outlive the bot's deletion; the surviving \
             half is the visible, retryable one"
        );
    }
}
