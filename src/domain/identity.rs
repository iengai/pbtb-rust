use crate::domain::error::DomainError;
use async_trait::async_trait;

/// An external identity that has been linked to a tenant.
///
/// The link is what turns a token's subject into a `user_id`. It exists only
/// because someone deliberately created it: an unlinked subject is refused
/// rather than given a tenant of its own, so authenticating with the identity
/// provider is never, by itself, enough to get an account here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedIdentity {
    pub user_id: String,
    pub email: Option<String>,
    pub linked_at: i64,
}

/// What became of an attempt to link an identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkOutcome {
    Linked,
    /// Already linked to this same tenant. Re-linking is how someone recovers
    /// from a flow that died after the write, so it must not look like a
    /// failure.
    AlreadyLinked,
    /// Linked to a different tenant, and left that way.
    ///
    /// The asymmetry is deliberate: one tenant may hold several identities, but
    /// an identity names exactly one tenant. Letting a second tenant claim an
    /// identity would make linking a way to take over someone else's bots.
    ClaimedByAnother,
}

/// A one-time ticket that carries a Telegram user's identity into the browser.
///
/// The browser leg of a link cannot be trusted to say who it is, so the claim is
/// established before the browser is ever involved: the bot mints a ticket for
/// the user it already authenticated, and the ticket is what the callback
/// resolves back to a tenant. Nothing the browser sends names a tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkTicket {
    pub user_id: String,
    /// Where to tell the user how it went.
    pub chat_id: i64,
    /// The PKCE verifier, on the ticket that survives the redirect to the
    /// authorization server. The first ticket has none: it exists before there
    /// is an authorization to bind to.
    pub code_verifier: Option<String>,
}

#[async_trait]
pub trait IdentityRepository: Send + Sync {
    /// The tenant `subject` was linked to, or `None` when it was never linked.
    ///
    /// `provider` and `subject` together name the identity: a subject is only
    /// unique within the provider that issued it.
    async fn find_link(
        &self,
        provider: &str,
        subject: &str,
    ) -> Result<Option<LinkedIdentity>, DomainError>;

    /// Link `subject` to `user_id`, refusing to move an identity that already
    /// belongs to someone else.
    async fn link(
        &self,
        provider: &str,
        subject: &str,
        identity: &LinkedIdentity,
    ) -> Result<LinkOutcome, DomainError>;

    /// The identities `user_id` holds, as `(provider, subject)` pairs.
    async fn links_of(&self, user_id: &str) -> Result<Vec<(String, String)>, DomainError>;

    /// Release an identity, and only one this tenant actually holds.
    ///
    /// Without this a link is permanent: an identity bound to the wrong tenant —
    /// the wrong account signed in, or someone else's link followed — can never
    /// be claimed by its owner, who gets a refusal forever. Scoped to the
    /// tenant's own so releasing is not a way to take an identity off someone.
    async fn unlink(
        &self,
        provider: &str,
        subject: &str,
        user_id: &str,
    ) -> Result<bool, DomainError>;
}

/// How long a user has to finish a link. Long enough to sign in with a password
/// manager and a second factor, short enough that a link left in a chat is not a
/// standing invitation.
pub const LINK_TICKET_TTL: i64 = 600;

#[async_trait]
pub trait LinkTicketRepository: Send + Sync {
    /// Mint a ticket redeemable until `expires_at`.
    ///
    /// `token_hash` is stored, never the token itself: the row is at rest in a
    /// table several roles can read, and a readable ticket is a usable one.
    /// `purpose` separates the two legs of a link. A ticket minted for the
    /// browser's first hop must not be redeemable as an authorization it was
    /// never part of, so the two live under different keys rather than trusting
    /// a caller to ask for the right one.
    async fn issue(
        &self,
        purpose: &str,
        token_hash: &str,
        ticket: &LinkTicket,
        now: i64,
        expires_at: i64,
    ) -> Result<(), DomainError>;

    /// Redeem a ticket, or `None` if it never existed, has expired, or has
    /// already been redeemed.
    ///
    /// One redemption, ever. A ticket that could be replayed would let anyone
    /// who saw the URL — in a browser history, a proxy log, a forwarded message
    /// — link their own identity to someone else's tenant.
    async fn redeem(
        &self,
        purpose: &str,
        token_hash: &str,
        now: i64,
    ) -> Result<Option<LinkTicket>, DomainError>;
}
