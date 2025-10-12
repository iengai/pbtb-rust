// Rust
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};

use super::types;

pub(crate) fn main_menu_keyboard() -> KeyboardMarkup {
    KeyboardMarkup::new(vec![
        vec![KeyboardButton::new("/start"), KeyboardButton::new("State"), KeyboardButton::new("Balance")],
        vec![KeyboardButton::new("Add bot"), KeyboardButton::new("Choose config..."), KeyboardButton::new("Risk level")],
        vec![KeyboardButton::new("Run bot"), KeyboardButton::new("Stop bot"), KeyboardButton::new("Unstuck")],
        vec![KeyboardButton::new("Delete API key"), KeyboardButton::new("List")],
        vec![KeyboardButton::new("/help")],
    ])
        .resize_keyboard(true)
        .one_time_keyboard(false)
}
