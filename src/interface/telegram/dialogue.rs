use teloxide::prelude::*;
use teloxide::dispatching::dialogue::{InMemStorage, Dialogue};

use super::{Deps, states::{DialogueState, BotContext}};

type MyDialogue = Dialogue<DialogueState, InMemStorage<DialogueState>>;
type MyBotContext = Dialogue<BotContext, InMemStorage<BotContext>>;

pub fn routes() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry()
        .branch(
            Update::filter_message()
                .enter_dialogue::<Message, InMemStorage<DialogueState>, DialogueState>()
                .enter_dialogue::<Message, InMemStorage<BotContext>, BotContext>()
                .branch(dptree::case![DialogueState::Start].endpoint(handle_start_state))
                .branch(dptree::case![DialogueState::ReceiveBotName].endpoint(receive_bot_name))
                .branch(dptree::case![DialogueState::ReceiveApiKey { name }].endpoint(receive_api_key))
                .branch(dptree::case![DialogueState::ReceiveSecretKey { name, api_key }].endpoint(receive_secret_key))
                .branch(dptree::case![DialogueState::ConfirmDelete { bot_id }].endpoint(confirm_delete))
                .branch(dptree::case![DialogueState::ReceiveRiskLevel].endpoint(receive_risk_level))
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

                // Try to get bot info from repository
                let bot_name = match deps.list_bots_usecase.execute(&user_id).await {
                    Ok(bots) => {
                        bots.iter()
                            .find(|b| &b.id == bot_id)
                            .map(|b| b.name.clone())
                            .unwrap_or_else(|| bot_id.clone())
                    }
                    Err(_) => bot_id.clone(),
                };

                // Try to get bot enabled status
                let bot_enabled = match deps.list_bots_usecase.execute(&user_id).await {
                    Ok(bots) => {
                        bots.iter()
                            .find(|b| &b.id == bot_id)
                            .map(|b| b.enabled)
                            .unwrap_or(false)
                    }
                    Err(_) => false,
                };

                // Get bot config
                match deps.get_bot_config_usecase.execute(&user_id, bot_id).await {
                    Ok(config) => {
                        // 1. Get template name from config_data
                        let template_name = config.config_data
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&config.template_name);

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
                                format!("   • Long: {}\n   • Short: {}", long_str, short_str)
                            }
                            Err(_) => "   • Not configured".to_string(),
                        };

                        // 5. Bot running state (from bot.enabled)
                        let state_icon = if bot_enabled { "🟢" } else { "🔴" };
                        let state_text = if bot_enabled { "Running" } else { "Stopped" };

                        // Build complete status message
                        let status_message = format!(
                            "📊 Bot Status\n\n\
                            🤖 Bot Information:\n\
                               • Name: {}\n\
                               • ID: {}\n\
                               • State: {} {}\n\n\
                            📋 Configuration:\n\
                               • Template: {}\n\
                            {}\n\n\
                            ⚠️ Risk Level:\n\
                            {}\n\n\
                            📈 Leverage: {}\n\n\
                            💰 Trading Coins:\n\
                            {}",
                            bot_name,
                            bot_id,
                            state_icon,
                            state_text,
                            template_name,
                            config.template_version
                                .as_ref()
                                .map(|v| format!("   • Version: {}", v))
                                .unwrap_or_else(|| "   • Version: N/A".to_string()),
                            risk_info,
                            leverage_info,
                            coins_info
                        );

                        bot.send_message(msg.chat.id, status_message)
                            .await?;
                    }
                    Err(_) => {
                        // No config found
                        let bot_enabled_status = if bot_enabled { "🟢 Running" } else { "🔴 Stopped" };

                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "📊 Bot Status\n\n\
                                🤖 Bot Information:\n\
                                   • Name: {}\n\
                                   • ID: {}\n\
                                   • State: {}\n\n\
                                ⚠️ No configuration found for this bot.\n\n\
                                Please apply a configuration template first using 'Choose config...'.",
                                bot_name,
                                bot_id,
                                bot_enabled_status
                            )
                        )
                            .await?;
                    }
                }
            }
            "Balance" => {
                bot.send_message(msg.chat.id, "💰 Balance: $0.00")
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
                            format!("❌ Error fetching templates: {}", e)
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
                                🤖 Bot: {}\n\
                                📊 Current Risk Level: {}\n\
                                📈 Current Leverage: {}\n\n\
                                Please enter the new risk level values in the following format:\n\
                                `<long_risk>/<short_risk>`\n\n\
                                Example: `3.0/1.5`\n\n\
                                Note:\n\
                                - Values should be decimal numbers (0.0 - 10.0)\n\
                                - Leverage will be automatically set to (max_risk + 1)\n\
                                - Send 'cancel' to abort",
                                bot_id,
                                current_risk,
                                current_leverage
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

                if let Some(ref bot_id) = ctx.selected_bot_id {
                    bot.send_message(msg.chat.id, format!("▶️ Starting bot {}...", bot_id))
                        .await?;
                } else {
                    bot.send_message(msg.chat.id, "❌ Please select a bot first using 'List'")
                        .await?;
                }
            }
            "Stop bot" => {
                let ctx = bot_context.get().await?
                    .unwrap_or_default();

                if let Some(ref bot_id) = ctx.selected_bot_id {
                    bot.send_message(msg.chat.id, format!("⏹️ Stopping bot {}...", bot_id))
                        .await?;
                } else {
                    bot.send_message(msg.chat.id, "❌ Please select a bot first using 'List'")
                        .await?;
                }
            }
            "Unstuck" => {
                bot.send_message(msg.chat.id, "🔧 Unstuck operation... (Feature coming soon)")
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
                            🤖 Bot ID: {}\n\n\
                            ❗ This action cannot be undone!\n\n\
                            Reply 'yes' to confirm or any other message to cancel.",
                            bot_id
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
                                format!("📋 Select a bot:\n\n✅ Currently selected: {}", bot_id)
                            } else {
                                "📋 Select a bot:\n\n(No bot selected)".to_string()
                            };

                            bot.send_message(msg.chat.id, header)
                                .reply_markup(super::keyboards::bot_list_keyboard(&bots))
                                .await?;
                        }
                    }
                    Err(e) => {
                        bot.send_message(
                            msg.chat.id,
                            format!("❌ Error fetching bots: {}", e),
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
                bot.send_message(msg.chat.id, format!("✅ Bot name: {}\n\nNow, please enter the API key:", name))
                    .await?;
                dialogue.update(DialogueState::ReceiveApiKey {
                    name: name.to_string(),
                }).await?;
            }
            None => {
                bot.send_message(msg.chat.id, "❌ Please send text for bot name.")
                    .await?;
            }
        }
        anyhow::Ok(())
    }.await;

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
                bot.send_message(msg.chat.id, "✅ API key received!\n\nFinally, please enter the secret key:")
                    .await?;
                dialogue.update(DialogueState::ReceiveSecretKey {
                    name,
                    api_key: api_key.to_string(),
                }).await?;
            }
            None => {
                bot.send_message(msg.chat.id, "❌ Please send text for API key.")
                    .await?;
            }
        }
        anyhow::Ok(())
    }.await;

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
                let user_id = msg.from()
                    .map(|user| user.id.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                // Save bot using use case
                match deps.add_bot_usecase.execute(
                    &user_id,
                    name.clone(),
                    api_key,
                    secret_key.to_string(),
                ).await {
                    Ok(new_bot) => {
                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "✅ Bot added successfully!\n\n\
                                📝 Name: {}\n\
                                🆔 ID: {}\n\
                                ⏸️ Status: Disabled (default)\n\n\
                                You can enable it later.",
                                new_bot.name,
                                new_bot.id
                            ),
                        )
                            .await?;
                    }
                    Err(e) => {
                        bot.send_message(
                            msg.chat.id,
                            format!("❌ Error saving bot: {}", e),
                        )
                            .await?;
                    }
                }

                // Reset dialogue to start
                dialogue.update(DialogueState::Start).await?;
            }
            None => {
                bot.send_message(msg.chat.id, "❌ Please send text for secret key.")
                    .await?;
            }
        }
        anyhow::Ok(())
    }.await;

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
                    let user_id = msg.from()
                        .map(|user| user.id.to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    match deps.delete_bot_usecase.execute(&user_id, &bot_id).await {
                        Ok(_) => {
                            // Clear the selected bot from context
                            bot_context.update(BotContext {
                                selected_bot_id: None,
                            }).await?;

                            bot.send_message(
                                msg.chat.id,
                                format!("✅ Bot deleted successfully!\n\n🤖 Bot ID: {}", bot_id),
                            )
                                .await?;
                        }
                        Err(e) => {
                            bot.send_message(
                                msg.chat.id,
                                format!("❌ Error deleting bot: {}", e),
                            )
                                .await?;
                        }
                    }
                } else {
                    // User cancelled
                    bot.send_message(
                        msg.chat.id,
                        "🚫 Deletion cancelled.",
                    )
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
    }.await;

    result.map_err(|_| DependencyMap::new())
}

/// Format template list for display
fn format_template_list(templates: &[String]) -> String {
    let mut message = String::from("⚙️ Available Configuration Templates:\n\n");

    for (index, template_name) in templates.iter().enumerate() {
        message.push_str(&format!(
            "{}. 📄 {}\n",
            index + 1,
            template_name
        ));
    }

    message.push_str("\n💡 Tip: These are predefined trading bot configurations.\n");
    message.push_str("To apply a template, use the bot management interface.");

    message
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
                        Or send 'cancel' to abort."
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
                            Example: `3.0/1.5`"
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
                            Example: `3.0/1.5`"
                        )
                            .await?;
                        return Ok(());
                    }
                };

                // Validate range
                if risk_long < 0.0 || risk_long > 10.0 || risk_short < 0.0 || risk_short > 10.0 {
                    bot.send_message(
                        msg.chat.id,
                        "❌ Risk values must be between 0.0 and 10.0.\n\n\
                        Please try again or send 'cancel'."
                    )
                        .await?;
                    return Ok(());
                }

                // Get user_id and bot_id
                let user_id = msg.from()
                    .map(|user| user.id.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                let ctx = bot_context.get().await?
                    .unwrap_or_default();

                let bot_id = match ctx.selected_bot_id {
                    Some(id) => id,
                    None => {
                        bot.send_message(msg.chat.id, "❌ No bot selected.")
                            .await?;
                        dialogue.update(DialogueState::Start).await?;
                        return Ok(());
                    }
                };

                // Update risk level
                match deps.update_risk_level_usecase.execute(&user_id, &bot_id, risk_long, risk_short).await {
                    Ok(_) => {
                        let max_risk = risk_long.max(risk_short);
                        let leverage = max_risk + 1.0;

                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "✅ Risk level updated successfully!\n\n\
                                🤖 Bot: {}\n\
                                📊 New Risk Level:\n\
                                   • Long: {:.2}\n\
                                   • Short: {:.2}\n\
                                📈 Leverage automatically set to: {:.1}x\n\n\
                                The configuration has been saved.",
                                bot_id,
                                risk_long,
                                risk_short,
                                leverage
                            )
                        )
                            .await?;
                    }
                    Err(e) => {
                        bot.send_message(
                            msg.chat.id,
                            format!("❌ Failed to update risk level:\n\n{}", e)
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
    }.await;

    result.map_err(|_| DependencyMap::new())
}