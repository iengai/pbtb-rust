//! How a bot, a template and a config are rendered to a caller.
//!
//! Shared by the REST and the MCP adapters, so a client that reads a bot over
//! one surface and over the other meets one shape — and so the rule these
//! functions carry cannot hold on one surface while lapsing on the other: a
//! config is *described*, never dumped. The strategy's own parameters are what
//! make it worth running, and a caller who can read them can run it anywhere.

use serde_json::{Value, json};

use crate::domain::bot::Bot;
use crate::domain::botconfig::BotConfig;
use crate::usecase::{TemplateListing, TemplatePreview};

/// A bot as every listing shows it. No `api_key` or `secret_key`: they are
/// never part of a response.
pub(crate) fn bot(bot: &Bot, phase: Option<String>) -> Value {
    json!({
        "bot_id": bot.id,
        "name": bot.name,
        "exchange": bot.exchange.as_str(),
        "enabled": bot.enabled,
        "runtime": bot.runtime.as_str(),
        "phase": phase,
        "created_at": bot.created_at,
        "updated_at": bot.updated_at,
    })
}

/// The operational view of a bot's config — what telebot's State screen
/// prints. Risk, leverage and coins are the fields the user set or can set;
/// the strategy's own parameters are not among them.
pub(crate) fn config(config: &BotConfig) -> Value {
    json!({
        "template_name": config.strategy_name().unwrap_or(&config.template_name),
        "title": config.title(),
        "title_zh": config.title_zh(),
        "template_version": config.template_version,
        "description": config.description(),
        "tuned_on": config.data_exchange(),
        "config_version": config.config_data.get("config_version"),
        "strategies": config
            .strategies()
            .iter()
            .map(|s| json!({ "name": s.name, "side": s.side }))
            .collect::<Vec<_>>(),
        "sides": {
            "long": config.side_enabled("long"),
            "short": config.side_enabled("short"),
        },
        "risk": config
            .risk_level()
            .ok()
            .map(|r| json!({ "long": r.long, "short": r.short })),
        "leverage": config.leverage().ok().map(|l| l.long),
        "coins": config
            .coins()
            .ok()
            .map(|c| json!({ "long": c.long, "short": c.short })),
        "updated_at": config.updated_at,
    })
}

/// A template as the chooser lists it: its name and the level it asks for.
pub(crate) fn listing(listing: &TemplateListing) -> Value {
    json!({ "name": listing.name, "min_vip_level": listing.min_vip_level })
}

/// A template described through the same accessors a bot's config is, since a
/// template is a config before any bot has claimed it. Risk and leverage are
/// left out: they are per-bot settings, not properties of the template.
pub(crate) fn template(preview: &TemplatePreview) -> Value {
    let mut body = json!({
        "name": preview.template.name,
        "version": preview.template.version,
        "description": preview.template.description,
        "min_vip_level": preview.template.min_vip_level(),
    });
    if let Value::Object(fields) = config(&preview.config) {
        for (key, value) in fields {
            if key != "updated_at" && key != "risk" && key != "leverage" {
                body[key] = value;
            }
        }
    }
    body
}
