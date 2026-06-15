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
}
