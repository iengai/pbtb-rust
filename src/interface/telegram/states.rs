use teloxide::prelude::*;

#[derive(Clone, Default)]
pub enum DialogueState {
    #[default]
    Start,
    ReceiveBotName,
    ReceiveApiKey { name: String },
    ReceiveSecretKey { name: String, api_key: String },
}