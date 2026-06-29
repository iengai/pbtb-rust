// Rust
use crate::domain::botconfig::{BotConfig, StrategyRef};
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
    let strategies = format_strategies(&preview.strategies());
    let description = preview.description().unwrap_or("—");

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
        • Template: {template_name}\n\
        🤖 Strategy: {strategies}\n\
        📝 Description: {description}\n\n\
        ⚠️ Wallet exposure (total_wallet_exposure_limit):\n\
        {exposure}\n\n\
        💰 Preset coins:\n\
        {coins}\n\n\
        Confirm to apply, or Cancel."
    )
}
