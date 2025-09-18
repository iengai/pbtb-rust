// Rust
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

use super::types;

pub fn main_menu() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback(
                "👋 Hello",
                types::CallbackData::Action(types::CallbackAction::Hello).encode(),
            ),
        ],
    ])
}