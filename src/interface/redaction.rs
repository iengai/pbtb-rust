use crate::domain::error::{DomainError, Retryability};
use uuid::Uuid;

/// Redact a use-case [`DomainError`] into user-facing copy at the interface
/// edge: hide the *cause*, keep the *consequence* (docs/conventions.md § Error
/// Handling).
///
/// Shared by every adapter. The policy is the same wherever a fault leaves the
/// process — an MCP tool result reaches a model and its transcript, so it is
/// owed the same closed category set and the same correlation id as a chat
/// reply.
///
/// The user-facing contract is a small, closed, stable category set keyed on
/// what the user does, not why it failed:
/// - `Validation` — the user's own domain; echoed with specifics so they can fix
///   their input (out-of-range risk, a missing config path).
/// - `Entitlement` — what the account's level does not allow; echoed with the
///   ceiling or the level asked for, since the user can act on either (stop a
///   bot, or know what a higher level unlocks).
/// - `Transient` — an infra fault infra classified as worth another attempt; the
///   user is told to retry, and still gets a correlation id.
/// - `Internal` — every other technical/data fault collapses to one opaque line
///   plus a correlation id; the full chain is logged only under that id.
///
/// `NotFound` and `Conflict` never travel as `Err` — they are expected
/// outcome-enum branches the caller renders directly.
///
/// Both fault categories log identically and identically hide the cause. They
/// differ only in what the user is told to do, which is the whole point of the
/// axis: a throttle is worth retrying, a missing IAM grant never is.
///
/// `action` is a gerund phrase ("starting the bot") spliced into the copy.
pub fn redact(action: &str, err: &DomainError) -> String {
    match err {
        DomainError::RiskOutOfRange { .. }
        | DomainError::LeverageOutOfRange { .. }
        | DomainError::MissingConfigPath(_)
        | DomainError::InvalidConfig(_)
        | DomainError::InvalidBotName(_) => format!("⚠️ {err}"),
        DomainError::QuotaExceeded { .. } | DomainError::InsufficientLevel { .. } => {
            format!("🔒 {err}")
        }
        DomainError::CorruptRecord(_) | DomainError::Repository { .. } => {
            let ref_id = Uuid::new_v4().simple().to_string();
            let ref_short = &ref_id[..8];
            // The only place the real cause is surfaced — the full source chain,
            // tagged with the id the user is shown, lives in the operator log.
            tracing::error!(ref_id = ref_short, "{action} failed: {}", error_chain(err));
            match err.retryability() {
                Retryability::Transient => format!(
                    "⏳ Temporarily unavailable while {action}. Please try again in a moment (ref: {ref_short})."
                ),
                Retryability::Permanent => format!(
                    "❌ Something went wrong while {action}. It's been logged (ref: {ref_short})."
                ),
            }
        }
    }
}

/// Walk an error's `source()` chain into one `": "`-joined operator string.
fn error_chain(err: &dyn std::error::Error) -> String {
    let mut out = err.to_string();
    let mut source = err.source();
    while let Some(inner) = source {
        out.push_str(": ");
        out.push_str(&inner.to_string());
        source = inner.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_error_is_echoed_with_specifics() {
        let err = DomainError::RiskOutOfRange {
            value: 11.0,
            min: 0.0,
            max: 10.0,
        };
        let msg = redact("updating the risk level", &err);
        assert!(msg.contains("out of range"), "constraint is shown: {msg}");
        assert!(!msg.contains("ref:"), "validation needs no correlation id");
    }

    #[test]
    fn entitlement_refusals_say_what_the_level_allows() {
        let msg = redact("starting the bot", &DomainError::QuotaExceeded { limit: 1 });
        assert!(msg.contains("1 running bot"), "the ceiling is shown: {msg}");
        assert!(!msg.contains("ref:"));

        let msg = redact(
            "applying the template",
            &DomainError::InsufficientLevel {
                required: 3,
                current: 0,
            },
        );
        assert!(
            msg.contains("VIP 3") && msg.contains("VIP 0"),
            "both levels are shown: {msg}"
        );
    }

    #[test]
    fn transient_fault_invites_a_retry_and_still_hides_the_cause() {
        let err = DomainError::repository_with(
            "DynamoDB get_item failed: ThrottlingException",
            Retryability::Transient,
            std::io::Error::other("throttled"),
        );
        let msg = redact("loading the bot", &err);
        assert!(
            msg.contains("try again"),
            "the user is told to retry: {msg}"
        );
        assert!(
            msg.contains("ref:"),
            "a correlation id is still shown: {msg}"
        );
        assert!(
            !msg.contains("DynamoDB") && !msg.contains("Throttling"),
            "the cause never leaks to the user: {msg}"
        );
    }

    #[test]
    fn permanent_fault_offers_no_retry() {
        let err = DomainError::repository(
            "DynamoDB get_item failed: AccessDenied",
            std::io::Error::other("denied"),
        );
        assert!(
            !redact("loading the bot", &err).contains("try again"),
            "a permanent fault must not promise a retry that cannot work"
        );
    }

    #[test]
    fn internal_error_is_redacted_to_a_ref_without_detail() {
        let err = DomainError::repository(
            "DynamoDB get_item failed: AccessDenied",
            std::io::Error::other("boom"),
        );
        let msg = redact("loading the bot", &err);
        assert!(msg.contains("ref:"), "a correlation id is shown: {msg}");
        assert!(
            !msg.contains("DynamoDB") && !msg.contains("AccessDenied") && !msg.contains("boom"),
            "the cause never leaks to the user: {msg}"
        );
    }
}
