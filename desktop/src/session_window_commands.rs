//! P1-1 — Session multi-window: commands, registry, persistence, restore.
//!
//! One desktop window per session (`session-<uuid>` label), openable from
//! the sidebar's「在新窗口打开」entry. The frontend boots the new window with
//! `/?windowSession=<id>`, which puts it into "window mode" (slim chrome,
//! pinned to that session — see `desktop/ui/src/lib/windowSession.ts`).
//!
//! Frozen command contract (do not reshape):
//! - [`open_session_window`]`(sessionId) -> { label, sessionId }` — creates
//!   the `session-<uuid>` window (1080x760, min 800x600, title = session
//!   title) or focuses the existing one (dedupe by label);
//! - [`list_session_windows`]`() -> [{ label, sessionId }]`;
//! - [`close_session_window`]`(label)` — refuses non-`session-*` labels so
//!   the main window can never be closed through this path.
//!
//! Supporting command: [`reveal_session_in_main`] — the window-mode header's
//! 「在主窗口打开」 button: focuses `main` and emits
//! [`SESSION_WINDOW_REVEAL`] so the main window switches to the session.
//!
//! Registry + persistence: opened labels live in the in-memory
//! [`SessionWindowRegistry`] (on `AppState`) and the session-id list is
//! mirrored into `DesktopConfig.open_session_windows`
//! (`~/.shannon/desktop/config.json`, following the existing config-store
//! convention). Window destruction (user close or [`close_session_window`])
//! cleans both via the global `on_window_event` hook in `main.rs`. App
//! startup replays the persisted list (`restore_session_windows` in
//! `setup`), silently skipping ids that no longer open, so a crash or
//! restart brings back the previous session-window set.
//!
//! ACL note: creating/focusing/closing windows happens on the Rust side,
//! which is not ACL-gated; the capability file
//! (`desktop/capabilities/session-windows.json`) only covers the JS-side
//! APIs the window-mode UI needs (core:event listen, core:window
//! set-title). See `desktop/build.rs` for how the ACL is generated.

use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Mutex;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use uuid::Uuid;

use crate::commands::{AppState, SessionMeta};
use crate::config;

/// Prefix of every session window's label; also the security boundary for
/// [`close_session_window`] (main can never be closed through it).
pub const SESSION_WINDOW_PREFIX: &str = "session-";

/// URL query parameter the session window boots with (`/?windowSession=<id>`).
/// Mirrored by the frontend parser (`ui/src/lib/windowSession.ts`).
pub const SESSION_WINDOW_QUERY_PARAM: &str = "windowSession";

/// Desktop-internal control event: the window-mode header's
/// 「在主窗口打开」 asks the main window to switch to a session. Emitted
/// (targeted) only at `main`.
pub const SESSION_WINDOW_REVEAL: &str = "session-window:reveal";

/// Window geometry: 1080x760 default, 800x600 minimum (task brief).
const SESSION_WINDOW_SIZE: (f64, f64) = (1080.0, 760.0);
const SESSION_WINDOW_MIN_SIZE: (f64, f64) = (800.0, 600.0);

/// Derive the deterministic window label for a session id.
/// Errors on non-UUID input — labels are derived from validated UUIDs only.
pub fn session_window_label(session_id: &str) -> Result<String, String> {
    let uuid =
        Uuid::parse_str(session_id.trim()).map_err(|e| format!("invalid sessionId: {e}"))?;
    Ok(format!("{SESSION_WINDOW_PREFIX}{uuid}"))
}

/// Inverse of [`session_window_label`] — `None` for non-session labels
/// (e.g. `main`).
pub fn session_id_from_label(label: &str) -> Option<&str> {
    label.strip_prefix(SESSION_WINDOW_PREFIX)
}

/// Frozen DTO — `{ label, sessionId }` (camelCase per the P0-2 contract
/// convention).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionWindowInfo {
    pub label: String,
    pub session_id: String,
}

/// Payload of [`SESSION_WINDOW_REVEAL`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionWindowRevealPayload {
    pub session_id: String,
}

// ── Registry ─────────────────────────────────────────────────────────────

/// In-memory label → session-id registry of live session windows.
/// Authoritative source for [`list_session_windows`]; mirrored to
/// `DesktopConfig.open_session_windows` for restart restore.
#[derive(Default)]
pub struct SessionWindowRegistry(Mutex<BTreeMap<String, String>>);

impl SessionWindowRegistry {
    /// Insert/refresh an entry (idempotent).
    pub fn register(&self, label: &str, session_id: &str) {
        self.lock()
            .insert(label.to_string(), session_id.to_string());
    }

    /// Remove an entry; returns the session id if the label was known.
    pub fn unregister(&self, label: &str) -> Option<String> {
        self.lock().remove(label)
    }

    /// Snapshot of the registry entries.
    pub fn list(&self) -> Vec<SessionWindowInfo> {
        self.lock()
            .iter()
            .map(|(label, session_id)| SessionWindowInfo {
                label: label.clone(),
                session_id: session_id.clone(),
            })
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, String>> {
        self.0.lock().expect("session window registry poisoned")
    }
}

// ── Persistence (DesktopConfig.open_session_windows) ─────────────────────

/// Keep exactly one entry per session id (dedupe on re-open).
pub(crate) fn upsert_session_window(list: &mut Vec<String>, session_id: &str) {
    if !list.iter().any(|id| id == session_id) {
        list.push(session_id.to_string());
    }
}

/// Drop a session id if present.
pub(crate) fn remove_session_window(list: &mut Vec<String>, session_id: &str) {
    list.retain(|id| id != session_id);
}

/// Keep only the entries that parse as UUIDs (normalized by trimming) —
/// persisted lists from older or hand-edited configs must not poison
/// window restore.
pub(crate) fn sanitize_persisted_session_windows(list: &[String]) -> Vec<String> {
    list.iter()
        .filter_map(|id| Uuid::parse_str(id.trim()).ok())
        .map(|uuid| uuid.to_string())
        .collect()
}

/// Mirror the registry's session ids into the persisted desktop config.
async fn persist_session_windows(state: &AppState) {
    let mut config = state.desktop_config.write().await;
    let ids: Vec<String> = state
        .session_windows
        .list()
        .into_iter()
        .map(|w| w.session_id)
        .collect();
    config.open_session_windows = ids;
    if let Err(e) = config::save_config(&config) {
        tracing::warn!(error = %e, "failed to persist session window list");
    }
}

// ── Window lifecycle ─────────────────────────────────────────────────────

fn focus(window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Session display title for the window title bar; 「Shannon」 fallback when
/// the session has no resolvable (non-empty) title yet.
fn session_title(sessions: &[SessionMeta], session_id: &str) -> String {
    sessions
        .iter()
        .find(|s| s.id == session_id)
        .map(|s| s.title.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Shannon".to_string())
}

/// Create (or focus) the `session-<uuid>` window. Shared by the
/// [`open_session_window`] command and startup restore.
async fn open_session_window_inner(
    state: &AppState,
    app: &tauri::AppHandle,
    session_id: &str,
) -> Result<SessionWindowInfo, String> {
    let uuid =
        Uuid::parse_str(session_id.trim()).map_err(|e| format!("invalid sessionId: {e}"))?;
    let id_str = uuid.to_string();
    let label = session_window_label(&id_str)?;

    // Dedupe: an existing window for the same session is focused instead.
    if let Some(existing) = app.get_webview_window(&label) {
        focus(&existing);
        state.session_windows.register(&label, &id_str);
        persist_session_windows(state).await;
        return Ok(SessionWindowInfo {
            label,
            session_id: id_str,
        });
    }

    let title = session_title(&state.sessions.lock().await, &id_str);
    let url = format!("/{SESSION_WINDOW_QUERY_PARAM}={id_str}");
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title(title)
        .inner_size(SESSION_WINDOW_SIZE.0, SESSION_WINDOW_SIZE.1)
        .min_inner_size(SESSION_WINDOW_MIN_SIZE.0, SESSION_WINDOW_MIN_SIZE.1)
        .build()
        .map_err(|e| format!("failed to create session window: {e}"))?;
    focus(&window);

    state.session_windows.register(&label, &id_str);
    persist_session_windows(state).await;
    Ok(SessionWindowInfo {
        label,
        session_id: id_str,
    })
}

/// Frozen contract: `open_session_window(sessionId) -> { label, sessionId }`.
#[tauri::command]
pub async fn open_session_window(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    session_id: String,
) -> Result<SessionWindowInfo, String> {
    open_session_window_inner(&state, &app, &session_id).await
}

/// Frozen contract: `list_session_windows() -> [{ label, sessionId }]`.
/// Entries whose window no longer exists are filtered out.
#[tauri::command]
pub async fn list_session_windows(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Vec<SessionWindowInfo>, String> {
    Ok(state
        .session_windows
        .list()
        .into_iter()
        .filter(|w| app.get_webview_window(&w.label).is_some())
        .collect())
}

/// Frozen contract: `close_session_window(label)`. Only `session-*` labels
/// are accepted — the main window is closed by the user or tray Quit, never
/// through this command.
#[tauri::command]
pub async fn close_session_window(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    label: String,
) -> Result<(), String> {
    if session_id_from_label(&label).is_none() {
        return Err(format!("not a session window label: {label}"));
    }
    if let Some(window) = app.get_webview_window(&label) {
        window
            .close()
            .map_err(|e| format!("failed to close session window: {e}"))?;
    }
    // The Destroyed window event also cleans up; doing it here as well keeps
    // registry + persistence correct even if the window was already gone.
    state.session_windows.unregister(&label);
    persist_session_windows(&state).await;
    Ok(())
}

/// Supporting command for the window-mode header: focus the main window and
/// ask it to switch to `sessionId` via [`SESSION_WINDOW_REVEAL`].
#[tauri::command]
pub async fn reveal_session_in_main(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<(), String> {
    // Validate before acting on it — the payload is echoed to the frontend.
    Uuid::parse_str(session_id.trim()).map_err(|e| format!("invalid sessionId: {e}"))?;
    let Some(main) = app.get_webview_window("main") else {
        return Err("main window not found".into());
    };
    focus(&main);
    app.emit_to(
        "main",
        SESSION_WINDOW_REVEAL,
        SessionWindowRevealPayload { session_id },
    )
    .map_err(|e| format!("failed to emit reveal event: {e}"))
}

/// `WindowEvent::Destroyed` cleanup for session windows — wired from the
/// global `on_window_event` hook in `main.rs` (covers titlebar close,
/// `close_session_window`, and OS-driven teardown alike).
pub async fn cleanup_destroyed_window(state: &AppState, label: &str) {
    if session_id_from_label(label).is_none() {
        return;
    }
    if let Some(session_id) = state.session_windows.unregister(label) {
        tracing::info!(%label, %session_id, "session window destroyed");
        persist_session_windows(state).await;
    }
}

/// Startup restore (called from `setup`): reopen the persisted session
/// windows. Failures are logged, never fatal — a stale id (session deleted
/// on disk elsewhere, hand-edited config) must not block app start. The
/// persisted list is rewritten with the successfully restored ids so it
/// self-heals.
pub fn restore_session_windows(app: &tauri::AppHandle) {
    let persisted = sanitize_persisted_session_windows(&config::load_config().open_session_windows);
    if persisted.is_empty() {
        return;
    }
    tracing::info!(count = persisted.len(), "restoring session windows");

    let state = app.state::<AppState>();
    let mut restored = Vec::new();
    for session_id in persisted {
        match tauri::async_runtime::block_on(open_session_window_inner(
            state.inner(),
            app,
            &session_id,
        )) {
            Ok(info) => restored.push(info.session_id),
            Err(e) => tracing::warn!(%session_id, error = %e, "session window restore failed"),
        }
    }

    // Self-heal the persisted list (drop ids that failed to restore).
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let mut cfg = state.desktop_config.write().await;
        cfg.open_session_windows = restored;
        if let Err(e) = config::save_config(&cfg) {
            tracing::warn!(error = %e, "failed to persist restored session window list");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_window_label_is_prefixed_uuid() {
        let id = "7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f1";
        assert_eq!(
            session_window_label(id).unwrap(),
            format!("session-{id}"),
            "label rule: session-<uuid>"
        );
        // Trimmed input is accepted (frontend may send padded strings).
        assert_eq!(session_window_label(" 7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f1 ").unwrap(), format!("session-{id}"));
    }

    #[test]
    fn session_window_label_rejects_non_uuid() {
        assert!(session_window_label("not-a-uuid").is_err());
        assert!(session_window_label("../etc/passwd").is_err());
        assert!(session_window_label("").is_err());
    }

    #[test]
    fn session_id_from_label_round_trips_and_guards_main() {
        let id = "7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f1";
        let label = session_window_label(id).unwrap();
        assert_eq!(session_id_from_label(&label), Some(id));
        assert_eq!(session_id_from_label("main"), None);
        assert_eq!(session_id_from_label("session-"), Some(""));
    }

    #[test]
    fn registry_register_unregister_list_round_trip() {
        let registry = SessionWindowRegistry::default();
        assert!(registry.list().is_empty());
        registry.register("session-a", "a");
        registry.register("session-b", "b");
        // Re-register is idempotent.
        registry.register("session-a", "a");
        let list = registry.list();
        assert_eq!(list.len(), 2);
        assert!(list.contains(&SessionWindowInfo {
            label: "session-a".into(),
            session_id: "a".into(),
        }));
        assert_eq!(registry.unregister("session-a").as_deref(), Some("a"));
        assert_eq!(registry.unregister("session-a"), None, "second unregister");
        assert_eq!(registry.list().len(), 1);
    }

    #[test]
    fn upsert_and_remove_keep_list_deduped() {
        let mut list = Vec::new();
        upsert_session_window(&mut list, "a");
        upsert_session_window(&mut list, "b");
        upsert_session_window(&mut list, "a"); // re-open of an existing window
        assert_eq!(list, vec!["a".to_string(), "b".to_string()]);
        remove_session_window(&mut list, "a");
        assert_eq!(list, vec!["b".to_string()]);
        remove_session_window(&mut list, "missing"); // no-op
        assert_eq!(list, vec!["b".to_string()]);
    }

    #[test]
    fn sanitize_persisted_session_windows_drops_invalid_ids() {
        let sanitized = sanitize_persisted_session_windows(&[
            "7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f1".into(),
            "garbage".into(),
            "".into(),
            " 7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f2 ".into(),
        ]);
        assert_eq!(
            sanitized,
            vec![
                "7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f1".to_string(),
                "7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f2".to_string(),
            ]
        );
    }

    #[test]
    fn session_title_falls_back_to_shannon() {
        let sessions = vec![SessionMeta {
            id: "7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f1".into(),
            title: "Refactor the parser".into(),
            created_at: 0,
            message_count: 0,
            working_dir: None,
            parent_id: None,
            branch_point: None,
        }];
        assert_eq!(
            session_title(&sessions, "7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f1"),
            "Refactor the parser"
        );
        // Unknown session → fallback.
        assert_eq!(session_title(&sessions, "deadbeef-0000-0000-0000-000000000000"), "Shannon");
        // Blank title → fallback.
        let blank = vec![SessionMeta {
            id: "7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f1".into(),
            title: "   ".into(),
            created_at: 0,
            message_count: 0,
            working_dir: None,
            parent_id: None,
            branch_point: None,
        }];
        assert_eq!(session_title(&blank, "7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f1"), "Shannon");
    }

    #[test]
    fn session_window_info_serializes_camel_case() {
        let info = SessionWindowInfo {
            label: "session-abc".into(),
            session_id: "abc".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains(r#""sessionId":"abc""#), "{json}");
        assert!(json.contains(r#""label":"session-abc""#), "{json}");
    }
}
