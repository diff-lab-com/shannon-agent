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

use super::{emit_message, matches_trigger, InboundMessage};

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

    tracing::info!("slack listener: started ({} allowed channels)", allowed.len());

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
    parsed.url.ok_or_else(|| "missing url in OK response".into())
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

fn strip_trigger(text: &str, trigger: &str) -> String {
    let trig = trigger.trim();
    if trig.is_empty() {
        return text.trim().to_string();
    }
    let trig_lower = trig.to_lowercase();
    if text.len() >= trig_lower.len() && text[..trig_lower.len()].eq_ignore_ascii_case(trig) {
        text[trig_lower.len()..].trim_start().to_string()
    } else {
        text.trim().to_string()
    }
}
