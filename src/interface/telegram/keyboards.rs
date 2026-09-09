use crate::domain::engine::Runtime;
use crate::domain::entitlement;
use crate::usecase::TemplateListing;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};

pub(crate) fn main_menu_keyboard() -> KeyboardMarkup {
    KeyboardMarkup::new(vec![
        vec![
            KeyboardButton::new("/start"),
            KeyboardButton::new("State"),
            KeyboardButton::new("Balance"),
        ],
        vec![
            KeyboardButton::new("Add bot"),
            KeyboardButton::new("Choose config..."),
            KeyboardButton::new("Risk level"),
        ],
        vec![
            KeyboardButton::new("Run bot"),
            KeyboardButton::new("Stop bot"),
            KeyboardButton::new("Unstuck"),
        ],
        vec![
            KeyboardButton::new("Delete API key"),
            KeyboardButton::new("List"),
            KeyboardButton::new("Sides"),
            KeyboardButton::new("Runtime"),
        ],
    ])
    .resize_keyboard(true)
    .one_time_keyboard(false)
}

/// Inline keyboard to toggle a bot's strategy sides on/off. Each button shows
/// the current state; tapping flips it (callback `toggle_side:<side>`).
pub(crate) fn strategy_sides_keyboard(
    long_enabled: bool,
    short_enabled: bool,
) -> InlineKeyboardMarkup {
    let label = |name: &str, on: bool| format!("{}: {}", name, if on { "🟢 ON" } else { "🔴 OFF" });
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            label("Long", long_enabled),
            "toggle_side:long",
        )],
        vec![InlineKeyboardButton::callback(
            label("Short", short_enabled),
            "toggle_side:short",
        )],
    ])
}

/// Inline keyboard to pick which image a bot launches on. The bot's current
/// choice is marked; tapping the other one switches it (callback
/// `set_runtime:<py|rs>`). Both options are always offered — a line with no
/// image for the tapped runtime is refused by the use case with a message that
/// names it, which is more use than a button that is silently missing.
pub(crate) fn runtime_keyboard(current: Runtime) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(Runtime::ALL.map(|rt| {
        let mark = if rt == current { "🟢" } else { "⚪" };
        vec![InlineKeyboardButton::callback(
            format!("{mark} {} — {}", rt, rt.image_label()),
            format!("set_runtime:{rt}"),
        )]
    }))
}

/// Create inline keyboard for bot list. Each button leads with the bot's OBSERVED
/// run-state glyph (not desired) so a fresh ✅ Running reads differently from a
/// 🛑 Stopping or ⏸️ Stopped one.
pub(crate) fn bot_list_keyboard(
    bots: &[(
        crate::domain::bot::Bot,
        Option<crate::domain::runtime::RuntimePhase>,
    )],
) -> InlineKeyboardMarkup {
    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    for (bot, phase) in bots {
        let glyph = super::views::runtime_phase_glyph(phase.as_ref());
        let button_text = format!(
            "{} {} | {} | {}",
            glyph,
            bot.exchange.as_str().to_uppercase(),
            bot.name,
            bot.id
        );

        // Callback data format: "select_bot:<bot_id>"
        let callback_data = format!("select_bot:{}", bot.id);

        let button = InlineKeyboardButton::callback(button_text, callback_data);
        keyboard.push(vec![button]);
    }

    InlineKeyboardMarkup::new(keyboard)
}

/// Inline keyboard for the apply-config confirmation modal: Confirm applies the
/// previewed template (`confirm_template:<name>`); Cancel aborts.
pub(crate) fn template_confirm_keyboard(template_name: &str) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("✅ Confirm", format!("confirm_template:{template_name}")),
        InlineKeyboardButton::callback("❌ Cancel", "cancel_template_selection"),
    ]])
}

/// Inline keyboard for the template list: one button per template
/// (`select_template:<name>`). A template above `vip_level` is shown locked
/// with the level it asks for; it stays tappable so the refusal can say why.
pub(crate) fn template_list_keyboard(
    templates: &[TemplateListing],
    vip_level: u8,
) -> InlineKeyboardMarkup {
    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    for template in templates {
        let template_name = &template.name;
        let button_text = if entitlement::meets(vip_level, template.min_vip_level) {
            format!("📄 {template_name}")
        } else {
            format!("🔒 {template_name} (VIP {})", template.min_vip_level)
        };

        let callback_data = format!("select_template:{template_name}");

        let button = InlineKeyboardButton::callback(button_text, callback_data);
        keyboard.push(vec![button]);
    }

    // Add a cancel button at the end
    keyboard.push(vec![InlineKeyboardButton::callback(
        "❌ Cancel",
        "cancel_template_selection",
    )]);

    InlineKeyboardMarkup::new(keyboard)
}
