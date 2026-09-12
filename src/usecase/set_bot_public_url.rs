use std::sync::Arc;

use crate::domain::bot::BotRepository;
use crate::domain::clock::Clock;
use crate::domain::error::DomainError;
use crate::domain::user::Role;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetPublicUrlOutcome {
    /// The row is written; `previous` lets the caller say what changed.
    Updated {
        previous: Option<String>,
        public_url: Option<String>,
    },
    /// The bot already carried this value; nothing was written.
    Unchanged {
        public_url: Option<String>,
    },
    BotNotFound,
}

/// Give a bot its Bybit copy-trading link, or take it away. A bot that
/// carries one is meant for the public showcase page, so the link belongs to
/// the operator's account alone: the page is one deployment's showcase, not a
/// per-tenant feature. The value itself is validated by the domain, so an
/// off-Bybit link never reaches a row.
pub struct SetBotPublicUrlUseCase {
    bots: Arc<dyn BotRepository>,
    clock: Arc<dyn Clock>,
}

impl SetBotPublicUrlUseCase {
    pub fn new(bots: Arc<dyn BotRepository>, clock: Arc<dyn Clock>) -> Self {
        Self { bots, clock }
    }

    pub async fn execute(
        &self,
        role: Role,
        user_id: &str,
        bot_id: &str,
        public_url: Option<String>,
    ) -> Result<SetPublicUrlOutcome, DomainError> {
        // Refused before the bot is read: a member gets the same answer whether
        // or not the bot exists, so the command reveals nothing about rows.
        if !role.is_operator() {
            return Err(DomainError::OperatorOnly);
        }
        let mut bot = match self.bots.find(user_id, bot_id).await? {
            Some(b) => b,
            None => return Ok(SetPublicUrlOutcome::BotNotFound),
        };
        if bot.public_url == public_url {
            return Ok(SetPublicUrlOutcome::Unchanged { public_url });
        }
        let previous = bot.public_url.clone();
        bot.set_public_url(public_url.clone(), self.clock.now())?;
        self.bots.save(&bot).await?;
        Ok(SetPublicUrlOutcome::Updated {
            previous,
            public_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::bot::Bot;
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
        rows: Mutex<HashMap<String, Bot>>,
    }
    #[async_trait]
    impl BotRepository for InMemoryBots {
        async fn find(&self, _u: &str, bot_id: &str) -> Result<Option<Bot>, DomainError> {
            Ok(self.rows.lock().unwrap().get(bot_id).cloned())
        }
        async fn save(&self, bot: &Bot) -> Result<(), DomainError> {
            self.rows
                .lock()
                .unwrap()
                .insert(bot.id.clone(), bot.clone());
            Ok(())
        }
        async fn find_by_user_id(&self, _u: &str) -> Result<Vec<Bot>, DomainError> {
            Ok(self.rows.lock().unwrap().values().cloned().collect())
        }
        async fn delete(&self, _u: &str, bot_id: &str) -> Result<(), DomainError> {
            self.rows.lock().unwrap().remove(bot_id);
            Ok(())
        }
    }

    const LINK: &str = "https://www.bybit.com/copyTrade/trade-center/detail?leaderMark=abc";

    async fn bots_with_one() -> Arc<InMemoryBots> {
        let bots = Arc::new(InMemoryBots::default());
        bots.save(&Bot::create(
            "u".into(),
            "b".into(),
            "ak".into(),
            "sk".into(),
            1,
        ))
        .await
        .unwrap();
        bots
    }

    fn usecase(bots: Arc<InMemoryBots>) -> SetBotPublicUrlUseCase {
        SetBotPublicUrlUseCase::new(bots, Arc::new(FixedClock))
    }

    #[tokio::test]
    async fn the_operator_sets_a_bybit_link_and_stamps_updated_at() {
        let bots = bots_with_one().await;
        let out = usecase(bots.clone())
            .execute(Role::Operator, "u", "b", Some(LINK.into()))
            .await
            .unwrap();
        assert_eq!(
            out,
            SetPublicUrlOutcome::Updated {
                previous: None,
                public_url: Some(LINK.into())
            }
        );
        let saved = bots.find("u", "b").await.unwrap().unwrap();
        assert_eq!(saved.public_url.as_deref(), Some(LINK));
        assert_eq!(saved.updated_at, 1_700_000_000);
    }

    #[tokio::test]
    async fn refused_when_the_sender_is_not_an_operator() {
        let bots = bots_with_one().await;
        let err = usecase(bots.clone())
            .execute(Role::Member, "u", "b", Some(LINK.into()))
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::OperatorOnly), "{err}");
        let saved = bots.find("u", "b").await.unwrap().unwrap();
        assert_eq!(saved.public_url, None, "nothing was written");
        assert_eq!(saved.updated_at, 1, "no write: updated_at keeps its stamp");
        // A member is refused before the lookup, so a missing bot answers the same.
        let err = usecase(bots)
            .execute(Role::Member, "u", "nope", None)
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::OperatorOnly));
    }

    #[tokio::test]
    async fn a_link_off_bybit_is_refused_and_not_written() {
        let bots = bots_with_one().await;
        for bad in [
            "https://evil.example\\@www.bybit.com",
            "https://www.bybit.com.evil.example/x",
            "https://bybit.com@evil.example",
            "http://www.bybit.com/x",
        ] {
            let err = usecase(bots.clone())
                .execute(Role::Operator, "u", "b", Some(bad.into()))
                .await
                .unwrap_err();
            assert!(
                matches!(err, DomainError::InvalidPublicUrl(_)),
                "{bad}: {err}"
            );
        }
        let saved = bots.find("u", "b").await.unwrap().unwrap();
        assert_eq!(saved.public_url, None);
        assert_eq!(saved.updated_at, 1);
    }

    #[tokio::test]
    async fn the_same_link_is_unchanged_without_a_write() {
        let bots = bots_with_one().await;
        let uc = usecase(bots.clone());
        uc.execute(Role::Operator, "u", "b", Some(LINK.into()))
            .await
            .unwrap();
        let out = uc
            .execute(Role::Operator, "u", "b", Some(LINK.into()))
            .await
            .unwrap();
        assert_eq!(
            out,
            SetPublicUrlOutcome::Unchanged {
                public_url: Some(LINK.into())
            }
        );
        // A private bot asked to stay private is unchanged too.
        let out = usecase(bots_with_one().await)
            .execute(Role::Operator, "u", "b", None)
            .await
            .unwrap();
        assert_eq!(out, SetPublicUrlOutcome::Unchanged { public_url: None });
    }

    #[tokio::test]
    async fn off_clears_the_link() {
        let bots = bots_with_one().await;
        let uc = usecase(bots.clone());
        uc.execute(Role::Operator, "u", "b", Some(LINK.into()))
            .await
            .unwrap();
        let out = uc.execute(Role::Operator, "u", "b", None).await.unwrap();
        assert_eq!(
            out,
            SetPublicUrlOutcome::Updated {
                previous: Some(LINK.into()),
                public_url: None
            }
        );
        assert_eq!(bots.find("u", "b").await.unwrap().unwrap().public_url, None);
    }

    #[tokio::test]
    async fn missing_bot_is_an_outcome_not_an_error() {
        let out = usecase(Arc::new(InMemoryBots::default()))
            .execute(Role::Operator, "u", "nope", Some(LINK.into()))
            .await
            .unwrap();
        assert_eq!(out, SetPublicUrlOutcome::BotNotFound);
    }
}
