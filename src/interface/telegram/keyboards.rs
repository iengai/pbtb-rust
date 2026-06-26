// Rust
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

/// Create inline keyboard for bot list. Each button leads with the bot's OBSERVED
/// run-state glyph (not desired) so a fresh ▶️ Running reads differently from a
/// 🛑 Stopping or ⏹️ Stopped one.
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

/// Create inline keyboard for template list
/// Each template is shown as a button with callback data containing template_name
pub(crate) fn template_list_keyboard(templates: &[String]) -> InlineKeyboardMarkup {
    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    for template_name in templates {
        let button_text = format!("📄 {}", template_name);

        // Callback data format: "select_template:<template_name>"
        let callback_data = format!("select_template:{}", template_name);

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
