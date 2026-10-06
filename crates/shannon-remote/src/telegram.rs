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
    /// Bot API base. Defaults to [`DEFAULT_API_BASE`]; overridable so tests
    /// can exercise transport-error paths against a closed local port.
    api_base: String,
}

impl TelegramTrigger {
    pub fn new(cfg: TelegramConfig, handler: Arc<dyn Fn(&str) + Send + Sync>) -> Self {
        Self {
            http: Client::new(),
            cfg,
            handler,
            api_base: DEFAULT_API_BASE.to_string(),
        }
    }

    /// Override the Bot API base (test seam for offline transport errors).
    pub fn with_api_base(mut self, base: impl Into<String>) -> Self {
        self.api_base = base.into();
        self
    }

    fn updates_url(&self, offset: Option<i64>) -> String {
        format!(
            "{}/bot{}/getUpdates?timeout={}&allowed_updates=[\"message\"]{}",
            self.api_base,
            self.cfg.bot_token,
            self.cfg.poll_timeout_secs,
            offset.map(|o| format!("&offset={o}")).unwrap_or_default(),
        )
    }

    /// Replace the bot token with a placeholder (no-op for an empty token —
    /// replacing an empty needle would mangle the whole string).
    fn redact_token(&self, text: &str) -> String {
        if self.cfg.bot_token.is_empty() {
            text.to_string()
        } else {
            text.replace(&self.cfg.bot_token, "<redacted>")
        }
    }

    /// Map a transport error to a token-free message (review F21).
    ///
    /// `reqwest::Error`'s Display embeds the full request URL — and the URL
    /// carries the credential (`/bot<token>/getUpdates`) — so raw errors
    /// must never flow into anyhow results, `tracing` payloads or
    /// notifications. Full detail goes to the debug log with the token
    /// redacted; the returned error carries only the endpoint name, the
    /// error kind and the HTTP status.
    fn sanitize_transport_error(&self, endpoint: &str, err: reqwest::Error) -> anyhow::Error {
        tracing::debug!(
            target: "telegram",
            endpoint,
            detail = %self.redact_token(&err.to_string()),
            "telegram transport error (full detail, token redacted)"
        );
        let kind = if err.is_timeout() {
            "timeout"
        } else if err.is_connect() {
            "connect"
        } else if err.is_body() {
            "body"
        } else if err.is_decode() {
            "decode"
        } else if err.is_status() {
            "status"
        } else {
            "request"
        };
        let status = err
            .status()
            .map(|s| format!(" (HTTP {s})"))
            .unwrap_or_default();
        anyhow::anyhow!("telegram {endpoint} failed: {kind}{status}")
    }

    /// One long-poll round. Returns the next offset to resume from.
    ///
    /// Transport failures are sanitized: the bot token never leaves this
    /// method inside an error message (review F21).
    pub async fn poll_once(&self, offset: Option<i64>) -> anyhow::Result<Option<i64>> {
        let response = self
            .http
            .get(self.updates_url(offset))
            .timeout(Duration::from_secs(self.cfg.poll_timeout_secs + 10))
            .send()
            .await
            .map_err(|e| self.sanitize_transport_error("getUpdates", e))?
            .error_for_status()
            .map_err(|e| self.sanitize_transport_error("getUpdates", e))?;
        let body = response
            .text()
            .await
            .map_err(|e| self.sanitize_transport_error("getUpdates", e))?;
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

    #[test]
    fn redacts_token_but_leaves_other_text_alone() {
        let cfg = TelegramConfig {
            bot_token: "123:ABC".into(),
            allowed_chat_ids: vec![42],
            poll_timeout_secs: 25,
        };
        let trig = TelegramTrigger::new(cfg, Arc::new(|_| {}));
        assert_eq!(
            trig.redact_token("GET https://api.telegram.org/bot123:ABC/getUpdates failed"),
            "GET https://api.telegram.org/bot<redacted>/getUpdates failed"
        );
        // Empty token: no replacement (an empty needle would mangle text).
        let anon = TelegramTrigger::new(TelegramConfig::default(), Arc::new(|_| {}));
        assert_eq!(anon.redact_token("plain error"), "plain error");
    }

    #[tokio::test]
    async fn transport_error_does_not_leak_the_bot_token() {
        // F21: reqwest's error Display embeds the full request URL, and the
        // URL carries the bot token. A refused connection must surface a
        // sanitized error that names the endpoint — never the token.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        drop(listener); // the port now refuses connections

        const TOKEN: &str = "123456:SECRET-TOKEN";
        let cfg = TelegramConfig {
            bot_token: TOKEN.into(),
            allowed_chat_ids: vec![42],
            poll_timeout_secs: 25,
        };
        let trig = TelegramTrigger::new(cfg, Arc::new(|_| {}))
            .with_api_base(format!("http://127.0.0.1:{port}"));

        let err = trig
            .poll_once(Some(3))
            .await
            .expect_err("refused connection must error");
        let rendered = format!("{err:#}");
        assert!(
            !rendered.contains(TOKEN),
            "error rendering must not contain the bot token: {rendered}"
        );
        assert!(
            rendered.contains("getUpdates"),
            "sanitized error names the endpoint: {rendered}"
        );
        assert!(
            rendered.contains("connect"),
            "sanitized error names the error kind: {rendered}"
        );
    }
}
