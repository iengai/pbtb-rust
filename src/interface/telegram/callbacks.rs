
use teloxide::prelude::*;
use teloxide::types::CallbackQuery;
use teloxide::dispatching::dialogue::{InMemStorage, Dialogue};
use super::{types, views, Deps, keyboards, states::{DialogueState, BotContext}};

type MyDialogue = Dialogue<DialogueState, InMemStorage<DialogueState>>;
type MyBotContext = Dialogue<BotContext, InMemStorage<BotContext>>;

pub fn routes() -> teloxide::dispatching::UpdateHandler<DependencyMap> {
    dptree::entry()
        .branch(
            Update::filter_callback_query()
                .enter_dialogue::<CallbackQuery, InMemStorage<DialogueState>, DialogueState>()
                .enter_dialogue::<CallbackQuery, InMemStorage<BotContext>, BotContext>()
                .endpoint(|bot: Bot, q: CallbackQuery, deps: Deps, dialogue: MyDialogue, bot_context: MyBotContext| async move {
                    handle_callback(bot, q, deps, dialogue, bot_context)
                        .await
                        .map_err(|_e| DependencyMap::new())
                }),
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
        handle_bot_selection(bot, q, dialogue, bot_context).await?;
        return Ok(());
    }

    // Check if this is a template selection callback
    if data.starts_with("select_template:") {
        handle_template_selection(bot, q, dialogue, bot_context).await?;
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
    _dialogue: MyDialogue,
    bot_context: MyBotContext,
) -> anyhow::Result<()> {
    if let Some(data) = &q.data {
        if data.starts_with("select_bot:") {
            let bot_id = data.strip_prefix("select_bot:").unwrap_or("").to_string();

            // Answer callback to remove loading state
            bot.answer_callback_query(&q.id)
                .text("✅ Bot selected!")
                .await?;

            // Update bot context with selected bot_id
            bot_context.update(BotContext {
                selected_bot_id: Some(bot_id.clone()),
            }).await?;

            // Confirm to user
            if let Some(Message { chat, .. }) = q.message {
                bot.send_message(
                    chat.id,
                    format!("✅ Bot selected!\n\n🤖 Bot ID: {}\n\nYou can now use 'Run bot', 'Stop bot' and other operations.", bot_id)
                )
                    .await?;
            }
        }
    }

    Ok(())
}

/// Handle template selection callback (placeholder for now)
async fn handle_template_selection(
    bot: Bot,
    q: CallbackQuery,
    _dialogue: MyDialogue,
    _bot_context: MyBotContext,
) -> anyhow::Result<()> {
    if let Some(data) = &q.data {
        if data.starts_with("select_template:") {
            let template_name = data.strip_prefix("select_template:").unwrap_or("");

            // Answer callback to remove loading state
            bot.answer_callback_query(&q.id)
                .text("📄 Template selected!")
                .await?;

            // For now, just show the template name
            // TODO: Implement template application logic
            if let Some(Message { chat, .. }) = q.message {
                bot.send_message(
                    chat.id,
                    format!(
                        "✅ Template selected: {}\n\n\
                        📝 Template Name: {}\n\n\
                        🚧 Applying template functionality coming soon...",
                        template_name,
                        template_name
                    )
                )
                    .await?;
            }
        }
    }

    Ok(())
}

/// Handle cancel template selection callback
async fn handle_cancel_template_selection(
    bot: Bot,
    q: CallbackQuery,
) -> anyhow::Result<()> {
    // Answer callback
    bot.answer_callback_query(&q.id)
        .text("❌ Cancelled")
        .await?;

    // Update message
    if let Some(Message { id, chat, .. }) = q.message {
        bot.edit_message_text(
            chat.id,
            id,
            "❌ Template selection cancelled."
        )
            .await?;
    }

    Ok(())
}