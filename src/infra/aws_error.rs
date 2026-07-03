use crate::domain::error::DomainError;
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
