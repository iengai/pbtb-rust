use std::error::Error as StdError;

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
    /// A persisted row was read successfully but does not parse into a domain
    /// value (e.g. an unknown exchange, an unparseable timestamp). It is a fault,
    /// not an absence: collapsing it into `Ok(None)` would let a corrupt live bot
    /// read back as "not found". Permanent — retrying the read never repairs the
    /// data — so it fails fast rather than degrading.
    #[error("corrupt persisted record: {0}")]
    CorruptRecord(String),
    /// A technical/infra fault crossing the port boundary. `context` is the
    /// de-masked, operator-facing summary; the underlying error is kept as the
    /// `#[source]` so the chain (and its retryability detail) survives a `{e:#}`
    /// log. The user only ever sees a redacted category, never this.
    #[error("repository error: {context}")]
    Repository {
        context: String,
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
        DomainError::Repository {
            context: context.into(),
            source: Box::new(source),
        }
    }
}
