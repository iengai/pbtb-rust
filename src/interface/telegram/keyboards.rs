// Rust
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};

use super::types;

pub(crate) fn main_menu_keyboard() -> KeyboardMarkup {
    KeyboardMarkup::new(vec![
        vec![KeyboardButton::new("/start"), KeyboardButton::new("State"), KeyboardButton::new("Balance")],
        vec![KeyboardButton::new("Add bot"), KeyboardButton::new("Choose config..."), KeyboardButton::new("Risk level")],
        vec![KeyboardButton::new("Run bot"), KeyboardButton::new("Stop bot"), KeyboardButton::new("Unstuck")],
        vec![KeyboardButton::new("Delete API key"), KeyboardButton::new("List")],
    ])
        .resize_keyboard(true)
        .one_time_keyboard(false)
}

/// Create inline keyboard for bot list
/// Each bot is shown as a button with callback data containing bot_id
pub(crate) fn bot_list_keyboard(bots: &[crate::domain::bot::Bot]) -> InlineKeyboardMarkup {
    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    for bot in bots {
        let status = if bot.enabled { "✅" } else { "⏸️" };
        let button_text = format!("{} {}", status, bot.name);

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
    keyboard.push(vec![
        InlineKeyboardButton::callback("❌ Cancel", "cancel_template_selection")
    ]);

    InlineKeyboardMarkup::new(keyboard)
}

