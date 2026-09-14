//! Telegram inbound trigger (audit G5 — message-channel dispatch, slice 1).
//!
//! Long-polls the Telegram Bot API `getUpdates` and hands each incoming
//! message text to an injected handler. Crate-level by design: the desktop
//! shell registers the handler (create a goal / fire a routine) — this module
//! only knows Telegram's wire protocol and the chat allow-list.
//!
//! Auth model: the bot token IS the credential; `allowed_chat_ids` is a
//! hard allow-list (messages from unknown chats are dropped, never handled).
//!
//! The parsing is a pure function (`parse_updates`) so the wire protocol and
//! the allow-list logic are unit-testable without network.

use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Telegram Bot API base. Overridable for tests.
pub const DEFAULT_API_BASE: &str = "https://api.telegram.org";

#[derive(Debug, Clone)]
pub struct TelegramConfig {
    /// Bot token from @BotFather (e.g. `123456:ABC-DEF…`).
    pub bot_token: String,
    /// Only messages from these chat ids are handled. Empty list = drop all
    /// (fail closed).
    pub allowed_chat_ids: Vec<i64>,
    /// Long-poll server wait per request, seconds.
    pub poll_timeout_secs: u64,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            bot_token: String::new(),
            allowed_chat_ids: Vec::new(),
            poll_timeout_secs: 25,
        }
    }
}

/// One extracted, allow-listed message.
#[derive(Debug, Clone, PartialEq)]
pub struct InboundMessage {
    pub update_id: i64,
    pub chat_id: i64,
    pub text: String,
}

/// Parse a `getUpdates` JSON response into allow-listed inbound messages and
/// the next offset cursor (`max(update_id) + 1`). Pure function.
pub fn parse_updates(body: &str, allowed_chat_ids: &[i64]) -> (Vec<InboundMessage>, Option<i64>) {
    let parsed: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return (Vec::new(), None),
    };
    let updates = match parsed.get("result").and_then(Value::as_array) {
        Some(a) => a,
        None => return (Vec::new(), None),
    };
    let mut out = Vec::new();
    let mut next_offset: Option<i64> = None;
    for u in updates {
        let update_id = u.get("update_id").and_then(Value::as_i64);
        let message = u.get("message");
        let chat_id = message
            .and_then(|m| m.get("chat"))
            .and_then(|c| c.get("id"))
            .and_then(Value::as_i64);
        let text = message
            .and_then(|m| m.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if let Some(uid) = update_id {
            next_offset = Some(next_offset.map_or(uid, |prev| prev.max(uid)) + 1);
        }
        if let (Some(uid), Some(chat_id)) = (update_id, chat_id) {
            // Fail closed: an empty allow-list handles nothing.
            if !allowed_chat_ids.contains(&chat_id) || text.is_empty() {
                continue;
            }
            out.push(InboundMessage {
                update_id: uid,
                chat_id,
                text,
            });
        }
    }
    (out, next_offset)
}

/// Long-polling Telegram trigger.
pub struct TelegramTrigger {
    http: Client,
    cfg: TelegramConfig,
    handler: Arc<dyn Fn(&str) + Send + Sync>,
}

impl TelegramTrigger {
    pub fn new(cfg: TelegramConfig, handler: Arc<dyn Fn(&str) + Send + Sync>) -> Self {
        Self {
            http: Client::new(),
            cfg,
            handler,
        }
    }

    fn updates_url(&self, offset: Option<i64>) -> String {
        format!(
            "{}/bot{}/getUpdates?timeout={}&allowed_updates=[\"message\"]{}",
            DEFAULT_API_BASE,
            self.cfg.bot_token,
            self.cfg.poll_timeout_secs,
            offset.map(|o| format!("&offset={o}")).unwrap_or_default(),
        )
    }

    /// One long-poll round. Returns the next offset to resume from.
    pub async fn poll_once(&self, offset: Option<i64>) -> anyhow::Result<Option<i64>> {
        let body = self
            .http
            .get(self.updates_url(offset))
            .timeout(Duration::from_secs(self.cfg.poll_timeout_secs + 10))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let (messages, next) = parse_updates(&body, &self.cfg.allowed_chat_ids);
        for m in &messages {
            (self.handler)(&m.text);
        }
        Ok(next)
    }

    /// Poll until `stop` flips. Errors back off (5s) instead of spinning.
    pub async fn run(&self, stop: Arc<AtomicBool>, mut offset: Option<i64>) {
        let mut backoff = 0u64;
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            match self.poll_once(offset).await {
                Ok(next) => {
                    offset = next;
                    backoff = 0;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "telegram poll failed; backing off");
                    tokio::time::sleep(Duration::from_secs(backoff.max(5))).await;
                    backoff = (backoff * 2).min(60);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r#"{"ok":true,"result":[
        {"update_id":101,"message":{"message_id":1,"chat":{"id":42},"text":"run nightly audit"}},
        {"update_id":102,"message":{"message_id":2,"chat":{"id":999},"text":"intruder"}},
        {"update_id":103,"message":{"message_id":3,"chat":{"id":42},"text":"  "}},
        {"update_id":104,"message":{"message_id":4,"chat":{"id":42},"text":"hello shannon"}}
    ]}"#;

    #[test]
    fn parses_allow_listed_messages_and_advances_offset() {
        let (msgs, next) = parse_updates(BODY, &[42]);
        assert_eq!(msgs.len(), 2, "unknown chat (999) and empty text dropped");
        assert_eq!(msgs[0].text, "run nightly audit");
        assert_eq!(msgs[0].chat_id, 42);
        assert_eq!(msgs[1].text, "hello shannon");
        assert_eq!(next, Some(105), "max(update_id)+1");
    }

    #[test]
    fn empty_allow_list_fails_closed() {
        let (msgs, _) = parse_updates(BODY, &[]);
        assert!(msgs.is_empty(), "no allow-list → nothing handled");
    }

    #[test]
    fn malformed_body_is_tolerated() {
        let (msgs, next) = parse_updates("not json", &[42]);
        assert!(msgs.is_empty());
        assert_eq!(next, None);
    }

    #[test]
    fn updates_url_carries_offset_and_timeout() {
        let cfg = TelegramConfig {
            bot_token: "T".into(),
            allowed_chat_ids: vec![42],
            poll_timeout_secs: 25,
        };
        let trig = TelegramTrigger::new(cfg, Arc::new(|_| {}));
        assert!(trig.updates_url(Some(7)).contains("offset=7"));
        assert!(trig.updates_url(Some(7)).contains("timeout=25"));
        assert!(trig.updates_url(None).contains("botT/getUpdates"));
    }
}
