use super::{
    Deps,
    states::{BotContext, DialogueState},
    types,
};
use teloxide::dispatching::dialogue::{Dialogue, InMemStorage};
use teloxide::prelude::*;
use teloxide::types::CallbackQuery;

type MyDialogue = Dialogue<DialogueState, InMemStorage<DialogueState>>;
type MyBotContext = Dialogue<BotContext, InMemStorage<BotContext>>;

pub fn routes() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry().branch(
        Update::filter_callback_query()
            .enter_dialogue::<CallbackQuery, InMemStorage<DialogueState>, DialogueState>()
            .enter_dialogue::<CallbackQuery, InMemStorage<BotContext>, BotContext>()
            .endpoint(
                |bot: Bot,
                 q: CallbackQuery,
                 deps: Deps,
                 dialogue: MyDialogue,
                 bot_context: MyBotContext| async move {
                    handle_callback(bot, q, deps, dialogue, bot_context)
                        .await
                        .map_err(|_e| DependencyMap::new())
                },
            ),
    )
}
async fn handle_callback(
    bot: Bot,
    q: CallbackQuery,
    deps: Deps,
    dialogue: MyDialogue,
    bot_context: MyBotContext,
) -> anyhow::Result<()> {
    let data = q.data.as_ref().map(|s| s.as_str()).unwrap_or("");

    // Check if this is a bot selection callback
    if data.starts_with("select_bot:") {
        handle_bot_selection(bot, q, deps, bot_context).await?;
        return Ok(());
    }

    // Check if this is a strategy-side toggle callback
    if data.starts_with("toggle_side:") {
        handle_toggle_side(bot, q, deps, bot_context).await?;
        return Ok(());
    }

    // Check if this is a template selection callback
    if data.starts_with("select_template:") {
        handle_template_selection(bot, q, dialogue, bot_context, deps).await?;
        return Ok(());
    }

    // Check if this is a cancel template selection callback
    if data == "cancel_template_selection" {
        handle_cancel_template_selection(bot, q).await?;
        return Ok(());
    }

    bot.answer_callback_query(q.id.clone()).await.ok();

    let callback = types::CallbackData::decode(data);

    match callback {
        types::CallbackData::Action(action) => {
            handle_action(bot, q, action).await?;
        }
        types::CallbackData::Unknown => {
            // 未知的 callback
            if let Some(message) = q.message {
                bot.send_message(message.chat.id, "⚠️ Unknown action")
                    .await?;
            }
        }
    }

    Ok(())
}

async fn handle_action(
    bot: Bot,
    q: CallbackQuery,
    action: types::CallbackAction,
) -> anyhow::Result<()> {
    let message = match q.message {
        Some(msg) => msg,
        None => return Ok(()),
    };

    match action {
        types::CallbackAction::Hello => {
            bot.send_message(message.chat.id, "👋 Hello! How can I help you?")
                .await?;
        }
    }

    Ok(())
}

/// Handle bot selection callback
/// Records the selected bot_id in BotContext for later use
async fn handle_bot_selection(
    bot: Bot,
    q: CallbackQuery,
    deps: Deps,
    bot_context: MyBotContext,
) -> anyhow::Result<()> {
    if let Some(data) = &q.data {
        if data.starts_with("select_bot:") {
            let bot_id = data.strip_prefix("select_bot:").unwrap_or("").to_string();
            let user_id = q.from.id.to_string();

            // Answer callback to remove loading state
            bot.answer_callback_query(&q.id)
                .text("✅ Bot selected!")
                .await?;

            // Update bot context with selected bot_id
            bot_context
                .update(BotContext {
                    selected_bot_id: Some(bot_id.clone()),
                })
                .await?;

            let selected = deps
                .list_bots_usecase
                .execute(&user_id)
                .await
                .ok()
                .and_then(|bots| bots.into_iter().find(|b| b.id == bot_id));

            let details = if let Some(b) = selected {
                let runtime = deps
                    .get_bot_runtime_usecase
                    .execute(&user_id, &b.id)
                    .await
                    .ok()
                    .flatten();
                let status = super::views::format_runtime_phase(runtime.as_ref().map(|r| &r.phase));
                format!(
                    "🤖 Exchange: {}\n• Name: {}\n• ID: {}\n• Status: {}",
                    b.exchange.as_str().to_uppercase(),
                    b.name,
                    b.id,
                    status
                )
            } else {
                format!("🤖 Bot ID: {}", bot_id)
            };

            // Confirm to user
            if let Some(Message { chat, .. }) = q.message {
                bot.send_message(
                    chat.id,
                    format!(
                        "✅ Bot selected!\n\n{}\n\nYou can now use 'Run bot', 'Stop bot' and other operations.",
                        details
                    ),
                )
                .await?;
            }
        }
    }

    Ok(())
}

/// Handle template selection callback
async fn handle_template_selection(
    bot: Bot,
    q: CallbackQuery,
    _dialogue: MyDialogue,
    bot_context: MyBotContext,
    deps: Deps,
) -> anyhow::Result<()> {
    if let Some(data) = &q.data {
        if data.starts_with("select_template:") {
            let template_name = data.strip_prefix("select_template:").unwrap_or("");

            // Get user_id and bot_id from context
            let user_id = q.from.id.to_string();

            let ctx = bot_context.get().await?.unwrap_or_default();

            let bot_id = match ctx.selected_bot_id {
                Some(id) => id,
                None => {
                    bot.answer_callback_query(&q.id)
                        .text("❌ No bot selected")
                        .show_alert(true)
                        .await?;
                    return Ok(());
                }
            };

            // Answer callback to remove loading state
            bot.answer_callback_query(&q.id)
                .text("⏳ Applying template...")
                .await?;

            // Apply template: copy to {user_id}/{bot_id}.json and override live.user
            match deps
                .apply_template_usecase
                .execute(&user_id, &bot_id, template_name)
                .await
            {
                Ok(_) => {
                    // Success message
                    if let Some(Message { chat, .. }) = q.message {
                        bot.send_message(
                            chat.id,
                            format!(
                                "✅ Configuration template applied successfully!\n\n\
                                📄 Template: {}\n\
                                🤖 Bot ID: {}\n\
                                📁 Saved to: {}/{}.json\n\n\
                                The configuration has been customized for this bot.",
                                template_name, bot_id, user_id, bot_id
                            ),
                        )
                        .await?;
                    }
                }
                Err(e) => {
                    // Error message
                    if let Some(Message { chat, .. }) = q.message {
                        bot.send_message(
                            chat.id,
                            format!(
                                "❌ Failed to apply template\n\n\
                                Error: {}\n\n\
                                Please try again or contact support.",
                                e
                            ),
                        )
                        .await?;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Handle a strategy-side toggle callback (`toggle_side:long`/`toggle_side:short`).
/// Flips the selected bot's side via the use case and re-renders the keyboard.
async fn handle_toggle_side(
    bot: Bot,
    q: CallbackQuery,
    deps: Deps,
    bot_context: MyBotContext,
) -> anyhow::Result<()> {
    let side = q
        .data
        .as_deref()
        .and_then(|d| d.strip_prefix("toggle_side:"))
        .unwrap_or("")
        .to_string();
    let user_id = q.from.id.to_string();

    let bot_id = match bot_context.get().await?.unwrap_or_default().selected_bot_id {
        Some(id) => id,
        None => {
            bot.answer_callback_query(&q.id)
                .text("❌ No bot selected")
                .show_alert(true)
                .await?;
            return Ok(());
        }
    };

    // Read the current state and flip it.
    let current = deps
        .get_bot_config_usecase
        .execute(&user_id, &bot_id)
        .await
        .ok()
        .map(|c| c.side_enabled(&side))
        .unwrap_or(true);

    match deps
        .set_strategy_side_usecase
        .execute(&user_id, &bot_id, &side, !current)
        .await
    {
        Ok(now_enabled) => {
            bot.answer_callback_query(&q.id)
                .text(format!(
                    "{} {} — applies on next 'Run bot'",
                    side,
                    if now_enabled { "enabled" } else { "disabled" }
                ))
                .await?;

            // Re-render both toggles from the freshly saved config.
            if let Ok(cfg) = deps.get_bot_config_usecase.execute(&user_id, &bot_id).await {
                if let Some(Message { id, chat, .. }) = q.message {
                    bot.edit_message_reply_markup(chat.id, id)
                        .reply_markup(super::keyboards::strategy_sides_keyboard(
                            cfg.side_enabled("long"),
                            cfg.side_enabled("short"),
                        ))
                        .await
                        .ok();
                }
            }
        }
        Err(e) => {
            bot.answer_callback_query(&q.id)
                .text(format!("❌ {}", e))
                .show_alert(true)
                .await?;
        }
    }

    Ok(())
}

/// Handle cancel template selection callback
async fn handle_cancel_template_selection(bot: Bot, q: CallbackQuery) -> anyhow::Result<()> {
    // Answer callback
    bot.answer_callback_query(&q.id)
        .text("❌ Cancelled")
        .await?;

    // Update message
    if let Some(Message { id, chat, .. }) = q.message {
        bot.edit_message_text(chat.id, id, "❌ Template selection cancelled.")
            .await?;
    }

    Ok(())
}
