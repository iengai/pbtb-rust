//! A stand-in Telegram Bot API, plus builders for the updates the real API
//! would deliver.
//!
//! Handlers call `bot.send_message(...).await` for real; `Bot::set_api_url`
//! points those calls at a local server that records them. So a test asserts on
//! what the user would actually have received, not on a handler's return value —
//! several handlers return `Ok(())` on paths that reply with an error.

use serde_json::{Value, json};
use teloxide::Bot;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const TOKEN: &str = "1234567890:TEST-TOKEN";

/// One outbound Bot API call: the method name and the JSON body sent with it.
#[derive(Debug, Clone)]
pub struct ApiCall {
    pub method: String,
    pub body: Value,
}

impl ApiCall {
    /// The `text` field, for the common case of asserting what the user was
    /// told.
    pub fn text(&self) -> &str {
        self.body.get("text").and_then(Value::as_str).unwrap_or("")
    }
}

pub struct FakeTelegram {
    server: MockServer,
}

impl FakeTelegram {
    pub async fn start() -> Self {
        let server = MockServer::start().await;

        // `sendMessage` and the edit methods answer with a Message; teloxide
        // deserializes the reply, so a shape it rejects surfaces as a handler
        // error rather than as a wrong assertion.
        Mock::given(method("POST"))
            .and(path_regex(r"(?i)/(sendMessage|editMessageText)$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "result": sent_message_stub(),
            })))
            .mount(&server)
            .await;

        // `answerCallbackQuery` and `editMessageReplyMarkup` answer with `true`.
        Mock::given(method("POST"))
            .and(path_regex(
                r"(?i)/(answerCallbackQuery|editMessageReplyMarkup)$",
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "ok": true, "result": true })),
            )
            .mount(&server)
            .await;

        Self { server }
    }

    /// A `Bot` whose API calls land on this server.
    pub fn bot(&self) -> Bot {
        Bot::new(TOKEN).set_api_url(self.server.uri().parse().expect("mock server uri"))
    }

    /// Every call the handlers made, in order.
    pub async fn calls(&self) -> Vec<ApiCall> {
        self.server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .map(to_call)
            .collect()
    }

    /// Just the send calls — what the chat actually shows. teloxide names the
    /// method in the path with its payload type's casing (`SendMessage`), not
    /// the Bot API's own `sendMessage`, so the comparison ignores case.
    pub async fn messages(&self) -> Vec<ApiCall> {
        self.calls()
            .await
            .into_iter()
            .filter(|c| c.method.eq_ignore_ascii_case("sendMessage"))
            .collect()
    }

    /// Every outbound call body, JSON and all. Buttons carry content the user
    /// sees just as much as `text` does — the bot list is a keyboard, not a
    /// paragraph — so a "did the user learn X" assertion belongs here rather
    /// than on `transcript`.
    pub async fn wire(&self) -> String {
        self.calls()
            .await
            .iter()
            .map(|c| format!("{} {}", c.method, c.body))
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    }

    /// The concatenated text of every reply, for substring assertions that do
    /// not care which message carried it.
    pub async fn transcript(&self) -> String {
        self.messages()
            .await
            .iter()
            .map(|c| c.text().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn to_call(req: &Request) -> ApiCall {
    let method = req
        .url
        .path()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string();
    let body = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
    ApiCall { method, body }
}

/// The Message the API returns for a successful send. Only the fields teloxide
/// requires are set.
fn sent_message_stub() -> Value {
    json!({
        "message_id": 1,
        "date": 0,
        "chat": { "id": CHAT_ID, "type": "private", "first_name": "test" },
        "text": "sent",
    })
}

pub const CHAT_ID: i64 = 4242;
/// The Telegram id the harness treats as the allowlisted operator.
pub const USER_ID: u64 = 5_351_347_639;
/// A Telegram id that is never on the allowlist.
pub const STRANGER_ID: u64 = 111_222_333;

/// A text message update, as the Bot API delivers it.
pub fn text_message(user_id: u64, text: &str) -> Value {
    json!({
        "update_id": 1,
        "message": {
            "message_id": 100,
            "date": 0,
            "chat": { "id": CHAT_ID, "type": "private", "first_name": "test" },
            "from": { "id": user_id, "is_bot": false, "first_name": "test" },
            "text": text,
        }
    })
}

/// A button press, as the Bot API delivers it.
pub fn callback(user_id: u64, data: &str) -> Value {
    json!({
        "update_id": 2,
        "callback_query": {
            "id": "cbq-1",
            "from": { "id": user_id, "is_bot": false, "first_name": "test" },
            "chat_instance": "instance-1",
            "data": data,
            "message": {
                "message_id": 101,
                "date": 0,
                "chat": { "id": CHAT_ID, "type": "private", "first_name": "test" },
                "text": "panel",
            }
        }
    })
}

/// An update with no sender at all (a channel post), which no allowlist entry
/// can match.
pub fn senderless() -> Value {
    json!({
        "update_id": 3,
        "channel_post": {
            "message_id": 102,
            "date": 0,
            "chat": { "id": -100, "type": "channel", "title": "somewhere" },
            "text": "/list",
        }
    })
}
