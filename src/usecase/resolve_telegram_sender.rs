use std::sync::Arc;

use crate::domain::error::DomainError;
use crate::domain::identity::{IdentityRepository, PROVIDER_TELEGRAM};
use crate::domain::user::UserRepository;

/// Who a Telegram sender is, once resolved: the tenant every handler acts as,
/// and the level the tenant's entitlements are read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramSender {
    pub user_id: String,
    pub vip_level: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SenderResolution {
    Known(TelegramSender),
    /// No account has bound this Telegram id. Signing up happens on the web,
    /// so the bot has nothing to offer but directions.
    Unbound,
    /// Bound to an account an operator has suspended.
    Suspended,
}

/// Turn the Telegram id an update carries into the account behind it.
///
/// This is the whole of the bot's authentication: Telegram vouches for the
/// sender id, and the `telegram` identity row says which account chose to be
/// reachable by that id. There is no allowlist in front of it — an account
/// exists because someone signed up on the web, and it is turned away only by
/// being suspended.
pub struct ResolveTelegramSenderUseCase {
    identities: Arc<dyn IdentityRepository>,
    users: Arc<dyn UserRepository>,
}

impl ResolveTelegramSenderUseCase {
    pub fn new(identities: Arc<dyn IdentityRepository>, users: Arc<dyn UserRepository>) -> Self {
        Self { identities, users }
    }

    pub async fn execute(&self, telegram_id: &str) -> Result<SenderResolution, DomainError> {
        let Some(link) = self
            .identities
            .find_link(PROVIDER_TELEGRAM, telegram_id)
            .await?
        else {
            return Ok(SenderResolution::Unbound);
        };

        let Some(user) = self.users.find_user(&link.user_id).await? else {
            // An identity pointing at no account. It cannot be acted on, and it
            // must not be silently ignored either: it is a row someone has to
            // look at.
            tracing::error!(
                user_id = %link.user_id,
                "telegram identity is bound to an account that has no row"
            );
            return Ok(SenderResolution::Unbound);
        };

        if !user.is_active() {
            return Ok(SenderResolution::Suspended);
        }

        Ok(SenderResolution::Known(TelegramSender {
            user_id: user.id,
            vip_level: user.vip_level,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{LinkOutcome, LinkedIdentity};
    use crate::domain::user::{User, UserStatus};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Links(Mutex<HashMap<(String, String), String>>);

    #[async_trait]
    impl IdentityRepository for Links {
        async fn find_link(
            &self,
            provider: &str,
            subject: &str,
        ) -> Result<Option<LinkedIdentity>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(&(provider.to_string(), subject.to_string()))
                .map(|user_id| LinkedIdentity {
                    user_id: user_id.clone(),
                    email: None,
                    linked_at: 0,
                }))
        }
        async fn link(
            &self,
            provider: &str,
            subject: &str,
            identity: &LinkedIdentity,
        ) -> Result<LinkOutcome, DomainError> {
            self.0.lock().unwrap().insert(
                (provider.to_string(), subject.to_string()),
                identity.user_id.clone(),
            );
            Ok(LinkOutcome::Linked)
        }
        async fn links_of(&self, _user_id: &str) -> Result<Vec<(String, String)>, DomainError> {
            Ok(vec![])
        }
        async fn unlink(&self, _: &str, _: &str, _: &str) -> Result<bool, DomainError> {
            Ok(false)
        }
    }

    #[derive(Default)]
    struct Users(Mutex<HashMap<String, User>>);

    #[async_trait]
    impl UserRepository for Users {
        async fn find_user(&self, user_id: &str) -> Result<Option<User>, DomainError> {
            Ok(self.0.lock().unwrap().get(user_id).cloned())
        }
        async fn create_user(&self, user: &User) -> Result<bool, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .insert(user.id.clone(), user.clone())
                .is_none())
        }
        async fn set_vip_level(&self, _: &str, _: u8, _: i64) -> Result<bool, DomainError> {
            Ok(false)
        }
        async fn set_status(&self, _: &str, _: UserStatus, _: i64) -> Result<bool, DomainError> {
            Ok(false)
        }
    }

    async fn given(links: &Links, users: &Users, tg: &str, user_id: &str, status: UserStatus) {
        links
            .link(
                PROVIDER_TELEGRAM,
                tg,
                &LinkedIdentity {
                    user_id: user_id.to_string(),
                    email: None,
                    linked_at: 0,
                },
            )
            .await
            .unwrap();
        let mut user = User::new(user_id.to_string(), None, 0);
        user.vip_level = 4;
        user.status = status;
        users.create_user(&user).await.unwrap();
    }

    #[tokio::test]
    async fn a_bound_active_sender_resolves_to_its_account_and_level() {
        let (links, users) = (Arc::new(Links::default()), Arc::new(Users::default()));
        given(&links, &users, "111", "acct-1", UserStatus::Active).await;

        let uc = ResolveTelegramSenderUseCase::new(links, users);
        assert_eq!(
            uc.execute("111").await.unwrap(),
            SenderResolution::Known(TelegramSender {
                user_id: "acct-1".to_string(),
                vip_level: 4,
            })
        );
    }

    #[tokio::test]
    async fn an_unbound_sender_is_unbound_not_a_tenant_of_its_own() {
        let uc = ResolveTelegramSenderUseCase::new(
            Arc::new(Links::default()),
            Arc::new(Users::default()),
        );
        assert_eq!(uc.execute("111").await.unwrap(), SenderResolution::Unbound);
    }

    #[tokio::test]
    async fn a_suspended_account_is_turned_away() {
        let (links, users) = (Arc::new(Links::default()), Arc::new(Users::default()));
        given(&links, &users, "111", "acct-1", UserStatus::Suspended).await;

        let uc = ResolveTelegramSenderUseCase::new(links, users);
        assert_eq!(
            uc.execute("111").await.unwrap(),
            SenderResolution::Suspended
        );
    }

    #[tokio::test]
    async fn an_identity_without_an_account_row_is_unbound() {
        let links = Arc::new(Links::default());
        links
            .link(
                PROVIDER_TELEGRAM,
                "111",
                &LinkedIdentity {
                    user_id: "acct-gone".to_string(),
                    email: None,
                    linked_at: 0,
                },
            )
            .await
            .unwrap();

        let uc = ResolveTelegramSenderUseCase::new(links, Arc::new(Users::default()));
        assert_eq!(uc.execute("111").await.unwrap(), SenderResolution::Unbound);
    }
}
