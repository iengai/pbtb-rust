use std::sync::Arc;

use crate::domain::bot::{Bot, BotRepository};
use crate::domain::clock::Clock;
use crate::domain::error::DomainError;
use crate::domain::showcase::{Published, ShowcasePublisher};
use crate::domain::user::Role;

/// `published` is whether the bot's artifact is on the public prefix after the
/// call: `false` for a hidden bot and for a shown one with no collected curve
/// yet, `None` where no publisher is configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetShowcaseOutcome {
    /// The choice is recorded.
    Updated {
        shown: bool,
        published: Option<bool>,
    },
    /// The bot already carried this choice; the row was not written.
    Unchanged {
        shown: bool,
        published: Option<bool>,
    },
    BotNotFound,
}

/// Put the operator's own bots on the public showcase page or take them off
/// it. The page is one deployment's showcase, so both the list of candidates
/// and the choice belong to the operator's account alone; the bots offered are
/// the caller's own, looked up under the caller's tenant.
///
/// The row is the record. After it, the bot's public artifact is brought in
/// line with it, on an unchanged choice too, so a retry repairs a publish that
/// failed after the save.
pub struct BotShowcaseUseCase {
    bots: Arc<dyn BotRepository>,
    clock: Arc<dyn Clock>,
    publisher: Option<Arc<dyn ShowcasePublisher>>,
}

impl BotShowcaseUseCase {
    pub fn new(
        bots: Arc<dyn BotRepository>,
        clock: Arc<dyn Clock>,
        publisher: Option<Arc<dyn ShowcasePublisher>>,
    ) -> Self {
        Self {
            bots,
            clock,
            publisher,
        }
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
        let unchanged = bot.showcase == Some(shown);
        if !unchanged {
            bot.set_showcase(shown, self.clock.now());
            self.bots.save(&bot).await?;
        }
        let published = self.reconcile(&bot, shown).await?;
        Ok(if unchanged {
            SetShowcaseOutcome::Unchanged { shown, published }
        } else {
            SetShowcaseOutcome::Updated { shown, published }
        })
    }

    async fn reconcile(&self, bot: &Bot, shown: bool) -> Result<Option<bool>, DomainError> {
        let Some(publisher) = &self.publisher else {
            return Ok(None);
        };
        if shown {
            Ok(Some(publisher.publish(bot).await? == Published::Live))
        } else {
            publisher.withdraw(bot).await?;
            Ok(Some(false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::Retryability;
    use crate::domain::exchange::Exchange;
    use async_trait::async_trait;
    use std::collections::{HashMap, HashSet};
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

    /// Records every call; `curves` are the bots with a collected curve, and
    /// `fault` makes every call fail with that retryability.
    #[derive(Default)]
    struct FakePublisher {
        curves: HashSet<String>,
        calls: Mutex<Vec<String>>,
        fault: Mutex<Option<Retryability>>,
    }

    impl FakePublisher {
        fn with_curve(bot_id: &str) -> Arc<Self> {
            Arc::new(Self {
                curves: HashSet::from([bot_id.to_string()]),
                ..Self::default()
            })
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn fail_with(&self, retry: Retryability) -> Result<(), DomainError> {
            match *self.fault.lock().unwrap() {
                Some(_) => Err(DomainError::repository_with(
                    "s3",
                    retry,
                    std::io::Error::other("unreachable"),
                )),
                None => Ok(()),
            }
        }
    }

    #[async_trait]
    impl ShowcasePublisher for FakePublisher {
        async fn publish(&self, bot: &Bot) -> Result<Published, DomainError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("publish {}", bot.id));
            let fault = *self.fault.lock().unwrap();
            if let Some(retry) = fault {
                self.fail_with(retry)?;
            }
            Ok(if self.curves.contains(&bot.id) {
                Published::Live
            } else {
                Published::NoCurveYet
            })
        }
        async fn withdraw(&self, bot: &Bot) -> Result<(), DomainError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("withdraw {}", bot.id));
            let fault = *self.fault.lock().unwrap();
            if let Some(retry) = fault {
                self.fail_with(retry)?;
            }
            Ok(())
        }
    }

    const LINK: &str = "https://www.bybit.com/copyTrade/trade-center/detail?leaderMark=abc";

    async fn a_linked_bot() -> Arc<InMemoryBots> {
        let bots = Arc::new(InMemoryBots::default());
        let mut bot = Bot::create(
            "u".into(),
            Exchange::Bybit,
            "b".into(),
            "ak".into(),
            "sk".into(),
            1,
        );
        bot.set_public_url(Some(LINK.into()), 1).unwrap();
        bots.save(&bot).await.unwrap();
        bots
    }

    fn usecase(
        bots: Arc<InMemoryBots>,
        publisher: Option<Arc<FakePublisher>>,
    ) -> BotShowcaseUseCase {
        BotShowcaseUseCase::new(
            bots,
            Arc::new(FixedClock),
            publisher.map(|p| p as Arc<dyn ShowcasePublisher>),
        )
    }

    #[tokio::test]
    async fn hiding_a_bot_keeps_its_link_and_withdraws_its_artifact() {
        let bots = a_linked_bot().await;
        let publisher = FakePublisher::with_curve("b");
        let out = usecase(bots.clone(), Some(publisher.clone()))
            .set(Role::Operator, "u", "b", false)
            .await
            .unwrap();
        assert_eq!(
            out,
            SetShowcaseOutcome::Updated {
                shown: false,
                published: Some(false)
            }
        );
        let saved = bots.find("u", "b").await.unwrap().unwrap();
        assert!(!saved.on_showcase());
        assert_eq!(saved.public_url.as_deref(), Some(LINK));
        assert_eq!(saved.updated_at, 1_700_000_000);
        assert_eq!(publisher.calls(), ["withdraw b"]);
    }

    #[tokio::test]
    async fn showing_a_bot_publishes_its_collected_curve() {
        let bots = a_linked_bot().await;
        let publisher = FakePublisher::with_curve("b");
        let out = usecase(bots.clone(), Some(publisher.clone()))
            .set(Role::Operator, "u", "b", true)
            .await
            .unwrap();
        assert_eq!(
            out,
            SetShowcaseOutcome::Updated {
                shown: true,
                published: Some(true)
            }
        );
        assert_eq!(
            bots.find("u", "b").await.unwrap().unwrap().showcase,
            Some(true)
        );
        assert_eq!(publisher.calls(), ["publish b"]);
    }

    #[tokio::test]
    async fn a_bot_without_a_collected_curve_is_saved_but_not_published() {
        let bots = a_linked_bot().await;
        let publisher = Arc::new(FakePublisher::default());
        let out = usecase(bots.clone(), Some(publisher))
            .set(Role::Operator, "u", "b", true)
            .await
            .unwrap();
        assert_eq!(
            out,
            SetShowcaseOutcome::Updated {
                shown: true,
                published: Some(false)
            }
        );
        assert_eq!(
            bots.find("u", "b").await.unwrap().unwrap().showcase,
            Some(true)
        );
    }

    #[tokio::test]
    async fn an_unchanged_choice_still_reconciles_the_artifact() {
        let bots = a_linked_bot().await;
        let publisher = FakePublisher::with_curve("b");
        let uc = usecase(bots.clone(), Some(publisher.clone()));
        uc.set(Role::Operator, "u", "b", true).await.unwrap();
        let out = uc.set(Role::Operator, "u", "b", true).await.unwrap();
        assert_eq!(
            out,
            SetShowcaseOutcome::Unchanged {
                shown: true,
                published: Some(true)
            }
        );
        assert_eq!(publisher.calls(), ["publish b", "publish b"]);
    }

    #[tokio::test]
    async fn a_publish_fault_after_the_save_keeps_its_retryability_and_the_row() {
        for retry in [Retryability::Transient, Retryability::Permanent] {
            let bots = a_linked_bot().await;
            let publisher = FakePublisher::with_curve("b");
            *publisher.fault.lock().unwrap() = Some(retry);
            let err = usecase(bots.clone(), Some(publisher))
                .set(Role::Operator, "u", "b", false)
                .await
                .unwrap_err();
            assert_eq!(err.retryability(), retry);
            assert_eq!(
                bots.find("u", "b").await.unwrap().unwrap().showcase,
                Some(false),
                "the row is the record; a retry reconciles the artifact"
            );
        }
    }

    #[tokio::test]
    async fn a_member_is_refused_and_nothing_is_written_or_published() {
        let bots = a_linked_bot().await;
        let publisher = FakePublisher::with_curve("b");
        let uc = usecase(bots.clone(), Some(publisher.clone()));
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
        assert!(publisher.calls().is_empty());
    }

    #[tokio::test]
    async fn without_a_publisher_the_choice_is_saved_alone() {
        let bots = a_linked_bot().await;
        let out = usecase(bots.clone(), None)
            .set(Role::Operator, "u", "b", false)
            .await
            .unwrap();
        assert_eq!(
            out,
            SetShowcaseOutcome::Updated {
                shown: false,
                published: None
            }
        );
    }

    #[tokio::test]
    async fn the_operator_is_offered_their_own_bots_and_a_missing_one_is_an_outcome() {
        let bots = a_linked_bot().await;
        let publisher = FakePublisher::with_curve("b");
        let uc = usecase(bots, Some(publisher.clone()));
        let listed = uc.candidates(Role::Operator, "u").await.unwrap();
        assert_eq!(
            listed.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(),
            ["b"]
        );
        assert_eq!(
            uc.set(Role::Operator, "u", "nope", true).await.unwrap(),
            SetShowcaseOutcome::BotNotFound
        );
        assert!(
            publisher.calls().is_empty(),
            "a missing bot publishes nothing"
        );
    }
}
