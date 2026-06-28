//! Bridge between Shannon engine events and native OS notifications.
//!
//! `TauriNotificationHandler` implements `shannon_core::notifier::NotificationHandler`
//! and delegates to `tauri-plugin-notification`. It is registered once at
//! startup on the shared `Notifier` (with `Cooldown` + `minimum_level`) stored
//! on `AppState`, so all query-event firing sites share the same dedup state.

use shannon_core::notifier::{Notification, NotificationHandler, NotifierError};
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

/// Notification handler that drives `tauri-plugin-notification` via a cloned
/// `AppHandle`. `AppHandle` is cheap to clone and `Send + Sync`, so a single
/// handler instance is safe to invoke from any Tauri task.
pub struct TauriNotificationHandler {
    app: AppHandle,
    name: String,
}

impl TauriNotificationHandler {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            name: "tauri".to_string(),
        }
    }
}

impl NotificationHandler for TauriNotificationHandler {
    fn send(&self, n: &Notification) -> Result<(), NotifierError> {
        // Master switch + quiet-hours (DND) suppression — desktop-local only.
        // Returning Ok(()) silently drops the OS popup; webhook handlers on the
        // same Notifier are unaffected, so away-from-desk delivery still fires.
        let prefs = crate::commands_notifications::NotificationPrefs::load();
        if !prefs.master_enabled || prefs.within_dnd_window() {
            return Ok(());
        }
        self.app
            .notification()
            .builder()
            .title(&n.title)
            .body(&n.body)
            .show()
            .map_err(|e| NotifierError::HandlerFailed {
                name: self.name.clone(),
                reason: format!("tauri-plugin-notification show failed: {e}"),
            })
    }

    fn name(&self) -> &str {
        &self.name
    }
}
