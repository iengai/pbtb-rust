use std::sync::Arc;

use crate::domain::clock::Clock;
use crate::domain::error::DomainError;
use crate::domain::identity::{IdentityRepository, LinkOutcome, LinkedIdentity, PROVIDER_WORKOS};
use crate::domain::user::{User, UserRepository, new_user_id};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignupOutcome {
    Created(User),
    /// The subject already had an account. Signing up twice is a retry, not a
    /// second account.
    Existing(User),
}

/// Turn a verified identity-provider subject into an account.
///
/// The one place an account is created. It is reached only from the web, with
/// a subject the token verifier has already checked, and it is explicit: an
/// unlinked subject that merely authenticates is still refused everywhere else,
/// so having a Google account is not the same as having an account here.
///
/// The `workos` identity written here is the account's primary identity and
/// nothing releases it; the account id itself is minted fresh and is nobody
/// else's namespace.
pub struct SignupUseCase {
    identities: Arc<dyn IdentityRepository>,
    users: Arc<dyn UserRepository>,
    clock: Arc<dyn Clock>,
}

impl SignupUseCase {
    pub fn new(
        identities: Arc<dyn IdentityRepository>,
        users: Arc<dyn UserRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            identities,
            users,
            clock,
        }
    }

    pub async fn execute(
        &self,
        subject: &str,
        email: Option<&str>,
    ) -> Result<SignupOutcome, DomainError> {
        if let Some(link) = self.identities.find_link(PROVIDER_WORKOS, subject).await? {
            return self.existing(&link.user_id).await;
        }

        let now = self.clock.now();
        let user = User::new(new_user_id(), email.map(str::to_owned), now);
        if !self.users.create_user(&user).await? {
            return Err(DomainError::CorruptRecord(format!(
                "freshly minted account id {} already exists",
                user.id
            )));
        }

        let outcome = self
            .identities
            .link(
                PROVIDER_WORKOS,
                subject,
                &LinkedIdentity {
                    user_id: user.id.clone(),
                    email: email.map(str::to_owned),
                    linked_at: now,
                },
            )
            .await?;

        match outcome {
            LinkOutcome::Linked | LinkOutcome::AlreadyLinked => Ok(SignupOutcome::Created(user)),
            LinkOutcome::ClaimedByAnother => {
                // Two signups for one subject raced and the other one won. The
                // account row written above points at nothing and is left
                // behind; it is inert, and the caller gets the account that
                // exists.
                tracing::warn!(
                    orphan = %user.id,
                    "signup lost a race for its subject; the account row it wrote is unreferenced"
                );
                match self.identities.find_link(PROVIDER_WORKOS, subject).await? {
                    Some(link) => self.existing(&link.user_id).await,
                    None => Err(DomainError::CorruptRecord(
                        "subject was claimed and then vanished".into(),
                    )),
                }
            }
        }
    }

    async fn existing(&self, user_id: &str) -> Result<SignupOutcome, DomainError> {
        self.users
            .find_user(user_id)
            .await?
            .map(SignupOutcome::Existing)
            .ok_or_else(|| {
                DomainError::CorruptRecord(format!(
                    "identity is linked to account {user_id}, which has no row"
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::user::UserStatus;
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
            let mut map = self.0.lock().unwrap();
            let key = (provider.to_string(), subject.to_string());
            Ok(match map.get(&key) {
                Some(held) if held == &identity.user_id => LinkOutcome::AlreadyLinked,
                Some(_) => LinkOutcome::ClaimedByAnother,
                None => {
                    map.insert(key, identity.user_id.clone());
                    LinkOutcome::Linked
                }
            })
        }
        async fn links_of(&self, _: &str) -> Result<Vec<(String, String)>, DomainError> {
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
            let mut map = self.0.lock().unwrap();
            if map.contains_key(&user.id) {
                return Ok(false);
            }
            map.insert(user.id.clone(), user.clone());
            Ok(true)
        }
        async fn set_vip_level(&self, _: &str, _: u8, _: i64) -> Result<bool, DomainError> {
            Ok(false)
        }
        async fn set_status(&self, _: &str, _: UserStatus, _: i64) -> Result<bool, DomainError> {
            Ok(false)
        }
    }

    fn usecase(links: Arc<Links>, users: Arc<Users>) -> SignupUseCase {
        SignupUseCase::new(links, users, Arc::new(FixedClock))
    }

    #[tokio::test]
    async fn a_new_subject_gets_a_fresh_account_at_level_zero() {
        let (links, users) = (Arc::new(Links::default()), Arc::new(Users::default()));
        let outcome = usecase(links.clone(), users.clone())
            .execute("user_01NEW", Some("new@example.com"))
            .await
            .unwrap();

        let SignupOutcome::Created(user) = outcome else {
            panic!("expected a created account, got {outcome:?}");
        };
        assert_eq!(user.vip_level, 0);
        assert!(user.is_active());
        assert_eq!(user.email.as_deref(), Some("new@example.com"));
        assert_ne!(user.id, "user_01NEW", "the account id is not the subject");
        assert_eq!(
            links
                .find_link(PROVIDER_WORKOS, "user_01NEW")
                .await
                .unwrap()
                .unwrap()
                .user_id,
            user.id
        );
        assert!(users.find_user(&user.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn signing_up_twice_returns_the_same_account() {
        let (links, users) = (Arc::new(Links::default()), Arc::new(Users::default()));
        let uc = usecase(links, users.clone());
        let SignupOutcome::Created(first) = uc.execute("user_01NEW", None).await.unwrap() else {
            panic!("first signup creates");
        };
        let SignupOutcome::Existing(second) = uc.execute("user_01NEW", None).await.unwrap() else {
            panic!("second signup finds");
        };
        assert_eq!(first.id, second.id);
        assert_eq!(users.0.lock().unwrap().len(), 1, "no second account row");
    }

    #[tokio::test]
    async fn an_identity_without_an_account_row_is_a_fault_not_a_signup() {
        let (links, users) = (Arc::new(Links::default()), Arc::new(Users::default()));
        links
            .link(
                PROVIDER_WORKOS,
                "user_01OLD",
                &LinkedIdentity {
                    user_id: "acct-gone".into(),
                    email: None,
                    linked_at: 0,
                },
            )
            .await
            .unwrap();

        let err = usecase(links, users.clone())
            .execute("user_01OLD", None)
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::CorruptRecord(_)), "{err}");
        assert!(
            users.0.lock().unwrap().is_empty(),
            "nothing was provisioned"
        );
    }
}
