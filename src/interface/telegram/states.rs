use teloxide::prelude::*;

#[derive(Clone, Default)]
pub enum DialogueState {
    #[default]
    Start,
    ReceiveBotName,
    ReceiveApiKey { name: String },
    ReceiveSecretKey { name: String, api_key: String },
    ConfirmDelete { bot_id: String },
}

/// Main state that tracks selected bot (if any)
#[derive(Clone, Default)]
pub struct BotContext {
    pub selected_bot_id: Option<String>,
}