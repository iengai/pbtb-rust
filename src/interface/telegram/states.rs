#[derive(Clone, Default)]
pub enum DialogueState {
    #[default]
    Start,
    ReceiveBotName,
    ReceiveApiKey {
        name: String,
    },
    ReceiveSecretKey {
        name: String,
        api_key: String,
    },
    ConfirmDelete {
        bot_id: String,
    },
    /// Awaiting yes/no after an add hit an existing bot of the same name. Holds
    /// the entered credentials so a confirmed overwrite can save without
    /// re-prompting.
    ConfirmOverwriteBot {
        name: String,
        api_key: String,
        secret_key: String,
    },
    ReceiveRiskLevel,
}

/// Main state that tracks selected bot (if any)
#[derive(Clone, Default)]
pub struct BotContext {
    pub selected_bot_id: Option<String>,
}
