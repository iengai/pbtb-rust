// Rust
use crate::domain::botconfig::{BotConfig, StrategyRef};
use crate::domain::engine::Runtime;
use crate::domain::runtime::RuntimePhase;

pub fn welcome_text() -> String {
    "Welcome! Use the menu below to get started.".to_owned()
}

/// Render the OBSERVED run state (not desired) with an icon + label. `None` means
/// no runtime record yet. Single source of truth so every view stays consistent.
pub fn format_runtime_phase(phase: Option<&RuntimePhase>) -> &'static str {
    match phase {
        Some(RuntimePhase::Starting) => "⏳ Starting",
        Some(RuntimePhase::Running) => "✅ Running",
        Some(RuntimePhase::Stopping) => "🛑 Stopping",
        Some(RuntimePhase::Stopped) => "⏸️ Stopped",
        None => "❔ Unknown",
    }
}

/// Render which image a bot launches on: `py — passivbot (Python)`.
pub fn format_bot_runtime(runtime: Runtime) -> String {
    format!("{runtime} — {}", runtime.image_label())
}

/// Compact observed-state glyph for list buttons.
pub fn runtime_phase_glyph(phase: Option<&RuntimePhase>) -> &'static str {
    match phase {
        Some(RuntimePhase::Starting) => "⏳",
        Some(RuntimePhase::Running) => "✅",
        Some(RuntimePhase::Stopping) => "🛑",
        Some(RuntimePhase::Stopped) => "⏸️",
        None => "❔",
    }
}

/// Name a template for a reader: its title with the id that addresses it, or
/// the bare id when the template carries no title.
pub fn format_template_label(id: &str, title: Option<&str>) -> String {
    match title {
        Some(title) => format!("{title} ({id})"),
        None => id.to_owned(),
    }
}

/// Render the strategies involved in a config for display. Groups sides by
/// strategy name (preserving first-seen order), so a strategy active on both
/// sides shows once as `name (long+short)`. Returns `—` when there are none.
pub fn format_strategies(strategies: &[StrategyRef]) -> String {
    if strategies.is_empty() {
        return "—".to_owned();
    }

    let mut order: Vec<&str> = Vec::new();
    let mut sides: Vec<(&str, bool, bool)> = Vec::new(); // (name, has_long, has_short)
    for s in strategies {
        let is_long = s.side == "long";
        let is_short = s.side == "short";
        if let Some(entry) = sides.iter_mut().find(|(n, _, _)| *n == s.name) {
            entry.1 |= is_long;
            entry.2 |= is_short;
        } else {
            order.push(&s.name);
            sides.push((&s.name, is_long, is_short));
        }
    }

    order
        .iter()
        .map(|name| {
            let (_, has_long, has_short) = sides.iter().find(|(n, _, _)| n == name).unwrap();
            let label = match (has_long, has_short) {
                (true, true) => "long+short",
                (true, false) => "long",
                (false, true) => "short",
                (false, false) => "",
            };
            if label.is_empty() {
                (*name).to_owned()
            } else {
                format!("{name} ({label})")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Render the confirmation modal for applying a config template: strategy +
/// notes, the wallet-exposure (`total_wallet_exposure_limit`) per side — the
/// number that actually governs leverage — and the preset coins per side.
pub fn format_template_confirm(template_name: &str, preview: &BotConfig) -> String {
    let template = format_template_label(template_name, preview.title());
    let strategies = format_strategies(&preview.strategies());
    let description = preview.description().unwrap_or("—");
    // Exchange whose data the strategy was tuned on (pbtb.exchange) — may
    // differ from the exchange this bot trades on, which is worth seeing
    // before confirming. Legacy templates don't carry it; omit the line.
    let data_source = preview
        .data_exchange()
        .map(|e| format!("🏦 Tuned on: {e} data\n"))
        .unwrap_or_default();
    // The passivbot engine line this config will run on (config_version major).
    // The launch is routed by it, so it is worth seeing before confirming.
    let engine = preview
        .engine_version()
        .map(|e| format!("🧠 Engine: passivbot {e} line\n"))
        .unwrap_or_default();
    // The template's strategy family and the lab iteration that produced it;
    // a template published without either reads without the line.
    let family = match (preview.style(), preview.generation()) {
        (Some(style), Some(generation)) => format!("🧬 Style: {style} · generation {generation}\n"),
        (Some(style), None) => format!("🧬 Style: {style}\n"),
        (None, Some(generation)) => format!("🧬 Generation: {generation}\n"),
        (None, None) => String::new(),
    };

    let exposure = match preview.risk_level() {
        Ok(r) => format!("   • Long: {:.2}\n   • Short: {:.2}", r.long, r.short),
        Err(_) => "   • Not configured".to_owned(),
    };

    let join_coins = |coins: &[String]| {
        if coins.is_empty() {
            "None".to_owned()
        } else {
            coins.join(", ")
        }
    };
    let coins = match preview.coins() {
        Ok(c) => format!(
            "   • Long: {}\n   • Short: {}",
            join_coins(&c.long),
            join_coins(&c.short)
        ),
        Err(_) => "   • Not configured".to_owned(),
    };

    format!(
        "📄 Apply this config?\n\n\
        • Template: {template}\n\
        {data_source}{engine}{family}🤖 Strategy: {strategies}\n\
        📝 Description: {description}\n\n\
        ⚠️ Wallet exposure (total_wallet_exposure_limit):\n\
        {exposure}\n\n\
        💰 Preset coins:\n\
        {coins}\n\n\
        Confirm to apply, or Cancel."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(pairs: &[(&str, &str)]) -> Vec<StrategyRef> {
        pairs
            .iter()
            .map(|(name, side)| StrategyRef {
                name: (*name).to_string(),
                side: (*side).to_string(),
            })
            .collect()
    }

    #[test]
    fn a_template_label_carries_the_title_and_the_id() {
        assert_eq!(
            format_template_label("tpl-bzwt9jn2", Some("10-coin basket · Balanced · $1k")),
            "10-coin basket · Balanced · $1k (tpl-bzwt9jn2)"
        );
    }

    #[test]
    fn a_template_without_a_title_is_named_by_its_id_alone() {
        assert_eq!(format_template_label("xrp-cus", None), "xrp-cus");
    }

    #[test]
    fn no_strategies_render_as_a_dash() {
        assert_eq!(format_strategies(&[]), "—");
    }

    #[test]
    fn a_strategy_on_both_sides_is_named_once() {
        assert_eq!(
            format_strategies(&refs(&[("grid", "long"), ("grid", "short")])),
            "grid (long+short)"
        );
    }

    #[test]
    fn strategies_keep_the_order_they_first_appear_in() {
        assert_eq!(
            format_strategies(&refs(&[
                ("beta", "short"),
                ("alpha", "long"),
                ("beta", "long"),
            ])),
            "beta (long+short), alpha (long)"
        );
    }

    #[test]
    fn a_side_that_is_neither_long_nor_short_gets_no_label() {
        // Configs are user data: a side this crate does not know must render as
        // the bare name rather than claim a side it never had.
        assert_eq!(format_strategies(&refs(&[("odd", "sideways")])), "odd");
    }

    #[test]
    fn every_observed_phase_has_its_own_glyph_and_label() {
        let phases = [
            RuntimePhase::Starting,
            RuntimePhase::Running,
            RuntimePhase::Stopping,
            RuntimePhase::Stopped,
        ];
        let labels: Vec<&str> = phases
            .iter()
            .map(|p| format_runtime_phase(Some(p)))
            .collect();
        let glyphs: Vec<&str> = phases
            .iter()
            .map(|p| runtime_phase_glyph(Some(p)))
            .collect();

        // Two phases sharing a rendering would make a wind-down look like a
        // start, which is the pair a reader most needs to tell apart.
        for set in [&labels, &glyphs] {
            let mut seen = set.clone();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), set.len(), "two phases render alike: {set:?}");
        }
        assert_eq!(format_runtime_phase(None), "❔ Unknown");
        assert_eq!(runtime_phase_glyph(None), "❔");
    }

    #[test]
    fn a_runtime_renders_with_the_image_it_stands_for() {
        assert_eq!(format_bot_runtime(Runtime::Py), "py — passivbot (Python)");
        assert_eq!(format_bot_runtime(Runtime::Rs), "rs — pb-runner (Rust)");
    }
}
