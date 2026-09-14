use std::sync::Arc;

use crate::domain::bot::{Bot, BotRepository};
use crate::domain::clock::Clock;
use crate::domain::error::DomainError;
use crate::domain::user::Role;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetShowcaseOutcome {
    /// The choice is recorded.
    Updated {
        shown: bool,
    },
    /// The bot already carried this choice; nothing was written.
    Unchanged {
        shown: bool,
    },
    BotNotFound,
}

/// Put the operator's own bots on the public showcase page or take them off
/// it. The page is one deployment's showcase, so both the list of candidates
/// and the choice belong to the operator's account alone; the bots offered are
/// the caller's own, looked up under the caller's tenant.
pub struct BotShowcaseUseCase {
    bots: Arc<dyn BotRepository>,
    clock: Arc<dyn Clock>,
}

impl BotShowcaseUseCase {
    pub fn new(bots: Arc<dyn BotRepository>, clock: Arc<dyn Clock>) -> Self {
        Self { bots, clock }
    }

    /// Every bot of the caller's that could be shown, with its current state
    /// on `Bot::on_showcase`.
    pub async fn candidates(&self, role: Role, user_id: &str) -> Result<Vec<Bot>, DomainError> {
        if !role.is_operator() {
            return Err(DomainError::OperatorOnly);
        }
        self.bots.find_by_user_id(user_id).await
    }

    /// Record the choice. A bot that never had one is written even when it is
    /// already shown by its link: the choice is what holds from then on, so a
    /// later change to the link cannot take the bot off the page.
    pub async fn set(
        &self,
        role: Role,
        user_id: &str,
        bot_id: &str,
        shown: bool,
    ) -> Result<SetShowcaseOutcome, DomainError> {
        // Refused before the bot is read, so a member gets the same answer
        // whether or not the bot exists.
        if !role.is_operator() {
            return Err(DomainError::OperatorOnly);
        }
        let Some(mut bot) = self.bots.find(user_id, bot_id).await? else {
            return Ok(SetShowcaseOutcome::BotNotFound);
        };
        if bot.showcase == Some(shown) {
            return Ok(SetShowcaseOutcome::Unchanged { shown });
        }
        bot.set_showcase(shown, self.clock.now());
        self.bots.save(&bot).await?;
        Ok(SetShowcaseOutcome::Updated { shown })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    async fn a_linked_bot() -> Arc<InMemoryBots> {
        let bots = Arc::new(InMemoryBots::default());
        let mut bot = Bot::create("u".into(), "b".into(), "ak".into(), "sk".into(), 1);
        bot.set_public_url(Some(LINK.into()), 1).unwrap();
        bots.save(&bot).await.unwrap();
        bots
    }

    fn usecase(bots: Arc<InMemoryBots>) -> BotShowcaseUseCase {
        BotShowcaseUseCase::new(bots, Arc::new(FixedClock))
    }

    #[tokio::test]
    async fn hiding_a_bot_keeps_its_link() {
        let bots = a_linked_bot().await;
        let out = usecase(bots.clone())
            .set(Role::Operator, "u", "b", false)
            .await
            .unwrap();
        assert_eq!(out, SetShowcaseOutcome::Updated { shown: false });
        let saved = bots.find("u", "b").await.unwrap().unwrap();
        assert!(!saved.on_showcase());
        assert_eq!(saved.public_url.as_deref(), Some(LINK));
        assert_eq!(saved.updated_at, 1_700_000_000);
    }

    #[tokio::test]
    async fn a_bot_shown_by_its_link_has_the_choice_recorded() {
        let bots = a_linked_bot().await;
        let uc = usecase(bots.clone());
        let out = uc.set(Role::Operator, "u", "b", true).await.unwrap();
        assert_eq!(out, SetShowcaseOutcome::Updated { shown: true });
        assert_eq!(
            bots.find("u", "b").await.unwrap().unwrap().showcase,
            Some(true)
        );

        let out = uc.set(Role::Operator, "u", "b", true).await.unwrap();
        assert_eq!(out, SetShowcaseOutcome::Unchanged { shown: true });
    }

    #[tokio::test]
    async fn a_member_is_refused_and_nothing_is_written() {
        let bots = a_linked_bot().await;
        let uc = usecase(bots.clone());
        for bot_id in ["b", "nope"] {
            let err = uc.set(Role::Member, "u", bot_id, false).await.unwrap_err();
            assert!(matches!(err, DomainError::OperatorOnly), "{bot_id}: {err}");
        }
        assert!(matches!(
            uc.candidates(Role::Member, "u").await,
            Err(DomainError::OperatorOnly)
        ));
        let saved = bots.find("u", "b").await.unwrap().unwrap();
        assert_eq!(saved.showcase, None);
        assert_eq!(saved.updated_at, 1);
    }

    #[tokio::test]
    async fn the_operator_is_offered_their_own_bots_and_a_missing_one_is_an_outcome() {
        let bots = a_linked_bot().await;
        let uc = usecase(bots);
        let listed = uc.candidates(Role::Operator, "u").await.unwrap();
        assert_eq!(
            listed.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(),
            ["b"]
        );
        assert_eq!(
            uc.set(Role::Operator, "u", "nope", true).await.unwrap(),
            SetShowcaseOutcome::BotNotFound
        );
    }
}
