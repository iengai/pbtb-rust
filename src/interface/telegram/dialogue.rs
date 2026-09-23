use teloxide::dispatching::dialogue::{Dialogue, InMemStorage};
use teloxide::prelude::*;

use crate::domain::exchange::Exchange;
use crate::usecase::TelegramSender;

use super::{
    Deps,
    redaction::redact,
    states::{BotContext, DialogueState},
};
use crate::usecase::{AddOutcome, RestartOutcome, StartOutcome, StopOutcome};

type MyDialogue = Dialogue<DialogueState, InMemStorage<DialogueState>>;
type MyBotContext = Dialogue<BotContext, InMemStorage<BotContext>>;

pub fn routes() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry().branch(
        Update::filter_message()
            .enter_dialogue::<Message, InMemStorage<DialogueState>, DialogueState>()
            .enter_dialogue::<Message, InMemStorage<BotContext>, BotContext>()
            .branch(dptree::case![DialogueState::Start].endpoint(handle_start_state))
            .branch(dptree::case![DialogueState::ReceiveBotName].endpoint(receive_bot_name))
            .branch(
                dptree::case![DialogueState::ReceiveExchange { name }].endpoint(receive_exchange),
            )
            .branch(
                dptree::case![DialogueState::ReceiveApiKey { name, exchange }]
                    .endpoint(receive_api_key),
            )
            .branch(
                dptree::case![DialogueState::ReceiveSecretKey {
                    name,
                    exchange,
                    api_key
                }]
                .endpoint(receive_secret_key),
            )
            .branch(dptree::case![DialogueState::ConfirmDelete { bot_id }].endpoint(confirm_delete))
            .branch(
                dptree::case![DialogueState::ConfirmOverwriteBot {
                    name,
                    exchange,
                    api_key,
                    secret_key
                }]
                .endpoint(confirm_overwrite_bot),
            ),
    )
}

async fn handle_start_state(
    bot: Bot,
    dialogue: MyDialogue,
    bot_context: MyBotContext,
    msg: Message,
    deps: Deps,
    sender: TelegramSender,
) -> Result<(), DependencyMap> {
    let result = super::with_deadline("handle_start_state", async {
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
                let user_id = sender.user_id.clone();

                // Fetch the bot once and reuse it for name, exchange, desired state and runtime.
                let (bot_name, bot_exchange, bot_enabled, bot_runtime) = match deps.list_bots_usecase.execute(&user_id).await {
                    Ok(bots) => {
                        bots.iter()
                            .find(|b| &b.id == bot_id)
                            .map(|b| (b.name.clone(), b.exchange.as_str().to_uppercase(), b.enabled, Some(b.runtime)))
                            .unwrap_or_else(|| (bot_id.clone(), "UNKNOWN".to_string(), false, None))
                    }
                    Err(_) => (bot_id.clone(), "UNKNOWN".to_string(), false, None),
                };
                // Which image the next launch uses (py passivbot / rs pb-runner).
                let runtime_text = bot_runtime
                    .map(super::views::format_bot_runtime)
                    .unwrap_or_else(|| "—".to_string());

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
                        let template_name = super::views::format_template_label(
                            config.strategy_name().unwrap_or(&config.template_name),
                            config.title(),
                        );

                        // 1b. Strategies involved + per-side on/off state.
                        let strategy_info = super::views::format_strategies(&config.strategies());
                        let description_info = config.description().unwrap_or("—");
                        // Exchange the strategy was tuned on (pbtb.exchange),
                        // the bot's own: a config for another never launches.
                        let tuned_on = config
                            .data_exchange()
                            .map(|e| format!("\n   • Tuned on: {e} data"))
                            .unwrap_or_default();
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
                               • Actual: {}\n\
                               • Runtime: {}\n\n\
                            📋 Configuration:\n\
                               • Template: {}{}\n\
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
                            runtime_text,
                            template_name,
                            tuned_on,
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
                                   • Actual: {actual_text}\n\
                                   • Runtime: {runtime_text}\n\n\
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

                // A bot is offered the templates of its own exchange: one made
                // for another could not be applied to it.
                let exchange = match ctx.selected_bot_id.as_deref() {
                    Some(bot_id) => match super::find_own_bot(&deps, &sender.user_id, bot_id).await {
                        Ok(Some(b)) => b.exchange,
                        Ok(None) => {
                            bot.send_message(msg.chat.id, format!("❌ Bot {bot_id} not found."))
                                .await?;
                            return Ok(());
                        }
                        Err(e) => {
                            bot.send_message(msg.chat.id, redact("fetching bots", &e))
                                .await?;
                            return Ok(());
                        }
                    },
                    None => return Ok(()),
                };

                // Get available templates
                match deps.list_templates_usecase.execute(sender.role).await {
                    Ok(mut templates) => {
                        templates.retain(|t| t.exchange == exchange);
                        if templates.is_empty() {
                            bot.send_message(
                                msg.chat.id,
                                format!(
                                    "📋 No configuration templates available for {}.\n\n\
                                    Please contact administrator to add templates.",
                                    exchange.label()
                                )
                            )
                                .await?;
                        } else {
                            bot.send_message(
                                msg.chat.id,
                                "⚙️ Choose a configuration template:\n\n\
                                Select one of the predefined templates below to view details. \
                                🔒 marks the ones above your VIP level."
                            )
                                .reply_markup(super::keyboards::template_list_keyboard(&templates, sender.vip_level))
                                .await?;
                        }
                    }
                    Err(e) => {
                        bot.send_message(msg.chat.id, redact("fetching templates", &e))
                            .await?;
                    }
                }
            }
            "Run bot" => {
                let ctx = bot_context.get().await?
                    .unwrap_or_default();

                let text = if let Some(ref bot_id) = ctx.selected_bot_id {
                    let user_id = sender.user_id.clone();

                    match deps.start_bot_usecase.execute(&user_id, sender.vip_level, bot_id).await {
                        Ok(StartOutcome::Started { .. }) => format!("▶️ Bot {bot_id} is starting up."),
                        Ok(StartOutcome::AlreadyRunning) => format!("▶️ Bot {bot_id} is already running."),
                        Ok(StartOutcome::AlreadyStarting) => format!("⏳ Bot {bot_id} is already starting — give it a few seconds."),
                        Ok(StartOutcome::Stopping) => format!("🛑 Bot {bot_id} is still stopping — wait a few seconds, then tap Run again."),
                        Ok(StartOutcome::BotNotFound) => format!("❌ Bot {bot_id} not found."),
                        Err(e) => redact("starting the bot", &e),
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
                    let user_id = sender.user_id.clone();

                    match deps.stop_bot_usecase.execute(&user_id, bot_id).await {
                        Ok(StopOutcome::Stopped { .. }) => format!("🛑 Bot {bot_id} is stopping."),
                        Ok(StopOutcome::NotRunning) => format!("⏹️ Bot {bot_id} turned off. It wasn't running."),
                        Ok(StopOutcome::StartInProgress) => format!(
                            "⏳ Bot {bot_id} turned off, but it's still starting up. \
                            Tap Stop again in a few seconds."
                        ),
                        Ok(StopOutcome::AlreadyStopping) => format!("🛑 Bot {bot_id} is already stopping."),
                        Ok(StopOutcome::BotNotFound) => format!("❌ Bot {bot_id} not found."),
                        Err(e) => redact("stopping the bot", &e),
                    }
                } else {
                    "❌ Please select a bot first using 'List'".to_string()
                };

                bot.send_message(msg.chat.id, text)
                    .reply_markup(super::keyboards::main_menu_keyboard())
                    .await?;
            }
            "Restart bot" => {
                let ctx = bot_context.get().await?
                    .unwrap_or_default();

                let text = if let Some(ref bot_id) = ctx.selected_bot_id {
                    let user_id = sender.user_id.clone();

                    match deps.restart_bot_usecase.execute(&user_id, sender.vip_level, bot_id).await {
                        Ok(RestartOutcome::Restarting { .. }) => format!(
                            "🔁 Bot {bot_id} is restarting: the task stops and comes back \
                            with the current config."
                        ),
                        Ok(RestartOutcome::Started { .. }) => format!("▶️ Bot {bot_id} wasn't running — starting it up."),
                        Ok(RestartOutcome::StartInProgress) => format!(
                            "⏳ Bot {bot_id} is starting — tap Restart again in a few seconds."
                        ),
                        Ok(RestartOutcome::Stopping) => format!(
                            "🛑 Bot {bot_id} is stopping. After a Restart it comes back by itself; \
                            after a Stop, tap Run once it has stopped."
                        ),
                        Ok(RestartOutcome::BotNotFound) => format!("❌ Bot {bot_id} not found."),
                        Err(e) => redact("restarting the bot", &e),
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

                let user_id = sender.user_id.clone();

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
                                ⚠️ Applies on the next 'Run bot' or 'Restart bot'."
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
            "Runtime" => {
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

                let user_id = sender.user_id.clone();

                // The panel needs only the bot row, not its config: a bot with no
                // config applied still has a runtime, and picking one before the
                // first template is legitimate.
                match super::find_own_bot(&deps, &user_id, bot_id).await {
                    Ok(Some(b)) => {
                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "⚙️ Runtime\n\n\
                                🤖 Bot: {bot_id}\n\
                                📦 Current: {}\n\n\
                                Tap an image to switch to it.\n\
                                Both run the same engine line — only the binary differs.\n\
                                ⚠️ Applies on the next 'Run bot' or 'Restart bot'. A running task keeps \
                                its current image until it is stopped and started again.",
                                super::views::format_bot_runtime(b.runtime)
                            ),
                        )
                        .reply_markup(super::keyboards::runtime_keyboard(b.runtime))
                        .await?;
                    }
                    Ok(None) => {
                        bot.send_message(msg.chat.id, format!("❌ Bot {bot_id} not found."))
                            .await?;
                    }
                    Err(e) => {
                        bot.send_message(msg.chat.id, redact("fetching bots", &e))
                            .await?;
                    }
                }
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
                let user_id = sender.user_id.clone();

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
                        bot.send_message(msg.chat.id, redact("fetching bots", &e))
                            .await?;
                    }
                }
            }
            _ => {
                // ignore unknown text
            }
        }

        anyhow::Ok(())
    })
    .await;

    result.map_err(|_| DependencyMap::new())
}

async fn receive_bot_name(
    bot: Bot,
    dialogue: MyDialogue,
    _bot_context: MyBotContext,
    msg: Message,
) -> Result<(), DependencyMap> {
    let result = super::with_deadline("receive_bot_name", async {
        match msg.text() {
            Some(name) => {
                bot.send_message(
                    msg.chat.id,
                    format!("✅ Bot name: {name}\n\nWhich exchange does it trade on?"),
                )
                .reply_markup(super::keyboards::exchange_keyboard())
                .await?;
                dialogue
                    .update(DialogueState::ReceiveExchange {
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
    })
    .await;

    result.map_err(|_| DependencyMap::new())
}

/// What the two credentials are called on an exchange, for the prompts.
fn credential_names(exchange: Exchange) -> (&'static str, &'static str) {
    match exchange {
        Exchange::Bybit => ("API key", "secret key"),
        Exchange::Hyperliquid => ("account address", "API wallet private key"),
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The prompt for the first credential.
fn first_credential_prompt(exchange: Exchange) -> &'static str {
    match exchange {
        Exchange::Bybit => "Now, please enter the API key:",
        Exchange::Hyperliquid => {
            "Now, please enter the Hyperliquid account address (0x…): the wallet that \
            holds the funds."
        }
    }
}

/// The prompt for the second credential.
fn second_credential_prompt(exchange: Exchange) -> &'static str {
    match exchange {
        Exchange::Bybit => {
            "Finally, please enter the secret key. The message is deleted once it is read."
        }
        Exchange::Hyperliquid => {
            "Finally, please enter the private key of an API wallet approved for that \
            account (app.hyperliquid.xyz → More → API). Never the account's own key: an \
            API wallet can trade but cannot withdraw. Give each bot a wallet of its own. \
            The message is deleted once it is read."
        }
    }
}

async fn receive_exchange(
    bot: Bot,
    dialogue: MyDialogue,
    _bot_context: MyBotContext,
    name: String,
    msg: Message,
) -> Result<(), DependencyMap> {
    let result = super::with_deadline("receive_exchange", async {
        match msg.text().and_then(Exchange::from_str) {
            Some(exchange) => {
                bot.send_message(
                    msg.chat.id,
                    format!(
                        "✅ Exchange: {}\n\n{}",
                        exchange.label(),
                        first_credential_prompt(exchange)
                    ),
                )
                .reply_markup(super::keyboards::main_menu_keyboard())
                .await?;
                dialogue
                    .update(DialogueState::ReceiveApiKey { name, exchange })
                    .await?;
            }
            None => {
                bot.send_message(msg.chat.id, "❌ Please pick one of the exchanges below.")
                    .reply_markup(super::keyboards::exchange_keyboard())
                    .await?;
            }
        }
        anyhow::Ok(())
    })
    .await;

    result.map_err(|_| DependencyMap::new())
}

async fn receive_api_key(
    bot: Bot,
    dialogue: MyDialogue,
    _bot_context: MyBotContext,
    (name, exchange): (String, Exchange),
    msg: Message,
) -> Result<(), DependencyMap> {
    let result = super::with_deadline("receive_api_key", async {
        let (first, _) = credential_names(exchange);
        match msg.text() {
            Some(api_key) => {
                bot.send_message(
                    msg.chat.id,
                    format!(
                        "✅ {} received!\n\n{}",
                        capitalize(first),
                        second_credential_prompt(exchange)
                    ),
                )
                .await?;
                dialogue
                    .update(DialogueState::ReceiveSecretKey {
                        name,
                        exchange,
                        api_key: api_key.to_string(),
                    })
                    .await?;
            }
            None => {
                bot.send_message(msg.chat.id, format!("❌ Please send text for the {first}."))
                    .await?;
            }
        }
        anyhow::Ok(())
    })
    .await;

    result.map_err(|_| DependencyMap::new())
}

async fn receive_secret_key(
    bot: Bot,
    dialogue: MyDialogue,
    _bot_context: MyBotContext,
    (name, exchange, api_key): (String, Exchange, String),
    msg: Message,
    deps: Deps,
    sender: TelegramSender,
) -> Result<(), DependencyMap> {
    let result = super::with_deadline("receive_secret_key", async {
        match msg.text() {
            Some(secret_key) => {
                let secret_key = secret_key.to_string();
                // The secret has been read; it should not sit in the chat
                // history. Best effort: a failed delete leaves it to the user.
                if let Err(e) = bot.delete_message(msg.chat.id, msg.id).await {
                    tracing::info!("could not delete the credential message: {e}");
                }
                let user_id = sender.user_id.clone();

                // Save bot using use case
                match deps
                    .add_bot_usecase
                    .execute(
                        &user_id,
                        exchange,
                        name.clone(),
                        api_key.clone(),
                        secret_key.clone(),
                    )
                    .await
                {
                    Ok(AddOutcome::Added(new_bot)) => {
                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "✅ Bot added successfully!\n\n\
                                📝 Name: {}\n\
                                🆔 ID: {}\n\
                                🏦 Exchange: {}\n\
                                ⏸️ Status: Disabled (default)\n\n\
                                You can enable it later.",
                                new_bot.name,
                                new_bot.id,
                                new_bot.exchange.label()
                            ),
                        )
                        .await?;
                        dialogue.update(DialogueState::Start).await?;
                    }
                    Ok(AddOutcome::AlreadyExists(existing)) if existing.exchange != exchange => {
                        // An overwrite keeps the exchange, so offering one here
                        // would promise what the confirm then refuses.
                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "⚠️ A bot named \"{}\" already exists on {}. A bot's exchange \
                                is fixed: delete it and add it again, or pick another name.",
                                existing.name,
                                existing.exchange.label()
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
                                exchange,
                                api_key,
                                secret_key,
                            })
                            .await?;
                    }
                    Err(e) => {
                        bot.send_message(msg.chat.id, redact("saving the bot", &e))
                            .await?;
                        dialogue.update(DialogueState::Start).await?;
                    }
                }
            }
            None => {
                let (_, second) = credential_names(exchange);
                bot.send_message(
                    msg.chat.id,
                    format!("❌ Please send text for the {second}."),
                )
                .await?;
            }
        }
        anyhow::Ok(())
    })
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
    sender: TelegramSender,
) -> Result<(), DependencyMap> {
    let result = super::with_deadline("confirm_delete", async {
        match msg.text() {
            Some(text) => {
                if text.trim().eq_ignore_ascii_case("yes") {
                    // User confirmed deletion
                    let user_id = sender.user_id.clone();

                    match deps
                        .delete_bot_usecase
                        .execute(&user_id, &bot_id, &bot_id)
                        .await
                    {
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
                            bot.send_message(msg.chat.id, redact("deleting the bot", &e))
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
    })
    .await;

    result.map_err(|_| DependencyMap::new())
}

async fn confirm_overwrite_bot(
    bot: Bot,
    dialogue: MyDialogue,
    _bot_context: MyBotContext,
    (name, exchange, api_key, secret_key): (String, Exchange, String, String),
    msg: Message,
    deps: Deps,
    sender: TelegramSender,
) -> Result<(), DependencyMap> {
    let result = super::with_deadline("confirm_overwrite_bot", async {
        match msg.text() {
            Some(text) => {
                if text.trim().eq_ignore_ascii_case("yes") {
                    let user_id = sender.user_id.clone();

                    match deps
                        .add_bot_usecase
                        .overwrite(&user_id, exchange, name, api_key, secret_key)
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
                            bot.send_message(msg.chat.id, redact("overwriting the bot", &e))
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
    })
    .await;

    result.map_err(|_| DependencyMap::new())
}
