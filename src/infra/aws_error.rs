use crate::domain::error::{DomainError, Retryability};
use aws_sdk_dynamodb::error::{ProvideErrorMetadata, SdkError};
use std::error::Error as StdError;

/// Walk an error's `source()` chain into a single `": "`-joined string. AWS
/// `SdkError`'s own `Display` masks the modeled service error — a missing IAM
/// permission reads only as "service error" — so walking the chain is what
/// surfaces the real code and message the masked top-level `Display` hides.
pub(crate) fn error_chain(err: &(dyn StdError)) -> String {
    let mut out = err.to_string();
    let mut source = err.source();
    while let Some(inner) = source {
        out.push_str(": ");
        out.push_str(&inner.to_string());
        source = inner.source();
    }
    out
}

/// Map an infra error (an AWS `SdkError`, a serialization failure, …) into
/// [`DomainError::Repository`]: the de-masked chain becomes the operator-facing
/// `context`, the error itself is kept as the `#[source]` so a `{e:#}` log and
/// any retryability downcast still reach the real cause.
pub(crate) fn repo_err<E>(context: &str, err: E) -> DomainError
where
    E: StdError + Send + Sync + 'static,
{
    let detail = error_chain(&err);
    DomainError::repository(format!("{context}: {detail}"), err)
}

/// Service error codes, across DynamoDB, S3 and ECS, whose only sensible answer
/// is to try again later.
///
/// One list rather than one per service: the codes are disjoint, and a shared
/// list is what keeps a service added later from silently defaulting every
/// throttle to permanent. By the time one of these reaches a port boundary the
/// SDK has already exhausted its own retries, so what is left is a policy
/// decision -- redeliver the event, or offer the user a retry.
const TRANSIENT_CODES: &[&str] = &[
    "ThrottlingException",
    "Throttling",
    "ThrottledException",
    "ProvisionedThroughputExceededException",
    "RequestLimitExceeded",
    "TooManyRequestsException",
    "TransactionInProgressException",
    "RequestTimeout",
    "RequestTimeoutException",
    "InternalServerError",
    "InternalError",
    "InternalFailure",
    "ServiceUnavailable",
    "ServiceUnavailableException",
    "ServerException",
    "SlowDown",
];

/// Map an AWS SDK error into [`DomainError::Repository`] with its retryability.
///
/// Separate from [`repo_err`] because only a call that actually reached the
/// network can be classified: a serialization or parse failure is permanent by
/// construction, and giving it the same entry point would invite it to be
/// guessed at.
pub(crate) fn sdk_err<E, R>(context: &str, err: SdkError<E, R>) -> DomainError
where
    SdkError<E, R>: StdError + Send + Sync + 'static,
    E: ProvideErrorMetadata,
{
    let retry = sdk_retryability(&err);
    let detail = error_chain(&err);
    DomainError::repository_with(format!("{context}: {detail}"), retry, err)
}

fn sdk_retryability<E, R>(err: &SdkError<E, R>) -> Retryability
where
    E: ProvideErrorMetadata,
{
    match err {
        // The request never got an answer. Nothing about the call itself is
        // known to be wrong.
        SdkError::TimeoutError(_) | SdkError::DispatchFailure(_) => Retryability::Transient,
        // A modeled service error: only its own code can say.
        _ => code_retryability(err.code()),
    }
}

/// Classify a service error code. Split out from [`sdk_retryability`] because
/// this half is the part worth testing: building a real `SdkError` needs a
/// synthetic HTTP response, while the list is what actually goes stale.
///
/// Everything unlisted is permanent, which is where AccessDeniedException,
/// ValidationException and ConditionalCheckFailedException land -- the faults
/// that must fail loud rather than invite a retry that cannot work.
fn code_retryability(code: Option<&str>) -> Retryability {
    match code {
        Some(c) if TRANSIENT_CODES.contains(&c) => Retryability::Transient,
        _ => Retryability::Permanent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttles_and_service_faults_are_transient() {
        for code in [
            "ThrottlingException",
            "ProvisionedThroughputExceededException",
            "RequestLimitExceeded",
            "SlowDown",
            "InternalServerError",
            "ServiceUnavailable",
        ] {
            assert_eq!(
                code_retryability(Some(code)),
                Retryability::Transient,
                "{code} should be retryable"
            );
        }
    }

    #[test]
    fn permission_and_validation_faults_are_permanent() {
        // Retrying one of these never repairs it, so the user must not be told
        // to try again -- a missing IAM grant has to surface as a real fault.
        for code in [
            "AccessDeniedException",
            "AccessDenied",
            "ValidationException",
            "ConditionalCheckFailedException",
            "ResourceNotFoundException",
            "ExpiredTokenException",
        ] {
            assert_eq!(
                code_retryability(Some(code)),
                Retryability::Permanent,
                "{code} must not invite a retry"
            );
        }
    }

    #[test]
    fn an_unknown_or_absent_code_is_permanent() {
        assert_eq!(code_retryability(None), Retryability::Permanent);
        assert_eq!(
            code_retryability(Some("SomethingAddedNextYear")),
            Retryability::Permanent
        );
    }

    #[test]
    fn a_repo_err_carries_its_source_and_is_permanent() {
        let err = repo_err(
            "Failed to parse bot config JSON",
            std::io::Error::other("bad json"),
        );
        assert_eq!(err.retryability(), Retryability::Permanent);
        assert!(
            std::error::Error::source(&err).is_some(),
            "the underlying error stays reachable for a {{e:#}} log"
        );
    }
}
