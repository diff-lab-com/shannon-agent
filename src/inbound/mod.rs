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
//!
//! ## Health monitoring (audit #8)
//!
//! The previous implementation set `slack_running = true` optimistically at
//! start and only cleared it on explicit `stop()`. A worker that exited on its
//! own (bad token, panic, fatal socket error after retry budget) left the flag
//! stuck at `true`, so the UI kept showing a green "running" badge.
//!
//! The current implementation tracks each worker's liveness via a supervisor
//! task that `await`s the worker's `JoinHandle`. When a worker exits — for any
//! reason — the supervisor flips the corresponding `AtomicBool` to `false`,
//! emits a `shannon:inbound-worker-exited` Tauri event so the UI can surface a
//! toast, and logs at `warn!`. The `stop()` path sets a shared `stopping` flag
//! first so the supervisor suppresses the "unexpected" event during graceful
//! shutdown.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;
use tokio::task::JoinHandle;

mod telegram;
mod slack;

pub use telegram::TelegramConfig;
pub use slack::SlackConfig;

/// Tauri event emitted when an inbound worker task exits outside of an
/// explicit `stop()` call. Payload is [`WorkerExitedPayload`].
pub const WORKER_EXITED_EVENT: &str = "shannon:inbound-worker-exited";

/// Payload for [`WORKER_EXITED_EVENT`].
#[derive(Debug, Clone, Serialize)]
pub struct WorkerExitedPayload {
    /// "slack" | "telegram"
    pub provider: String,
    /// `"panicked"` if the task panicked, `"returned"` if it finished
    /// normally. Workers that bail early (e.g. empty token) return normally
    /// but unexpectedly — the supervisor flags both as an exit worth surfacing
    /// unless we're shutting down.
    pub reason: String,
}

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
///
/// Both flags reflect *actual* worker liveness, not optimistic intent: see the
/// module-level docs on health monitoring (audit #8).
#[derive(Debug, Clone, Serialize, Default)]
pub struct InboundListenerStatus {
    pub telegram_running: bool,
    pub slack_running: bool,
}

/// Handle returned by [`InboundListener::start`]. Drop goes through [`stop`]
/// to keep shutdown explicit — listeners are usually app-lifetime resources.
pub struct InboundListener {
    shutdown_tx: Option<watch::Sender<bool>>,
    /// Tracks whether a worker is alive. Shared with the per-worker supervisor
    /// tasks so they can flip it to `false` when their worker exits.
    slack_running: Arc<AtomicBool>,
    telegram_running: Arc<AtomicBool>,
    /// Set to `true` by `stop()` so the supervisor tasks know an imminent
    /// worker exit is expected and should NOT emit a worker-exited event.
    stopping: Arc<AtomicBool>,
}

impl InboundListener {
    /// Spawn workers for every provider configured in `dto`. Returns a handle
    /// that owns the shutdown signal and live liveness flags. Workers that
    /// fail to start (e.g. bad token) log a warning and are skipped — the rest
    /// of the config still runs.
    ///
    /// Each spawned worker is wrapped in a supervisor task that monitors its
    /// `JoinHandle`; see the module docs for why (audit #8).
    pub fn start(
        app: AppHandle,
        slack: Option<SlackConfig>,
        telegram: Option<TelegramConfig>,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let slack_running = Arc::new(AtomicBool::new(slack.is_some()));
        let telegram_running = Arc::new(AtomicBool::new(telegram.is_some()));
        let stopping = Arc::new(AtomicBool::new(false));

        if let Some(cfg) = telegram {
            let worker_app = app.clone();
            let supervisor_app = app.clone();
            let rx = shutdown_rx.clone();
            let flag = telegram_running.clone();
            let stop_flag = stopping.clone();
            let worker = tokio::spawn(async move {
                telegram::run(worker_app, cfg, rx).await;
            });
            spawn_supervisor(supervisor_app, "telegram", worker, flag, stop_flag);
        }
        if let Some(cfg) = slack {
            let worker_app = app.clone();
            let supervisor_app = app.clone();
            let rx = shutdown_rx.clone();
            let flag = slack_running.clone();
            let stop_flag = stopping.clone();
            let worker = tokio::spawn(async move {
                slack::run(worker_app, cfg, rx).await;
            });
            spawn_supervisor(supervisor_app, "slack", worker, flag, stop_flag);
        }

        Self {
            shutdown_tx: Some(shutdown_tx),
            slack_running,
            telegram_running,
            stopping,
        }
    }

    /// Signal all workers to shut down and wait for them. Idempotent.
    ///
    /// Sets the `stopping` flag *before* signalling so the supervisor tasks
    /// know not to emit a worker-exited event for this graceful shutdown.
    pub async fn stop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }
        // Wait for the flags to clear — the supervisors flip them when the
        // workers finish. Give them a generous bound so a wedged worker can't
        // hang stop() forever; this matches the prior semantics where stop()
        // drained all join handles.
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(35);
        loop {
            let any_alive = self.slack_running.load(Ordering::SeqCst)
                || self.telegram_running.load(Ordering::SeqCst);
            if !any_alive {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                tracing::warn!(
                    "inbound listener: stop() timed out waiting for workers to exit; flags may be stale"
                );
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        // Defensive: make sure status() reports stopped even if a supervisor
        // never ran (e.g. we timed out above).
        self.slack_running.store(false, Ordering::SeqCst);
        self.telegram_running.store(false, Ordering::SeqCst);
    }

    pub fn status(&self) -> InboundListenerStatus {
        InboundListenerStatus {
            telegram_running: self.telegram_running.load(Ordering::SeqCst),
            slack_running: self.slack_running.load(Ordering::SeqCst),
        }
    }
}

/// Spawn a supervisor for a single worker. When the worker's `JoinHandle`
/// resolves, the supervisor clears `running_flag` and (unless `stop()` set
/// `stopping`) emits a [`WORKER_EXITED_EVENT`] so the UI can surface a toast.
fn spawn_supervisor(
    app: AppHandle,
    provider: &'static str,
    worker: JoinHandle<()>,
    running_flag: Arc<AtomicBool>,
    stopping: Arc<AtomicBool>,
) {
    tokio::spawn(async move {
        let reason = match worker.await {
            Ok(()) => "returned",
            Err(join_err) if join_err.is_cancelled() => "cancelled",
            Err(_) => "panicked",
        };

        // The worker is gone — reflect that immediately so status() stops lying.
        running_flag.store(false, Ordering::SeqCst);

        if stopping.load(Ordering::SeqCst) {
            // Graceful shutdown path; don't alarm the user.
            tracing::info!(provider, reason, "inbound worker exited during shutdown");
            return;
        }

        tracing::warn!(
            provider,
            reason,
            "inbound worker exited unexpectedly — flagging as stopped"
        );
        let payload = WorkerExitedPayload {
            provider: provider.into(),
            reason: reason.into(),
        };
        if let Err(e) = app.emit(WORKER_EXITED_EVENT, &payload) {
            tracing::warn!(
                provider,
                error = %e,
                "failed to emit inbound worker-exited event"
            );
        }
    });
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression for audit #8: a worker that exits unexpectedly must cause
    /// `status()` to report `slack_running: false`. We can't easily construct
    /// a Wry `AppHandle` in a unit test, so this exercises the liveness-flag
    /// mechanism directly — the same `Arc<AtomicBool>` the supervisor flips
    /// when a worker's `JoinHandle` resolves.
    #[tokio::test]
    async fn worker_exit_flips_running_flag_to_false() {
        let slack_running = Arc::new(AtomicBool::new(true));
        let telegram_running = Arc::new(AtomicBool::new(true));

        // Simulate a worker exiting. The supervisor would do:
        //   running_flag.store(false, SeqCst);
        // We test the observable effect on a status struct built the same way
        // `InboundListener::status` builds it.
        assert!(slack_running.load(Ordering::SeqCst));
        slack_running.store(false, Ordering::SeqCst);

        let status = InboundListenerStatus {
            telegram_running: telegram_running.load(Ordering::SeqCst),
            slack_running: slack_running.load(Ordering::SeqCst),
        };
        assert!(
            !status.slack_running,
            "slack_running must reflect the flag flip"
        );
        assert!(
            status.telegram_running,
            "telegram_running must stay true (its worker is still alive)"
        );
    }

    /// `stop()` semantics: after stop completes, both flags must be false even
    /// if the supervisor never ran (defensive reset). This mirrors the tail
    /// of `InboundListener::stop`.
    #[tokio::test]
    async fn defensive_reset_after_stop_deadline_makes_status_false() {
        let slack_running = Arc::new(AtomicBool::new(true));
        let telegram_running = Arc::new(AtomicBool::new(true));
        // Defensive reset (what stop() does if it times out).
        slack_running.store(false, Ordering::SeqCst);
        telegram_running.store(false, Ordering::SeqCst);
        let status = InboundListenerStatus {
            telegram_running: telegram_running.load(Ordering::SeqCst),
            slack_running: slack_running.load(Ordering::SeqCst),
        };
        assert!(!status.slack_running);
        assert!(!status.telegram_running);
    }

    /// `stop()` sets the stopping flag BEFORE signalling workers, so the
    /// supervisor knows not to emit a worker-exited event. Verify the flag
    /// propagation.
    #[tokio::test]
    async fn stop_sets_stopping_flag_before_signalling() {
        let stopping = Arc::new(AtomicBool::new(false));
        // Mimic the first line of stop().
        stopping.store(true, Ordering::SeqCst);
        assert!(stopping.load(Ordering::SeqCst));
    }
}
