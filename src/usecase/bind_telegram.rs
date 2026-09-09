use std::sync::Arc;

use crate::domain::clock::Clock;
use crate::domain::error::DomainError;
use crate::domain::identity::{
    IdentityRepository, LINK_TICKET_TTL, LinkOutcome, LinkTicket, LinkTicketRepository,
    LinkedIdentity, PROVIDER_TELEGRAM,
};
use crate::domain::secret::{random_token, token_digest};

/// The key a Telegram bind ticket is redeemed under. Its own key, so a ticket
/// minted for the browser's link flow cannot be presented to the bot as a bind,
/// nor the other way round.
pub const PURPOSE_TELEGRAM_BIND: &str = "telegram-bind";

/// Mint the one-time token the web hands a signed-in user to bind a Telegram
/// account to theirs.
///
/// 🔴 `user_id` is the token's, established by the bearer check before this is
/// reached. It travels only in the stored row: the token the user carries into
/// Telegram names nothing, so nothing the bot later receives can choose the
/// account.
pub struct IssueTelegramBindTicketUseCase {
    tickets: Arc<dyn LinkTicketRepository>,
    clock: Arc<dyn Clock>,
}

impl IssueTelegramBindTicketUseCase {
    pub fn new(tickets: Arc<dyn LinkTicketRepository>, clock: Arc<dyn Clock>) -> Self {
        Self { tickets, clock }
    }

    /// The token, in the clear, once. The row holds only its digest.
    pub async fn execute(&self, user_id: &str) -> Result<String, DomainError> {
        let token = random_token();
        let now = self.clock.now();
        self.tickets
            .issue(
                PURPOSE_TELEGRAM_BIND,
                &token_digest(&token),
                &LinkTicket {
                    user_id: user_id.to_string(),
                    chat_id: 0,
                    code_verifier: None,
                },
                now,
                now + LINK_TICKET_TTL,
            )
            .await?;
        Ok(token)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindOutcome {
    Bound {
        user_id: String,
    },
    /// This Telegram id already belongs to the ticket's account. A retry of a
    /// bind that already went through, not a failure.
    AlreadyBound {
        user_id: String,
    },
    /// Never issued, expired, or already spent.
    InvalidTicket,
    /// This Telegram id belongs to some other account, which keeps it. Taking
    /// it over would hand that account's bots to whoever holds the ticket.
    TelegramTakenByAnother,
    /// The account already has a different Telegram id bound. One each: the
    /// bot resolves a sender to exactly one account, and an account speaks
    /// through one sender, so a rebind starts with an unbind.
    ///
    /// The subject side (one Telegram id, one account) is a conditional write;
    /// this side is a read before the write, so two tickets for the same
    /// account redeemed at the same instant from two Telegram ids can both
    /// bind. Only the account holder can arrange that, both ids then resolve
    /// to their own account, and `/unlink` releases both — so it is left as a
    /// check rather than made a lock.
    AccountAlreadyBound,
}

/// Redeem a bind ticket on behalf of the Telegram sender who presented it.
pub struct BindTelegramUseCase {
    tickets: Arc<dyn LinkTicketRepository>,
    identities: Arc<dyn IdentityRepository>,
    clock: Arc<dyn Clock>,
}

impl BindTelegramUseCase {
    pub fn new(
        tickets: Arc<dyn LinkTicketRepository>,
        identities: Arc<dyn IdentityRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            tickets,
            identities,
            clock,
        }
    }

    pub async fn execute(
        &self,
        token: &str,
        telegram_id: &str,
    ) -> Result<BindOutcome, DomainError> {
        let now = self.clock.now();
        let Some(ticket) = self
            .tickets
            .redeem(PURPOSE_TELEGRAM_BIND, &token_digest(token), now)
            .await?
        else {
            return Ok(BindOutcome::InvalidTicket);
        };

        // The ticket is spent by now whatever happens below. A refusal costs the
        // user a fresh ticket from the web, which is the cheap direction; a
        // ticket that survived a refusal would be a standing invitation.
        let held = self.identities.links_of(&ticket.user_id).await?;
        if held
            .iter()
            .any(|(provider, subject)| provider == PROVIDER_TELEGRAM && subject != telegram_id)
        {
            return Ok(BindOutcome::AccountAlreadyBound);
        }

        let outcome = self
            .identities
            .link(
                PROVIDER_TELEGRAM,
                telegram_id,
                &LinkedIdentity {
                    user_id: ticket.user_id.clone(),
                    email: None,
                    linked_at: now,
                },
            )
            .await?;

        Ok(match outcome {
            LinkOutcome::Linked => BindOutcome::Bound {
                user_id: ticket.user_id,
            },
            LinkOutcome::AlreadyLinked => BindOutcome::AlreadyBound {
                user_id: ticket.user_id,
            },
            LinkOutcome::ClaimedByAnother => BindOutcome::TelegramTakenByAnother,
        })
    }
}

/// Release the Telegram id an account speaks through.
///
/// Scoped to the caller's own account by the repository's condition, so this
/// is a way to stop being reachable and to make room for another Telegram id,
/// never a way to detach someone else's. The `workos` identity has no such
/// operation: it is the account.
pub struct UnbindTelegramUseCase {
    identities: Arc<dyn IdentityRepository>,
}

impl UnbindTelegramUseCase {
    pub fn new(identities: Arc<dyn IdentityRepository>) -> Self {
        Self { identities }
    }

    /// Every `telegram` identity the account holds is released — one, unless
    /// two binds raced. How many were.
    pub async fn execute(&self, user_id: &str) -> Result<usize, DomainError> {
        let mut released = 0;
        for (provider, subject) in self.identities.links_of(user_id).await? {
            if provider == PROVIDER_TELEGRAM
                && self.identities.unlink(&provider, &subject, user_id).await?
            {
                released += 1;
            }
        }
        Ok(released)
    }
}
