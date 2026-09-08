use std::sync::Arc;

use crate::domain::clock::Clock;
use crate::domain::error::DomainError;
use crate::domain::identity::{LINK_TICKET_TTL, LinkTicket, LinkTicketRepository};
use crate::domain::secret::{random_token, token_digest};

/// Hand a user a one-time URL that will link whatever identity they sign in
/// with to their own account.
///
/// The URL is the only place the token exists in the clear. What is stored is
/// its digest, so the rows this leaves behind cannot be turned back into a
/// working link by anyone who can read the table.
pub struct IssueLinkTicketUseCase {
    tickets: Arc<dyn LinkTicketRepository>,
    clock: Arc<dyn Clock>,
    link_url: String,
}

/// The key the browser's first hop redeems under. It is not the key the
/// authorization callback redeems under, so a link URL cannot stand in for an
/// authorization it was never part of.
pub const PURPOSE_START: &str = "start";

impl IssueLinkTicketUseCase {
    pub fn new(
        tickets: Arc<dyn LinkTicketRepository>,
        clock: Arc<dyn Clock>,
        link_url: impl Into<String>,
    ) -> Self {
        Self {
            tickets,
            clock,
            link_url: link_url.into(),
        }
    }

    /// Whether linking is available at all. Without a URL to send anyone to,
    /// offering the button would only produce a dead end.
    pub fn is_configured(&self) -> bool {
        !self.link_url.trim().is_empty()
    }

    /// 🔴 `user_id` is the caller's own, established by the bot before this is
    /// reached. It travels only in the stored row — never in the URL, where the
    /// user could edit it into someone else's.
    pub async fn execute(&self, user_id: &str, chat_id: i64) -> Result<String, DomainError> {
        let token = random_token();
        let now = self.clock.now();

        self.tickets
            .issue(
                PURPOSE_START,
                &token_digest(&token),
                &LinkTicket {
                    user_id: user_id.to_string(),
                    chat_id,
                    code_verifier: None,
                },
                now,
                now + LINK_TICKET_TTL,
            )
            .await?;

        Ok(format!("{}?t={token}", self.link_url.trim_end_matches('?')))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> i64 {
            1_700_000_000
        }
    }

    #[derive(Default)]
    struct Tickets(Mutex<Vec<(String, String, LinkTicket, i64)>>);

    #[async_trait]
    impl LinkTicketRepository for Tickets {
        async fn issue(
            &self,
            purpose: &str,
            token_hash: &str,
            ticket: &LinkTicket,
            _now: i64,
            expires_at: i64,
        ) -> Result<(), DomainError> {
            self.0.lock().unwrap().push((
                purpose.to_string(),
                token_hash.to_string(),
                ticket.clone(),
                expires_at,
            ));
            Ok(())
        }

        async fn redeem(
            &self,
            _purpose: &str,
            _token_hash: &str,
            _now: i64,
        ) -> Result<Option<LinkTicket>, DomainError> {
            Ok(None)
        }
    }

    fn usecase(tickets: Arc<Tickets>) -> IssueLinkTicketUseCase {
        IssueLinkTicketUseCase::new(tickets, Arc::new(FixedClock), "https://example.test/link")
    }

    #[tokio::test]
    async fn the_url_carries_a_token_the_row_only_holds_the_digest_of() {
        let tickets = Arc::new(Tickets::default());
        let url = usecase(tickets.clone())
            .execute("u-1", 4242)
            .await
            .expect("issue");

        let token = url
            .strip_prefix("https://example.test/link?t=")
            .expect("the url carries the token");
        assert_eq!(token.len(), 64);

        let issued = tickets.0.lock().unwrap().clone();
        assert_eq!(issued.len(), 1);
        let (purpose, stored_hash, ticket, expires_at) = &issued[0];
        assert_eq!(purpose, PURPOSE_START);
        assert_eq!(stored_hash, &token_digest(token));
        assert_ne!(
            stored_hash, token,
            "storing the token itself would make a readable row a usable link"
        );
        assert_eq!(ticket.user_id, "u-1");
        assert_eq!(ticket.chat_id, 4242);
        assert!(ticket.code_verifier.is_none());
        assert_eq!(*expires_at, 1_700_000_000 + LINK_TICKET_TTL);
    }

    #[tokio::test]
    async fn two_tickets_never_share_a_token() {
        let tickets = Arc::new(Tickets::default());
        let uc = usecase(tickets.clone());
        let first = uc.execute("u-1", 1).await.expect("issue");
        let second = uc.execute("u-1", 1).await.expect("issue");
        assert_ne!(first, second);
    }

    #[test]
    fn an_unconfigured_link_url_offers_nothing() {
        let uc =
            IssueLinkTicketUseCase::new(Arc::new(Tickets::default()), Arc::new(FixedClock), "   ");
        assert!(!uc.is_configured());
    }
}
