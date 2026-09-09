//! What a VIP level entitles an account to.
//!
//! Levels are a bare ordinal (`0..=MAX_VIP_LEVEL`, higher is more) and this
//! module is the whole table that turns one into a concrete allowance. The
//! table lives in code rather than in a row so every surface — bot, API, MCP —
//! answers the same question the same way, and so a change to it is a reviewed
//! change.

use crate::domain::user::MAX_VIP_LEVEL;

/// How many bots an account may have switched on at once (desired state, the
/// one the user controls), or `None` for no ceiling.
///
/// Level 0 runs one bot; each level above it adds one; the top level is
/// unlimited. Desired state is what is counted rather than the observed task
/// phase: the ceiling is on what the user asks for, and the reconcile Lambda
/// restarts only bots that are already within it.
pub fn max_running_bots(level: u8) -> Option<usize> {
    if level >= MAX_VIP_LEVEL {
        None
    } else {
        Some(usize::from(level) + 1)
    }
}

/// Whether an account at `level` may use a template that asks for `required`.
pub fn meets(level: u8, required: u8) -> bool {
    level >= required
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_zero_runs_one_bot_and_each_level_adds_one() {
        assert_eq!(max_running_bots(0), Some(1));
        assert_eq!(max_running_bots(1), Some(2));
        assert_eq!(max_running_bots(8), Some(9));
    }

    #[test]
    fn the_top_level_is_unlimited() {
        assert_eq!(max_running_bots(MAX_VIP_LEVEL), None);
        assert_eq!(max_running_bots(u8::MAX), None);
    }

    #[test]
    fn a_template_gate_is_met_at_or_above_its_level() {
        assert!(meets(0, 0));
        assert!(meets(3, 3));
        assert!(meets(9, 3));
        assert!(!meets(2, 3));
    }
}
