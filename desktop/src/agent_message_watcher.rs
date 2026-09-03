//! Watches `~/.shannon/agent-messages/` for out-of-band writes and converts
//! them into the same `agent-messages-updated` event the desktop emits for
//! its own writes (`commands_agents::record_agent_message`).
//!
//! Without this, inter-agent messages appended by the CLI or any other
//! external process never refresh the desktop's AgentMessagesPanel — it
//! would still poll or wait for a focus refresh. The watcher is pure
//! sugar on top of the existing event: re-emitting on our own writes is
//! harmless (the panel reload is an idempotent fetch), so the watcher
//! makes no attempt to attribute writes to a process.

use crate::events::event_names;
use notify::{RecursiveMode, Watcher};
use std::sync::mpsc::channel;
use std::time::Duration;
use tauri::Emitter;

/// How long to let the filesystem settle before emitting — one logical
/// append fans out into several notify events (create + write + close).
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(400);

/// Block on a watcher thread for the lifetime of the process. Fails soft:
/// a missing directory or watcher error only logs and skips watching —
/// external messages then fall back to the pre-watcher refresh paths.
pub fn spawn(base_dir: &std::path::Path, app: tauri::AppHandle) {
    let (tx, rx) = channel();
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("agent-message watcher: {e}");
            return;
        }
    };
    if let Err(e) = watcher.watch(base_dir, RecursiveMode::Recursive) {
        eprintln!("agent-message watcher: {} ({e})", base_dir.display());
        return;
    }
    std::thread::spawn(move || {
        // The watcher handle must outlive this loop or its background
        // thread shuts down and the channel closes.
        let _keep_alive = watcher;
        loop {
            match rx.recv() {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    eprintln!("agent-message watcher: {e}");
                    continue;
                }
                Err(_) => break, // channel closed — watcher dropped
            }
            while rx.recv_timeout(DEBOUNCE_WINDOW).is_ok() {}
            let _ = app.emit(event_names::AGENT_MESSAGES_UPDATED, ());
        }
    });
}
