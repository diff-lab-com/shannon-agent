//! Inbound message listener — Phase 2.
//!
//! Supervisor that owns one tokio task per configured provider (Slack Socket
//! Mode, Telegram getUpdates long-poll). When a message matches the trigger
//! word and source filter, it's forwarded to the frontend via a Tauri event
//! (`inbound-message`) so the UI can decide what to do with it (Phase 3 will
//! wire the trigger into a chat).
//!
//! The supervisor is owned by `AppState` and restarts whenever the inbound
//! config changes (save / clear). All workers share a single watch channel
//! for shutdown so `stop()` is graceful.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;
use tokio::task::JoinHandle;

mod telegram;
mod slack;

pub use telegram::TelegramConfig;
pub use slack::SlackConfig;

/// Normalized inbound message — emitted to the frontend via `inbound-message`.
#[derive(Debug, Clone, Serialize)]
pub struct InboundMessage {
    pub provider: String,   // "slack" | "telegram"
    pub source_id: String,  // channel id (Slack) or chat id (Telegram)
    pub source_name: String,
    pub sender_id: String,
    pub sender_name: String,
    pub text: String,
    pub timestamp: i64,
}

/// What the listener is currently doing for a given provider.
#[derive(Debug, Clone, Serialize, Default)]
pub struct InboundListenerStatus {
    pub telegram_running: bool,
    pub slack_running: bool,
}

/// Handle returned by [`InboundListener::start`]. Drop goes through [`stop`]
/// to keep shutdown explicit — listeners are usually app-lifetime resources.
pub struct InboundListener {
    shutdown_tx: Option<watch::Sender<bool>>,
    tasks: Vec<JoinHandle<()>>,
    telegram_running: bool,
    slack_running: bool,
}

impl InboundListener {
    /// Spawn workers for every provider configured in `dto`. Returns a handle
    /// that owns the join handles and the shutdown signal. Workers that fail
    /// to start (e.g. bad token) log a warning and are skipped — the rest of
    /// the config still runs.
    pub fn start(
        app: AppHandle,
        slack: Option<SlackConfig>,
        telegram: Option<TelegramConfig>,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let mut tasks: Vec<JoinHandle<()>> = Vec::new();
        let telegram_running = telegram.is_some();
        let slack_running = slack.is_some();

        if let Some(cfg) = telegram {
            let app = app.clone();
            let rx = shutdown_rx.clone();
            tasks.push(tokio::spawn(async move {
                telegram::run(app, cfg, rx).await;
            }));
        }
        if let Some(cfg) = slack {
            let app = app.clone();
            let rx = shutdown_rx.clone();
            tasks.push(tokio::spawn(async move {
                slack::run(app, cfg, rx).await;
            }));
        }

        Self {
            shutdown_tx: Some(shutdown_tx),
            tasks,
            telegram_running,
            slack_running,
        }
    }

    /// Signal all workers to shut down and wait for them. Idempotent.
    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }
        for handle in self.tasks.drain(..) {
            let _ = handle.await;
        }
        self.telegram_running = false;
        self.slack_running = false;
    }

    pub fn status(&self) -> InboundListenerStatus {
        InboundListenerStatus {
            telegram_running: self.telegram_running && !self.tasks.is_empty(),
            slack_running: self.slack_running && !self.tasks.is_empty(),
        }
    }
}

/// Emit a normalized message to the frontend. Best-effort: a failed emit
/// (e.g. no window) is logged but does not crash the worker.
pub(crate) fn emit_message(app: &AppHandle, msg: InboundMessage) {
    if let Err(e) = app.emit("inbound-message", &msg) {
        tracing::warn!(provider = %msg.provider, error = %e, "emit inbound-message failed");
    }
}

/// True when `text` starts with `trigger` (case-insensitive, trimmed).
/// Trigger may be empty — in that case every message matches.
pub(crate) fn matches_trigger(text: &str, trigger: &str) -> bool {
    let trig = trigger.trim();
    if trig.is_empty() {
        return true;
    }
    text.trim().to_lowercase().starts_with(&trig.to_lowercase())
}
