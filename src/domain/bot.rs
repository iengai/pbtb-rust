use crate::domain::engine::Runtime;
use crate::domain::error::DomainError;
use crate::domain::exchange::Exchange;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct Bot {
    pub id: String,
    pub user_id: String,
    pub exchange: Exchange,
    pub name: String,
    /// The first of the two credentials, in the exchange's meaning
    /// (`Exchange::validate_credentials`): a Bybit API key, or a Hyperliquid
    /// account address.
    pub api_key: String,
    /// The second credential: a Bybit API secret, or a Hyperliquid API wallet
    /// private key.
    pub secret_key: String,
    pub enabled: bool,
    /// Which image runs this bot's engine line (`py` passivbot, `rs` pb-runner).
    /// Read at launch only; a change applies on the next start.
    pub runtime: Runtime,
    /// The bot's public page on its exchange (a Bybit copy-trading page, a
    /// Hyperliquid vault), given by the operator with `/public`. The showcase links to it; whether the bot is on the
    /// showcase at all is `on_showcase`.
    pub public_url: Option<String>,
    /// The operator's explicit showcase choice. `None` on a row that never
    /// made one, which is on the showcase exactly when it carries a link.
    pub showcase: Option<bool>,
    pub created_at: i64, // Unix timestamp in seconds
    pub updated_at: i64, // Unix timestamp in seconds
}

impl Bot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        user_id: String,
        exchange: Exchange,
        name: String,
        api_key: String,
        secret_key: String,
        enabled: bool,
        runtime: Runtime,
        created_at: i64,
        updated_at: i64,
    ) -> Self {
        Self {
            id,
            user_id,
            exchange,
            name,
            api_key,
            secret_key,
            enabled,
            runtime,
            public_url: None,
            showcase: None,
            created_at,
            updated_at,
        }
    }

    /// A bot's id is its name, and the id is the row's sort key, where `#`
    /// marks the `<kind>#` rows kept beside bots; a name carrying it would be
    /// stored as something no reader recognises as a bot and vanish from every
    /// listing.
    pub fn validate_name(name: &str) -> Result<(), DomainError> {
        if name.is_empty() || name.contains('#') {
            return Err(DomainError::InvalidBotName(name.to_string()));
        }
        Ok(())
    }

    /// Factory encapsulating the construction policy for a newly added bot:
    /// the id is derived from the name, the bot starts disabled (desired state
    /// off) and runs on the default (Python) runtime.
    pub fn create(
        user_id: String,
        exchange: Exchange,
        name: String,
        api_key: String,
        secret_key: String,
        now: i64,
    ) -> Self {
        Self {
            id: name.clone(),
            user_id,
            exchange,
            name,
            api_key,
            secret_key,
            enabled: false,
            runtime: Runtime::default(),
            public_url: None,
            showcase: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Desired-state transition: user turned the bot on.
    pub fn enable(&mut self, now: i64) {
        self.enabled = true;
        self.updated_at = now;
    }

    /// Desired-state transition: user turned the bot off.
    pub fn disable(&mut self, now: i64) {
        self.enabled = false;
        self.updated_at = now;
    }

    /// Accept a link only when it is an https page on the exchange's site
    /// (`Exchange::public_host` or a subdomain of it). The authority
    /// is read by hand (the domain takes no URL crate) and ends at the first of
    /// `/ ? # \\`; the backslash counts because browsers read it as `/` in an
    /// https URL, so `https://evil.example\\@www.bybit.com` would otherwise
    /// pass as a bybit.com host and land on evil.example. A user-info part
    /// (`@`), a port, and any byte outside `[A-Za-z0-9.-]` in the authority
    /// are refused outright, as is whitespace or a control character anywhere.
    pub fn validate_public_url(exchange: Exchange, url: &str) -> Result<(), DomainError> {
        // The echo is what the user reads back; a value near Telegram's message
        // limit would make the reply itself undeliverable.
        let refuse = || DomainError::InvalidPublicUrl {
            host: exchange.public_host(),
            got: url.chars().take(128).collect(),
        };
        if url.len() > 512 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(refuse());
        }
        let rest = url.strip_prefix("https://").ok_or_else(refuse)?;
        let authority = rest.split(['/', '?', '#', '\\']).next().unwrap_or("");
        let plain = |b: u8| b.is_ascii_alphanumeric() || b == b'.' || b == b'-';
        if authority.is_empty() || !authority.bytes().all(plain) {
            return Err(refuse());
        }
        let host = authority.to_ascii_lowercase();
        let site = exchange.public_host();
        if host == site
            || host
                .strip_suffix(site)
                .is_some_and(|sub| sub.ends_with('.'))
        {
            Ok(())
        } else {
            Err(refuse())
        }
    }

    /// Give the bot its public link, or take it away. Validated here so a
    /// link off the bot's exchange can never reach a row.
    pub fn set_public_url(&mut self, url: Option<String>, now: i64) -> Result<(), DomainError> {
        if let Some(u) = &url {
            Self::validate_public_url(self.exchange, u)?;
        }
        self.public_url = url;
        self.updated_at = now;
        Ok(())
    }

    /// Whether the bot is meant for the public showcase page. The link and the
    /// choice are separate, so hiding a bot keeps its link and a bot without
    /// one can still be shown.
    pub fn on_showcase(&self) -> bool {
        self.showcase.unwrap_or(self.public_url.is_some())
    }

    /// Record the operator's showcase choice.
    pub fn set_showcase(&mut self, shown: bool, now: i64) {
        self.showcase = Some(shown);
        self.updated_at = now;
    }

    /// Move the bot to another runtime image. Takes effect on the next launch:
    /// a running task keeps the binary it started with until it is restarted.
    /// Refused when the image cannot trade on the bot's exchange.
    pub fn set_runtime(&mut self, runtime: Runtime, now: i64) -> Result<(), DomainError> {
        runtime.ensure_trades_on(self.exchange)?;
        self.runtime = runtime;
        self.updated_at = now;
        Ok(())
    }
}

#[async_trait]
pub trait BotRepository: Send + Sync {
    /// `Ok(None)` is a genuine absence; `Err` is a read failure. A fault must
    /// never be collapsed into `None`, so a caller can tell "no such bot" from
    /// "the read failed" (see docs/conventions.md § Error Handling).
    async fn find(&self, user_id: &str, bot_id: &str) -> Result<Option<Bot>, DomainError>;
    /// Strongly-consistent read for decisions that must not act on a stale
    /// replica — re-validating desired state (`enabled`) inside the restart lock
    /// before launching. Defaults to `find`; the DynamoDB implementation
    /// overrides it with a consistent read.
    async fn find_consistent(
        &self,
        user_id: &str,
        bot_id: &str,
    ) -> Result<Option<Bot>, DomainError> {
        self.find(user_id, bot_id).await
    }
    async fn save(&self, bot: &Bot) -> Result<(), DomainError>;
    async fn find_by_user_id(&self, user_id: &str) -> Result<Vec<Bot>, DomainError>;
    async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), DomainError>;
}

/// Domain port for persisting a bot's exchange API keys (e.g. to object
/// storage). Use cases depend on this abstraction, not the concrete infra impl.
#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    async fn save(&self, bot: &Bot) -> Result<(), DomainError>;
    async fn delete(&self, user_id: &str, bot_id: &str) -> Result<(), DomainError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_refused_when_empty_or_carrying_the_kind_separator() {
        assert!(matches!(
            Bot::validate_name("my#bot"),
            Err(DomainError::InvalidBotName(_))
        ));
        assert!(matches!(
            Bot::validate_name(""),
            Err(DomainError::InvalidBotName(_))
        ));
        assert!(Bot::validate_name("my-bot").is_ok());
    }

    #[test]
    fn create_sets_defaults() {
        let bot = Bot::create(
            "user-1".into(),
            Exchange::Bybit,
            "mybot".into(),
            "ak".into(),
            "sk".into(),
            42,
        );
        assert_eq!(bot.id, "mybot");
        assert_eq!(bot.name, "mybot");
        assert_eq!(bot.user_id, "user-1");
        assert_eq!(bot.exchange, Exchange::Bybit);
        assert!(!bot.enabled);
        assert_eq!(bot.runtime, Runtime::Py);
        assert_eq!(bot.created_at, 42);
        assert_eq!(bot.updated_at, 42);
    }

    #[test]
    fn enable_disable_transitions() {
        let mut bot = Bot::create(
            "u".into(),
            Exchange::Bybit,
            "b".into(),
            "ak".into(),
            "sk".into(),
            1,
        );
        bot.enable(100);
        assert!(bot.enabled);
        assert_eq!(bot.updated_at, 100);
        bot.disable(200);
        assert!(!bot.enabled);
        assert_eq!(bot.updated_at, 200);
    }

    #[test]
    fn a_public_link_must_be_https_on_bybit() {
        for ok in [
            "https://www.bybit.com/copyTrade/trade-center/detail?leaderMark=abc",
            "https://bybit.com",
            "https://WWW.Bybit.com/x#y",
        ] {
            assert!(
                Bot::validate_public_url(Exchange::Bybit, ok).is_ok(),
                "{ok}"
            );
        }
        for bad in [
            "https://evil.example\\@www.bybit.com",
            "https://www.bybit.com.evil.example/x",
            "https://bybit.com@evil.example",
            "http://www.bybit.com/x",
            "https://www.bybit.com:443/x",
            "https://",
            "bybit.com",
            "https://www.bybit.com/x y",
            "https://www.bybit.com/x\n",
        ] {
            assert!(
                matches!(
                    Bot::validate_public_url(Exchange::Bybit, bad),
                    Err(DomainError::InvalidPublicUrl { .. })
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_public_link_must_be_on_the_bot_s_own_exchange() {
        let vault = "https://app.hyperliquid.xyz/vaults/0x1111111111111111111111111111111111111111";
        assert!(Bot::validate_public_url(Exchange::Hyperliquid, vault).is_ok());
        assert!(Bot::validate_public_url(Exchange::Hyperliquid, "https://hyperliquid.xyz").is_ok());
        for bad in [
            "https://www.bybit.com/x",
            "https://evilhyperliquid.xyz/x",
            "https://app.hyperliquid.xyz.evil.example/x",
        ] {
            let err = Bot::validate_public_url(Exchange::Hyperliquid, bad).unwrap_err();
            assert!(err.to_string().contains("hyperliquid.xyz"), "{err}");
        }
        assert!(Bot::validate_public_url(Exchange::Bybit, vault).is_err());
        assert!(Bot::validate_public_url(Exchange::Bybit, "https://evilbybit.com").is_err());
    }

    #[test]
    fn set_public_url_validates_and_stamps_updated_at() {
        let mut bot = Bot::create(
            "u".into(),
            Exchange::Bybit,
            "b".into(),
            "ak".into(),
            "sk".into(),
            1,
        );
        assert!(
            bot.set_public_url(Some("http://www.bybit.com/x".into()), 300)
                .is_err()
        );
        assert_eq!(bot.public_url, None);
        assert_eq!(bot.updated_at, 1, "a refused value stamps nothing");
        bot.set_public_url(Some("https://www.bybit.com/x".into()), 300)
            .unwrap();
        assert_eq!(bot.public_url.as_deref(), Some("https://www.bybit.com/x"));
        assert_eq!(bot.updated_at, 300);
        bot.set_public_url(None, 400).unwrap();
        assert_eq!(bot.public_url, None);
        assert_eq!(bot.updated_at, 400);
    }

    #[test]
    fn a_bot_without_a_choice_is_shown_exactly_when_it_has_a_link() {
        let mut bot = Bot::create(
            "u".into(),
            Exchange::Bybit,
            "b".into(),
            "ak".into(),
            "sk".into(),
            1,
        );
        assert!(!bot.on_showcase());
        bot.set_public_url(Some("https://www.bybit.com/x".into()), 2)
            .unwrap();
        assert!(bot.on_showcase());
    }

    #[test]
    fn the_showcase_choice_overrides_the_link_and_keeps_it() {
        let mut bot = Bot::create(
            "u".into(),
            Exchange::Bybit,
            "b".into(),
            "ak".into(),
            "sk".into(),
            1,
        );
        bot.set_public_url(Some("https://www.bybit.com/x".into()), 2)
            .unwrap();
        bot.set_showcase(false, 300);
        assert!(!bot.on_showcase());
        assert_eq!(bot.public_url.as_deref(), Some("https://www.bybit.com/x"));
        assert_eq!(bot.updated_at, 300);

        let mut unlinked = Bot::create(
            "u".into(),
            Exchange::Bybit,
            "c".into(),
            "ak".into(),
            "sk".into(),
            1,
        );
        unlinked.set_showcase(true, 400);
        assert!(unlinked.on_showcase(), "a link is not required to be shown");
    }

    #[test]
    fn set_runtime_stamps_updated_at() {
        let mut bot = Bot::create(
            "u".into(),
            Exchange::Bybit,
            "b".into(),
            "ak".into(),
            "sk".into(),
            1,
        );
        bot.set_runtime(Runtime::Rs, 300).unwrap();
        assert_eq!(bot.runtime, Runtime::Rs);
        assert_eq!(bot.updated_at, 300);
    }

    #[test]
    fn a_runtime_that_cannot_trade_on_the_exchange_is_refused() {
        let mut bot = Bot::create(
            "u".into(),
            Exchange::Hyperliquid,
            "b".into(),
            "ak".into(),
            "sk".into(),
            1,
        );
        let err = bot.set_runtime(Runtime::Rs, 300).unwrap_err();
        assert!(matches!(err, DomainError::InvalidConfig(_)), "{err}");
        assert_eq!(bot.runtime, Runtime::Py);
        assert_eq!(bot.updated_at, 1, "a refused switch stamps nothing");
    }
}
