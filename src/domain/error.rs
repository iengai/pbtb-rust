use std::error::Error as StdError;

/// Whether retrying the failed operation could plausibly succeed.
///
/// This is the axis that drives real decisions and the one that cuts across
/// every layer: a throttle or a timeout is worth another attempt, and worth
/// telling the user so, while a missing IAM grant or a malformed row will fail
/// identically forever and has to fail loud instead of quietly degrading.
///
/// `Permanent` is the default wherever retryability cannot be established.
/// Misclassifying a transient fault as permanent only over-redacts; the reverse
/// promises the user a retry that can never work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retryability {
    Transient,
    Permanent,
}

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("risk level {value} out of range [{min}, {max}]")]
    RiskOutOfRange { value: f64, min: f64, max: f64 },
    #[error("leverage {value} out of range [{min}, {max}]")]
    LeverageOutOfRange { value: f64, min: f64, max: f64 },
    #[error("missing config path: {0}")]
    MissingConfigPath(&'static str),
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    /// A bot's name is its id and the row's sort key, where `#` marks the
    /// `<kind>#` rows kept beside bots; a name carrying it would be stored as
    /// something no reader recognises as a bot.
    #[error("bot name {0:?} must be non-empty and must not contain '#'")]
    InvalidBotName(String),
    /// A persisted row was read successfully but does not parse into a domain
    /// value (e.g. an unknown exchange, an unparseable timestamp). It is a fault,
    /// not an absence: collapsing it into `Ok(None)` would let a corrupt live bot
    /// read back as "not found". Permanent — retrying the read never repairs the
    /// data — so it fails fast rather than degrading.
    #[error("corrupt persisted record: {0}")]
    CorruptRecord(String),
    /// A technical/infra fault crossing the port boundary. `context` is the
    /// de-masked, operator-facing summary; the underlying error is kept as the
    /// `#[source]` so the chain survives a `{e:#}` log. The user only ever sees a
    /// redacted category, never this.
    ///
    /// `retry` is classified by infra, which is the only layer that can read the
    /// SDK's error code, and consumed by whoever owns the policy: the Telegram
    /// edge renders it as a retry affordance or not.
    #[error("repository error: {context}")]
    Repository {
        context: String,
        retry: Retryability,
        #[source]
        source: Box<dyn StdError + Send + Sync>,
    },
}

impl DomainError {
    /// Wrap an infra fault as [`DomainError::Repository`], preserving the
    /// underlying error as the `#[source]`. Ports are domain-owned, so infra maps
    /// its own error type into this at the trait boundary rather than leaking it.
    pub fn repository<E>(context: impl Into<String>, source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self::repository_with(context, Retryability::Permanent, source)
    }

    /// As [`DomainError::repository`], for a fault infra was able to classify.
    pub fn repository_with<E>(context: impl Into<String>, retry: Retryability, source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        DomainError::Repository {
            context: context.into(),
            retry,
            source: Box::new(source),
        }
    }

    /// Whether retrying the operation that produced this error could help.
    ///
    /// Every variant other than an infra fault is permanent by construction: a
    /// value out of range and a row that does not parse fail the same way on
    /// every attempt.
    pub fn retryability(&self) -> Retryability {
        match self {
            DomainError::Repository { retry, .. } => *retry,
            _ => Retryability::Permanent,
        }
    }
}
