//! Slack Socket Mode worker.
//!
//! Flow: POST apps.connections.open with the bot token → get a WSS URL →
//! connect → receive envelopes. Hello / disconnect are protocol control
//! messages; events_api carries the actual payload. Each envelope must be
//! ACKed via apps.events.post with envelope_id, otherwise Slack will redeliver.
//!
//! Reconnect logic: on disconnect (or any socket error) we back off 5s and
//! re-handshake. Slack's own hello envelope re-syncs the connection state.
//!
//! Shutdown is cooperative via watch — the read loop checks between receives.

use std::time::Duration;

use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;

use super::{InboundMessage, emit_message, matches_trigger};

#[derive(Debug, Clone)]
pub struct SlackConfig {
    pub bot_token: String,
    pub trigger_word: String,
    pub allowed_channels: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OpenResponse {
    ok: bool,
    url: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    envelope_type: String,
    envelope_id: Option<String>,
    payload: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct Ack<'a> {
    envelope_id: &'a str,
}

const BACKOFF: Duration = Duration::from_secs(5);

pub async fn run(app: AppHandle, cfg: SlackConfig, mut shutdown: watch::Receiver<bool>) {
    if cfg.bot_token.trim().is_empty() {
        tracing::warn!("slack listener: empty bot token, not starting");
        return;
    }

    let http = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest client builds with sensible defaults");
    let allowed: std::collections::HashSet<String> = cfg
        .allowed_channels
        .iter()
        .cloned()
        .filter(|s| !s.trim().is_empty())
        .collect();

    tracing::info!(
        "slack listener: started ({} allowed channels)",
        allowed.len()
    );

    while !*shutdown.borrow() {
        match run_once(&http, &app, &cfg, &allowed, &mut shutdown).await {
            LoopOutcome::Shutdown => break,
            LoopOutcome::Transient => {
                tracing::info!("slack listener: backing off for {:?}", BACKOFF);
                tokio::time::sleep(BACKOFF).await;
            }
        }
    }

    tracing::info!("slack listener: stopped");
}

enum LoopOutcome {
    Shutdown,
    Transient,
}

async fn run_once(
    http: &Client,
    app: &AppHandle,
    cfg: &SlackConfig,
    allowed: &std::collections::HashSet<String>,
    shutdown: &mut watch::Receiver<bool>,
) -> LoopOutcome {
    let url = match open_socket_url(http, &cfg.bot_token).await {
        Ok(u) => u,
        Err(e) => {
            if *shutdown.borrow() {
                return LoopOutcome::Shutdown;
            }
            tracing::warn!(error = %e, "slack listener: apps.connections.open failed");
            return LoopOutcome::Transient;
        }
    };

    let (mut ws, _) = match tokio_tungstenite::connect_async(&url).await {
        Ok(c) => c,
        Err(e) => {
            if *shutdown.borrow() {
                return LoopOutcome::Shutdown;
            }
            tracing::warn!(error = %e, "slack listener: WSS connect failed");
            return LoopOutcome::Transient;
        }
    };

    while !*shutdown.borrow() {
        let next = tokio::time::timeout(Duration::from_secs(60), ws.next()).await;
        let msg = match next {
            Ok(Some(Ok(m))) => m,
            Ok(Some(Err(e))) => {
                tracing::warn!(error = %e, "slack listener: socket error");
                return LoopOutcome::Transient;
            }
            Ok(None) => {
                tracing::info!("slack listener: socket closed by remote");
                return LoopOutcome::Transient;
            }
            Err(_) => {
                // Timeout — keep the connection warm by reading again. Slack
                // sends periodic pings; a 60s silence means something's wrong.
                tracing::debug!("slack listener: 60s silence, reconnecting");
                return LoopOutcome::Transient;
            }
        };

        let Message::Text(text) = msg else {
            // Binary / Ping / Pong / Close frames are handled by tungstenite
            // internally; we only care about text envelopes.
            continue;
        };

        let Ok(env) = serde_json::from_str::<Envelope>(&text) else {
            tracing::warn!(raw = %text, "slack listener: malformed envelope");
            continue;
        };

        if let Some(id) = &env.envelope_id {
            ack_envelope(http, &cfg.bot_token, id).await;
        }

        match env.envelope_type.as_str() {
            "hello" => tracing::info!("slack listener: hello received"),
            "disconnect" => {
                tracing::info!("slack listener: server asked to disconnect");
                return LoopOutcome::Transient;
            }
            "events_api" => {
                if let Some(payload) = env.payload {
                    handle_event(app, cfg, allowed, payload);
                }
            }
            "interactive" | "block_actions" | "view_submission" | "slash_commands" => {
                // Not relevant for inbound chat triggers — silently ignore.
            }
            other => tracing::debug!(kind = %other, "slack listener: unhandled envelope type"),
        }
    }

    let _ = ws.close(None).await;
    LoopOutcome::Shutdown
}

async fn open_socket_url(http: &Client, bot_token: &str) -> Result<String, String> {
    let resp = http
        .post("https://slack.com/api/apps.connections.open")
        .bearer_auth(bot_token)
        .send()
        .await
        .map_err(|e| format!("send: {e}"))?;
    let parsed: OpenResponse = resp.json().await.map_err(|e| format!("decode: {e}"))?;
    if !parsed.ok {
        return Err(parsed.error.unwrap_or_else(|| "unknown error".into()));
    }
    parsed
        .url
        .ok_or_else(|| "missing url in OK response".into())
}

async fn ack_envelope(http: &Client, bot_token: &str, envelope_id: &str) {
    let body = Ack { envelope_id };
    let _ = http
        .post("https://slack.com/api/apps.events.post")
        .bearer_auth(bot_token)
        .json(&body)
        .send()
        .await;
}

fn handle_event(
    app: &AppHandle,
    cfg: &SlackConfig,
    allowed: &std::collections::HashSet<String>,
    payload: serde_json::Value,
) {
    let event = match payload.get("event") {
        Some(e) => e,
        None => return,
    };
    let kind = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if kind != "message" {
        return;
    }
    // Ignore bot messages and message edits — only fresh human text.
    if event.get("subtype").is_some() {
        return;
    }
    let text = event.get("text").and_then(|v| v.as_str()).unwrap_or("");
    if text.is_empty() {
        return;
    }
    let channel = event
        .get("channel")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if !allowed.is_empty() && !allowed.contains(&channel) {
        return;
    }
    if !matches_trigger(text, &cfg.trigger_word) {
        return;
    }
    let user = event
        .get("user")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let ts = event
        .get("ts")
        .and_then(|v| v.as_str())
        .and_then(|s| s.split('.').next())
        .and_then(|n| n.parse::<f64>().ok())
        .map(|f| f as i64)
        .unwrap_or(0);

    let stripped = strip_trigger(text, &cfg.trigger_word);
    emit_message(
        app,
        InboundMessage {
            provider: "slack".into(),
            source_id: channel.clone(),
            source_name: channel,
            sender_id: user,
            sender_name: String::new(),
            text: stripped,
            timestamp: ts,
        },
    );
}

/// Strip the trigger prefix from `text` (case-insensitive, trim-tolerant).
///
/// UTF-8 safe: never slices on a byte index that could land inside a multibyte
/// sequence. Uses `char_indices` + `eq_ignore_ascii_case` on whole-char slices
/// rather than `text[..len]` which panics on multibyte triggers (e.g. `帮我`).
/// Returns the remainder (left-trimmed) on match, or the trimmed original on
/// no match.
fn strip_trigger(text: &str, trigger: &str) -> String {
    let trig = trigger.trim();
    if trig.is_empty() {
        return text.trim().to_string();
    }
    if starts_with_ignore_case(text, trig) {
        // Safe: `trig.chars().count()` gives us a *character* count; we then
        // walk `text`'s char boundaries to find the byte offset of the Nth
        // char. Slicing at a char boundary never panics.
        let prefix_chars = trig.chars().count();
        let split_at = text
            .char_indices()
            .nth(prefix_chars)
            .map(|(byte_idx, _)| byte_idx)
            .unwrap_or(text.len());
        text[split_at..].trim_start().to_string()
    } else {
        text.trim().to_string()
    }
}

/// Case-insensitive prefix check that is UTF-8 safe. Compares character by
/// character so multibyte triggers (CJK, emoji, etc.) work correctly.
fn starts_with_ignore_case(haystack: &str, needle: &str) -> bool {
    let mut h = haystack.chars();
    let mut n = needle.chars();
    loop {
        match (n.next(), h.next()) {
            (None, _) => return true,
            (Some(nc), Some(hc)) if nc.eq_ignore_ascii_case(&hc) => continue,
            (Some(_), _) => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_trigger_ascii_case_insensitive() {
        assert_eq!(
            strip_trigger("Shannon do the thing", "shannon"),
            "do the thing"
        );
        assert_eq!(
            strip_trigger("shannon: run tests", "Shannon"),
            ": run tests"
        );
        assert_eq!(strip_trigger("not prefixed", "shannon"), "not prefixed");
        assert_eq!(strip_trigger("Shannon", "shannon"), "");
    }

    #[test]
    fn empty_trigger_returns_trimmed_text() {
        assert_eq!(strip_trigger("  hello  ", ""), "hello");
    }

    #[test]
    fn strip_trigger_multibyte_chinese_does_not_panic() {
        // Regression for audit #9: byte-slice `text[..trig_lower.len()]` would
        // panic here because `帮我` is 6 bytes but only 2 chars; slicing by
        // byte length landed mid-codepoint.
        let trigger = "帮我";
        let text = "帮我写个测试 please";
        assert_eq!(strip_trigger(text, trigger), "写个测试 please");
    }

    #[test]
    fn strip_trigger_multibyte_emoji() {
        // 4-byte UTF-8 trigger; verifies char-boundary safety.
        let trigger = "🚀";
        let text = "🚀 launch it";
        assert_eq!(strip_trigger(text, trigger), "launch it");
    }

    #[test]
    fn strip_trigger_multibyte_no_match_returns_original() {
        assert_eq!(strip_trigger("hello world", "帮我"), "hello world");
    }

    #[test]
    fn starts_with_ignore_case_handles_multibyte() {
        assert!(starts_with_ignore_case("帮我 go", "帮我"));
        assert!(!starts_with_ignore_case("帮他 go", "帮我"));
        assert!(starts_with_ignore_case("SHANNON go", "shannon"));
    }
}
