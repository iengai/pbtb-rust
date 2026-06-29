use teloxide::dispatching::dialogue::{Dialogue, InMemStorage};
use teloxide::prelude::*;

use super::{
    Deps,
    states::{BotContext, DialogueState},
};
use crate::usecase::{AddOutcome, StartOutcome, StopOutcome};

type MyDialogue = Dialogue<DialogueState, InMemStorage<DialogueState>>;
type MyBotContext = Dialogue<BotContext, InMemStorage<BotContext>>;

pub fn routes() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry().branch(
        Update::filter_message()
            .enter_dialogue::<Message, InMemStorage<DialogueState>, DialogueState>()
            .enter_dialogue::<Message, InMemStorage<BotContext>, BotContext>()
            .branch(dptree::case![DialogueState::Start].endpoint(handle_start_state))
            .branch(dptree::case![DialogueState::ReceiveBotName].endpoint(receive_bot_name))
            .branch(dptree::case![DialogueState::ReceiveApiKey { name }].endpoint(receive_api_key))
            .branch(
                dptree::case![DialogueState::ReceiveSecretKey { name, api_key }]
                    .endpoint(receive_secret_key),
            )
            .branch(dptree::case![DialogueState::ConfirmDelete { bot_id }].endpoint(confirm_delete))
            .branch(
                dptree::case![DialogueState::ConfirmOverwriteBot {
                    name,
                    api_key,
                    secret_key
                }]
                .endpoint(confirm_overwrite_bot),
            )
            .branch(dptree::case![DialogueState::ReceiveRiskLevel].endpoint(receive_risk_level)),
    )
}

async fn handle_start_state(
    bot: Bot,
    dialogue: MyDialogue,
    bot_context: MyBotContext,
    msg: Message,
    deps: Deps,
) -> Result<(), DependencyMap> {
    let result = async {
        let text = match msg.text() {
            Some(t) => t,
            None => return Ok(()),
        };

        // Handle keyboard button text
        match text {
            "State" => {
                let ctx = bot_context.get().await?
                    .unwrap_or_default();

                // Check if bot is selected
                if ctx.selected_bot_id.is_none() {
                    bot.send_message(
                        msg.chat.id,
                        "📊 Bot State\n\n🤖 No bot selected\n\nPlease use 'List' to select a bot first."
                    )
                        .await?;
                    return Ok(());
                }

                let bot_id = ctx.selected_bot_id.as_ref().unwrap();
                let user_id = msg.from()
                    .map(|user| user.id.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                // Fetch the bot once and reuse it for name, exchange, and desired state.
                let (bot_name, bot_exchange, bot_enabled) = match deps.list_bots_usecase.execute(&user_id).await {
                    Ok(bots) => {
                        bots.iter()
                            .find(|b| &b.id == bot_id)
                            .map(|b| (b.name.clone(), b.exchange.as_str().to_uppercase(), b.enabled))
                            .unwrap_or_else(|| (bot_id.clone(), "UNKNOWN".to_string(), false))
                    }
                    Err(_) => (bot_id.clone(), "UNKNOWN".to_string(), false),
                };

                // Observed runtime (actual task phase), independent of desired state.
                let runtime = deps.get_bot_runtime_usecase
                    .execute(&user_id, bot_id)
                    .await
                    .ok()
                    .flatten();

                // Desired state (user intent) from Bot.enabled.
                let desired_text = if bot_enabled { "🟢 Enabled" } else { "🔴 Disabled" };

                // Actual state (observed task) from the runtime record.
                let actual_text =
                    super::views::format_runtime_phase(runtime.as_ref().map(|r| &r.phase));

                // Get bot config
                match deps.get_bot_config_usecase.execute(&user_id, bot_id).await {
                    Ok(config) => {
                        // 1. Get template name from config_data
                        let template_name = config.config_data
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&config.template_name);

                        // 1b. Strategies involved + per-side on/off state.
                        let strategy_info = super::views::format_strategies(&config.strategies());
                        let description_info = config.description().unwrap_or("—");
                        let sides_info = format!(
                            "Long {}, Short {}",
                            if config.side_enabled("long") { "🟢 on" } else { "🔴 off" },
                            if config.side_enabled("short") { "🟢 on" } else { "🔴 off" },
                        );

                        // 2. Get risk level (long and short)
                        let risk_info = match config.risk_level() {
                            Ok(risk) => format!("   • Long: {:.2}\n   • Short: {:.2}", risk.long, risk.short),
                            Err(_) => "   • Not configured".to_string(),
                        };

                        // 3. Get leverage
                        let leverage_info = match config.leverage() {
                            Ok(lev) => format!("{:.1}x", lev.long),
                            Err(_) => "Not set".to_string(),
                        };

                        // 4. Get coins (long and short)
                        let coins_info = match config.coins() {
                            Ok(coins) => {
                                let long_str = if coins.long.is_empty() {
                                    "None".to_string()
                                } else {
                                    coins.long.join(", ")
                                };
                                let short_str = if coins.short.is_empty() {
                                    "None".to_string()
                                } else {
                                    coins.short.join(", ")
                                };
                                format!("   • Long: {long_str}\n   • Short: {short_str}")
                            }
                            Err(_) => "   • Not configured".to_string(),
                        };

                        // Build complete status message
                        let status_message = format!(
                            "📊 Bot Status\n\n\
                            🤖 Bot Information:\n\
                               • Exchange: {}\n\
                               • Name: {}\n\
                               • ID: {}\n\
                               • Desired: {}\n\
                               • Actual: {}\n\n\
                            📋 Configuration:\n\
                               • Template: {}\n\
                               • Strategy: {}\n\
                               • Description: {}\n\
                               • Sides: {}\n\
                            {}\n\n\
                            ⚠️ Risk Level:\n\
                            {}\n\n\
                            📈 Leverage: {}\n\n\
                            💰 Trading Coins:\n\
                            {}",
                            bot_exchange,
                            bot_name,
                            bot_id,
                            desired_text,
                            actual_text,
                            template_name,
                            strategy_info,
                            description_info,
                            sides_info,
                            config.template_version
                                .as_ref()
                                .map(|v| format!("   • Version: {v}"))
                                .unwrap_or_else(|| "   • Version: N/A".to_string()),
                            risk_info,
                            leverage_info,
                            coins_info
                        );

                        bot.send_message(msg.chat.id, status_message)
                            .reply_markup(super::keyboards::main_menu_keyboard())
                            .await?;
                    }
                    Err(_) => {
                        // No config found
                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "📊 Bot Status\n\n\
                                🤖 Bot Information:\n\
                                   • Exchange: {bot_exchange}\n\
                                   • Name: {bot_name}\n\
                                   • ID: {bot_id}\n\
                                   • Desired: {desired_text}\n\
                                   • Actual: {actual_text}\n\n\
                                ⚠️ No configuration found for this bot.\n\n\
                                Please apply a configuration template first using 'Choose config...'."
                            )
                        )
                            .reply_markup(super::keyboards::main_menu_keyboard())
                            .await?;
                    }
                }
            }
            "Balance" => {
                bot.send_message(msg.chat.id, "💰 Balance: $0.00")
                    .reply_markup(super::keyboards::main_menu_keyboard())
                    .await?;
            }
            "Add bot" => {
                bot.send_message(msg.chat.id, "🤖 Let's add a new bot!\n\nPlease enter the bot name:")
                    .await?;
                dialogue.update(DialogueState::ReceiveBotName).await?;
            }
            "Choose config..." => {
                // Check if bot is selected
                let ctx = bot_context.get().await?
                    .unwrap_or_default();

                if ctx.selected_bot_id.is_none() {
                    bot.send_message(
                        msg.chat.id,
                        "❌ No bot selected. Please use 'List' to select a bot first."
                    )
                        .await?;
                    return Ok(());
                }

                // Get available templates
                match deps.list_templates_usecase.execute().await {
                    Ok(templates) => {
                        if templates.is_empty() {
                            bot.send_message(
                                msg.chat.id,
                                "📋 No configuration templates available.\n\n\
                                Please contact administrator to add templates."
                            )
                                .await?;
                        } else {
                            bot.send_message(
                                msg.chat.id,
                                "⚙️ Choose a configuration template:\n\n\
                                Select one of the predefined templates below to view details."
                            )
                                .reply_markup(super::keyboards::template_list_keyboard(&templates))
                                .await?;
                        }
                    }
                    Err(e) => {
                        bot.send_message(
                            msg.chat.id,
                            format!("❌ Error fetching templates: {e}")
                        )
                            .await?;
                    }
                }
            }
            "Risk level" => {
                // Check if bot is selected
                let ctx = bot_context.get().await?
                    .unwrap_or_default();

                if ctx.selected_bot_id.is_none() {
                    bot.send_message(
                        msg.chat.id,
                        "❌ No bot selected. Please use 'List' to select a bot first."
                    )
                        .await?;
                    return Ok(());
                }

                // Check if bot has config
                let user_id = msg.from()
                    .map(|user| user.id.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                let bot_id = ctx.selected_bot_id.as_ref().unwrap();

                match deps.get_bot_config_usecase.execute(&user_id, bot_id).await {
                    Ok(config) => {
                        // Get current risk level
                        let current_risk = config.risk_level()
                            .map(|r| format!("Long: {:.2}, Short: {:.2}", r.long, r.short))
                            .unwrap_or_else(|_| "Not set".to_string());

                        let current_leverage = config.leverage()
                            .map(|l| format!("{:.1}x", l.long))
                            .unwrap_or_else(|_| "Not set".to_string());

                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "⚠️ Risk Level Configuration\n\n\
                                🤖 Bot: {bot_id}\n\
                                📊 Current Risk Level: {current_risk}\n\
                                📈 Current Leverage: {current_leverage}\n\n\
                                Please enter the new risk level values in the following format:\n\
                                `<long_risk>/<short_risk>`\n\n\
                                Example: `3.0/1.5`\n\n\
                                Note:\n\
                                - Values should be decimal numbers (0.0 - 10.0)\n\
                                - Leverage will be automatically set to (max_risk + 1)\n\
                                - Send 'cancel' to abort"
                            )
                        )
                            .await?;

                        dialogue.update(DialogueState::ReceiveRiskLevel).await?;
                    }
                    Err(_) => {
                        bot.send_message(
                            msg.chat.id,
                            "❌ No configuration found for this bot.\n\n\
                            Please apply a configuration template first using 'Choose config...'."
                        )
                            .await?;
                    }
                }
            }
            "Run bot" => {
                let ctx = bot_context.get().await?
                    .unwrap_or_default();

                let text = if let Some(ref bot_id) = ctx.selected_bot_id {
                    let user_id = msg.from()
                        .map(|user| user.id.to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    match deps.start_bot_usecase.execute(&user_id, bot_id).await {
                        Ok(StartOutcome::Started { .. }) => format!("▶️ Bot {bot_id} is starting up."),
                        Ok(StartOutcome::AlreadyRunning) => format!("▶️ Bot {bot_id} is already running."),
                        Ok(StartOutcome::AlreadyStarting) => format!("⏳ Bot {bot_id} is already starting — give it a few seconds."),
                        Ok(StartOutcome::Stopping) => format!("🛑 Bot {bot_id} is still stopping — wait a few seconds, then tap Run again."),
                        Ok(StartOutcome::BotNotFound) => format!("❌ Bot {bot_id} not found."),
                        Err(e) => format!("❌ Failed to start bot {bot_id}:\n\n{e}"),
                    }
                } else {
                    "❌ Please select a bot first using 'List'".to_string()
                };

                // Re-attach the menu keyboard so the command buttons stay available.
                bot.send_message(msg.chat.id, text)
                    .reply_markup(super::keyboards::main_menu_keyboard())
                    .await?;
            }
            "Stop bot" => {
                let ctx = bot_context.get().await?
                    .unwrap_or_default();

                let text = if let Some(ref bot_id) = ctx.selected_bot_id {
                    let user_id = msg.from()
                        .map(|user| user.id.to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    match deps.stop_bot_usecase.execute(&user_id, bot_id).await {
                        Ok(StopOutcome::Stopped { .. }) => format!("🛑 Bot {bot_id} is stopping."),
                        Ok(StopOutcome::NotRunning) => format!("⏹️ Bot {bot_id} turned off. It wasn't running."),
                        Ok(StopOutcome::StartInProgress) => format!(
                            "⏳ Bot {bot_id} turned off, but it's still starting up. \
                            Tap Stop again in a few seconds."
                        ),
                        Ok(StopOutcome::AlreadyStopping) => format!("🛑 Bot {bot_id} is already stopping."),
                        Ok(StopOutcome::BotNotFound) => format!("❌ Bot {bot_id} not found."),
                        Err(e) => format!("❌ Failed to stop bot {bot_id}:\n\n{e}"),
                    }
                } else {
                    "❌ Please select a bot first using 'List'".to_string()
                };

                bot.send_message(msg.chat.id, text)
                    .reply_markup(super::keyboards::main_menu_keyboard())
                    .await?;
            }
            "Sides" => {
                let ctx = bot_context.get().await?.unwrap_or_default();

                let bot_id = match ctx.selected_bot_id.as_ref() {
                    Some(id) => id,
                    None => {
                        bot.send_message(
                            msg.chat.id,
                            "❌ No bot selected. Please use 'List' to select a bot first.",
                        )
                        .await?;
                        return Ok(());
                    }
                };

                let user_id = msg
                    .from()
                    .map(|user| user.id.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                match deps.get_bot_config_usecase.execute(&user_id, bot_id).await {
                    Ok(config) => {
                        let strategy_info = super::views::format_strategies(&config.strategies());
                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "🎛️ Strategy Sides\n\n\
                                🤖 Bot: {bot_id}\n\
                                📋 Strategy: {strategy_info}\n\n\
                                Tap a side to turn it on/off.\n\
                                Off stops opening new positions and closes \
                                existing ones gradually.\n\
                                ⚠️ Applies on the next 'Run bot'."
                            ),
                        )
                        .reply_markup(super::keyboards::strategy_sides_keyboard(
                            config.side_enabled("long"),
                            config.side_enabled("short"),
                        ))
                        .await?;
                    }
                    Err(_) => {
                        bot.send_message(
                            msg.chat.id,
                            "❌ No configuration found for this bot.\n\n\
                            Please apply a configuration template first using 'Choose config...'.",
                        )
                        .await?;
                    }
                }
            }
            "Unstuck" => {
                bot.send_message(msg.chat.id, "🔧 Unstuck operation... (Feature coming soon)")
                    .reply_markup(super::keyboards::main_menu_keyboard())
                    .await?;
            }
            "Delete API key" => {
                let ctx = bot_context.get().await?
                    .unwrap_or_default();

                if let Some(ref bot_id) = ctx.selected_bot_id {
                    bot.send_message(
                        msg.chat.id,
                        format!(
                            "⚠️ Are you sure you want to delete this bot?\n\n\
                            🤖 Bot ID: {bot_id}\n\n\
                            ❗ This action cannot be undone!\n\n\
                            Reply 'yes' to confirm or any other message to cancel."
                        )
                    )
                        .await?;

                    dialogue.update(DialogueState::ConfirmDelete {
                        bot_id: bot_id.clone()
                    }).await?;
                } else {
                    bot.send_message(
                        msg.chat.id,
                        "❌ No bot selected. Please use 'List' to select a bot first."
                    )
                        .await?;
                }
            }
            "List" => {
                // Get user_id from telegram message
                let user_id = msg.from()
                    .map(|user| user.id.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                // Call use case to get bots
                match deps.list_bots_usecase.execute(&user_id).await {
                    Ok(bots) => {
                        if bots.is_empty() {
                            bot.send_message(
                                msg.chat.id,
                                "📋 Your bots:\n\n(No bots configured yet)",
                            )
                                .await?;
                        } else {
                            let ctx = bot_context.get().await?
                                .unwrap_or_default();

                            let header = if let Some(ref bot_id) = ctx.selected_bot_id {
                                format!("📋 Select a bot:\n\n✅ Currently selected: {bot_id}")
                            } else {
                                "📋 Select a bot:\n\n(No bot selected)".to_string()
                            };

                            let augmented = super::bots_with_phase(&deps, &user_id, bots).await;
                            bot.send_message(msg.chat.id, header)
                                .reply_markup(super::keyboards::bot_list_keyboard(&augmented))
                                .await?;
                        }
                    }
                    Err(e) => {
                        bot.send_message(
                            msg.chat.id,
                            format!("❌ Error fetching bots: {e}"),
                        )
                            .await?;
                    }
                }
            }
            _ => {
                // ignore unknown text
            }
        }

        anyhow::Ok(())
    }.await;

    result.map_err(|_| DependencyMap::new())
}

async fn receive_bot_name(
    bot: Bot,
    dialogue: MyDialogue,
    _bot_context: MyBotContext,
    msg: Message,
) -> Result<(), DependencyMap> {
    let result = async {
        match msg.text() {
            Some(name) => {
                bot.send_message(
                    msg.chat.id,
                    format!("✅ Bot name: {name}\n\nNow, please enter the API key:"),
                )
                .await?;
                dialogue
                    .update(DialogueState::ReceiveApiKey {
                        name: name.to_string(),
                    })
                    .await?;
            }
            None => {
                bot.send_message(msg.chat.id, "❌ Please send text for bot name.")
                    .await?;
            }
        }
        anyhow::Ok(())
    }
    .await;

    result.map_err(|_| DependencyMap::new())
}

async fn receive_api_key(
    bot: Bot,
    dialogue: MyDialogue,
    _bot_context: MyBotContext,
    name: String,
    msg: Message,
) -> Result<(), DependencyMap> {
    let result = async {
        match msg.text() {
            Some(api_key) => {
                bot.send_message(
                    msg.chat.id,
                    "✅ API key received!\n\nFinally, please enter the secret key:",
                )
                .await?;
                dialogue
                    .update(DialogueState::ReceiveSecretKey {
                        name,
                        api_key: api_key.to_string(),
                    })
                    .await?;
            }
            None => {
                bot.send_message(msg.chat.id, "❌ Please send text for API key.")
                    .await?;
            }
        }
        anyhow::Ok(())
    }
    .await;

    result.map_err(|_| DependencyMap::new())
}

async fn receive_secret_key(
    bot: Bot,
    dialogue: MyDialogue,
    _bot_context: MyBotContext,
    (name, api_key): (String, String),
    msg: Message,
    deps: Deps,
) -> Result<(), DependencyMap> {
    let result = async {
        match msg.text() {
            Some(secret_key) => {
                let secret_key = secret_key.to_string();
                let user_id = msg
                    .from()
                    .map(|user| user.id.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                // Save bot using use case
                match deps
                    .add_bot_usecase
                    .execute(&user_id, name.clone(), api_key.clone(), secret_key.clone())
                    .await
                {
                    Ok(AddOutcome::Added(new_bot)) => {
                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "✅ Bot added successfully!\n\n\
                                📝 Name: {}\n\
                                🆔 ID: {}\n\
                                ⏸️ Status: Disabled (default)\n\n\
                                You can enable it later.",
                                new_bot.name, new_bot.id
                            ),
                        )
                        .await?;
                        dialogue.update(DialogueState::Start).await?;
                    }
                    Ok(AddOutcome::AlreadyExists(existing)) => {
                        let status = if existing.enabled {
                            "🟢 Enabled"
                        } else {
                            "🔴 Disabled"
                        };
                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "⚠️ A bot named \"{}\" already exists.\n\n\
                                🆔 ID: {}\n\
                                📊 Status: {}\n\n\
                                Adding it again will overwrite its API keys \
                                (its config and run state are kept).\n\n\
                                Reply 'yes' to overwrite, or any other message to cancel.",
                                existing.name, existing.id, status
                            ),
                        )
                        .await?;
                        dialogue
                            .update(DialogueState::ConfirmOverwriteBot {
                                name,
                                api_key,
                                secret_key,
                            })
                            .await?;
                    }
                    Err(e) => {
                        bot.send_message(msg.chat.id, format!("❌ Error saving bot: {e}"))
                            .await?;
                        dialogue.update(DialogueState::Start).await?;
                    }
                }
            }
            None => {
                bot.send_message(msg.chat.id, "❌ Please send text for secret key.")
                    .await?;
            }
        }
        anyhow::Ok(())
    }
    .await;

    result.map_err(|_| DependencyMap::new())
}

async fn confirm_delete(
    bot: Bot,
    dialogue: MyDialogue,
    bot_context: MyBotContext,
    bot_id: String,
    msg: Message,
    deps: Deps,
) -> Result<(), DependencyMap> {
    let result = async {
        match msg.text() {
            Some(text) => {
                if text.trim().eq_ignore_ascii_case("yes") {
                    // User confirmed deletion
                    let user_id = msg
                        .from()
                        .map(|user| user.id.to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    match deps.delete_bot_usecase.execute(&user_id, &bot_id).await {
                        Ok(_) => {
                            // Clear the selected bot from context
                            bot_context
                                .update(BotContext {
                                    selected_bot_id: None,
                                })
                                .await?;

                            bot.send_message(
                                msg.chat.id,
                                format!("✅ Bot deleted successfully!\n\n🤖 Bot ID: {bot_id}"),
                            )
                            .await?;
                        }
                        Err(e) => {
                            bot.send_message(msg.chat.id, format!("❌ Error deleting bot: {e}"))
                                .await?;
                        }
                    }
                } else {
                    // User cancelled
                    bot.send_message(msg.chat.id, "🚫 Deletion cancelled.")
                        .await?;
                }

                // Reset dialogue to start
                dialogue.update(DialogueState::Start).await?;
            }
            None => {
                bot.send_message(msg.chat.id, "❌ Please send text to confirm.")
                    .await?;
            }
        }
        anyhow::Ok(())
    }
    .await;

    result.map_err(|_| DependencyMap::new())
}

async fn confirm_overwrite_bot(
    bot: Bot,
    dialogue: MyDialogue,
    _bot_context: MyBotContext,
    (name, api_key, secret_key): (String, String, String),
    msg: Message,
    deps: Deps,
) -> Result<(), DependencyMap> {
    let result = async {
        match msg.text() {
            Some(text) => {
                if text.trim().eq_ignore_ascii_case("yes") {
                    let user_id = msg
                        .from()
                        .map(|user| user.id.to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    match deps
                        .add_bot_usecase
                        .overwrite(&user_id, name, api_key, secret_key)
                        .await
                    {
                        Ok(saved) => {
                            bot.send_message(
                                msg.chat.id,
                                format!(
                                    "✅ Bot overwritten successfully!\n\n\
                                    📝 Name: {}\n\
                                    🆔 ID: {}\n\n\
                                    Its API keys were updated.",
                                    saved.name, saved.id
                                ),
                            )
                            .await?;
                        }
                        Err(e) => {
                            bot.send_message(msg.chat.id, format!("❌ Error overwriting bot: {e}"))
                                .await?;
                        }
                    }
                } else {
                    bot.send_message(
                        msg.chat.id,
                        "🚫 Overwrite cancelled. The existing bot was left unchanged.",
                    )
                    .await?;
                }

                // Reset dialogue to start
                dialogue.update(DialogueState::Start).await?;
            }
            None => {
                bot.send_message(
                    msg.chat.id,
                    "❌ Please send 'yes' to confirm or any other message to cancel.",
                )
                .await?;
            }
        }
        anyhow::Ok(())
    }
    .await;

    result.map_err(|_| DependencyMap::new())
}

async fn receive_risk_level(
    bot: Bot,
    dialogue: MyDialogue,
    bot_context: MyBotContext,
    msg: Message,
    deps: Deps,
) -> Result<(), DependencyMap> {
    let result = async {
        match msg.text() {
            Some(text) => {
                // Allow cancellation
                if text.trim().eq_ignore_ascii_case("cancel") {
                    bot.send_message(msg.chat.id, "🚫 Risk level update cancelled.")
                        .await?;
                    dialogue.update(DialogueState::Start).await?;
                    return Ok(());
                }

                // Parse input: "3.0/1.5"
                let parts: Vec<&str> = text.trim().split('/').collect();

                if parts.len() != 2 {
                    bot.send_message(
                        msg.chat.id,
                        "❌ Invalid format. Please enter two numbers separated by /\n\n\
                        Example: `3.0/1.5`\n\
                        Or send 'cancel' to abort.",
                    )
                    .await?;
                    return Ok(());
                }

                // Parse risk values
                let risk_long: f64 = match parts[0].trim().parse() {
                    Ok(v) => v,
                    Err(_) => {
                        bot.send_message(
                            msg.chat.id,
                            "❌ Invalid long risk value. Please enter a decimal number.\n\n\
                            Example: `3.0/1.5`",
                        )
                        .await?;
                        return Ok(());
                    }
                };

                let risk_short: f64 = match parts[1].trim().parse() {
                    Ok(v) => v,
                    Err(_) => {
                        bot.send_message(
                            msg.chat.id,
                            "❌ Invalid short risk value. Please enter a decimal number.\n\n\
                            Example: `3.0/1.5`",
                        )
                        .await?;
                        return Ok(());
                    }
                };

                // Validate range
                if !(0.0..=10.0).contains(&risk_long) || !(0.0..=10.0).contains(&risk_short) {
                    bot.send_message(
                        msg.chat.id,
                        "❌ Risk values must be between 0.0 and 10.0.\n\n\
                        Please try again or send 'cancel'.",
                    )
                    .await?;
                    return Ok(());
                }

                // Get user_id and bot_id
                let user_id = msg
                    .from()
                    .map(|user| user.id.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                let ctx = bot_context.get().await?.unwrap_or_default();

                let bot_id = match ctx.selected_bot_id {
                    Some(id) => id,
                    None => {
                        bot.send_message(msg.chat.id, "❌ No bot selected.").await?;
                        dialogue.update(DialogueState::Start).await?;
                        return Ok(());
                    }
                };

                // Update risk level
                match deps
                    .update_risk_level_usecase
                    .execute(&user_id, &bot_id, risk_long, risk_short)
                    .await
                {
                    Ok(_) => {
                        let max_risk = risk_long.max(risk_short);
                        let leverage = max_risk + 1.0;

                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "✅ Risk level updated successfully!\n\n\
                                🤖 Bot: {bot_id}\n\
                                📊 New Risk Level:\n\
                                   • Long: {risk_long:.2}\n\
                                   • Short: {risk_short:.2}\n\
                                📈 Leverage automatically set to: {leverage:.1}x\n\n\
                                The configuration has been saved."
                            ),
                        )
                        .await?;
                    }
                    Err(e) => {
                        bot.send_message(
                            msg.chat.id,
                            format!("❌ Failed to update risk level:\n\n{e}"),
                        )
                        .await?;
                    }
                }

                // Reset dialogue to start
                dialogue.update(DialogueState::Start).await?;
            }
            None => {
                bot.send_message(msg.chat.id, "❌ Please send text with risk level values.")
                    .await?;
            }
        }
        anyhow::Ok(())
    }
    .await;

    result.map_err(|_| DependencyMap::new())
}
