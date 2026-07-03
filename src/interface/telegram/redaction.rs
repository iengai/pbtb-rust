use crate::domain::error::DomainError;
use uuid::Uuid;

/// Redact a use-case [`DomainError`] into user-facing Telegram copy at the
/// interface edge: hide the *cause*, keep the *consequence*
/// (docs/conventions.md § Error Handling).
///
/// The user-facing contract is a small, closed, stable category set keyed on
/// what the user does, not why it failed:
/// - `Validation` — the user's own domain; echoed with specifics so they can fix
///   their input (out-of-range risk, a missing config path).
/// - `Internal` — every technical/data fault collapses to one opaque line plus a
///   correlation id; the full chain is logged only under that id, never shown.
///
/// `NotFound` and `Conflict` never travel as `Err` — they are expected
/// outcome-enum branches the caller renders directly. `Transient` is folded into
/// `Internal` until repository faults carry typed retryability; this only ever
/// over-redacts (no false "retry" promise), never leaks.
///
/// `action` is a gerund phrase ("starting the bot") spliced into the copy.
pub fn redact(action: &str, err: &DomainError) -> String {
    match err {
        DomainError::RiskOutOfRange { .. }
        | DomainError::LeverageOutOfRange { .. }
        | DomainError::MissingConfigPath(_)
        | DomainError::InvalidConfig(_) => format!("⚠️ {err}"),
        DomainError::CorruptRecord(_) | DomainError::Repository { .. } => {
            let ref_id = Uuid::new_v4().simple().to_string();
            let ref_short = &ref_id[..8];
            // The only place the real cause is surfaced — the full source chain,
            // tagged with the id the user is shown, lives in the operator log.
            tracing::error!(ref_id = ref_short, "{action} failed: {}", error_chain(err));
            format!("❌ Something went wrong while {action}. It's been logged (ref: {ref_short}).")
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
