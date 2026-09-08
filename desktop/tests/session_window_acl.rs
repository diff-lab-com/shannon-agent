//! P1-1 — compiled-in ACL contract for the session multi-window feature.
//!
//! `desktop/build.rs` generates `acl-manifests.json` + `capabilities.json`
//! from `desktop/capabilities/*.json`, and `tauri::generate_context!`
//! bakes the resolved ACL into the binary. These tests compile the app's
//! real context and assert the exact runtime-visible permission surface:
//!
//! - the JS event API (`plugin:event|listen` etc.) is allowed on `main`
//!   and on `session-*` windows — without this, streaming `query:*` events
//!   never reach any webview;
//! - window title sync (`plugin:window|set_title`) is allowed;
//! - nothing else leaked: dialog/notification/shell/window-close stay
//!   denied (the capability is deliberately minimal).
//!
//! Runtime semantics note: application commands (`send_message`,
//! `open_session_window`, ...) are *not* ACL-gated — `desktop/build.rs`
//! deliberately never writes an `__app-acl__` manifest, and
//! `tauri`'s invoke dispatcher skips the ACL check for app commands when
//! no app manifest exists (webview/mod.rs `on_message`). The
//! `resolve_access(...) == None` result is therefore the *expected* state
//! for app commands and is asserted here as a guard against accidentally
//! enabling an app manifest, which would start denying every existing
//! command.

use tauri::ipc::Origin;

fn allowed(cmd: &str, window: &str) -> bool {
    let mut context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    context
        .runtime_authority_mut()
        .resolve_access(cmd, window, window, &Origin::Local)
        .is_some()
}

const MAIN: &str = "main";
const SESSION: &str = "session-00000000-0000-0000-0000-000000000000";

#[test]
fn event_api_allowed_on_main_and_session_windows() {
    for window in [MAIN, SESSION] {
        assert!(allowed("plugin:event|listen", window), "listen on {window}");
        assert!(
            allowed("plugin:event|unlisten", window),
            "unlisten on {window}"
        );
        assert!(allowed("plugin:event|emit", window), "emit on {window}");
        assert!(
            allowed("plugin:event|emit_to", window),
            "emit_to on {window}"
        );
    }
}

#[test]
fn window_title_sync_allowed_on_main_and_session_windows() {
    for window in [MAIN, SESSION] {
        assert!(
            allowed("plugin:window|set_title", window),
            "set_title on {window}"
        );
    }
}

#[test]
fn capability_stays_minimal_no_other_plugin_commands_allowed() {
    for window in [MAIN, SESSION] {
        // File dialogs (plugin-dialog) are still denied — the current app
        // never had them allowed and P1-1 does not change that.
        assert!(!allowed("plugin:dialog|open", window), "dialog on {window}");
        // Window close is done through the `close_session_window` app
        // command (not ACL-gated), so the raw window API stays closed.
        assert!(
            !allowed("plugin:window|close", window),
            "window close on {window}"
        );
        assert!(
            !allowed("plugin:window|set_focus", window),
            "set_focus on {window}"
        );
        assert!(!allowed("plugin:shell|open", window), "shell on {window}");
        assert!(
            !allowed("plugin:notification|notify", window),
            "notification on {window}"
        );
    }
}

#[test]
fn app_commands_are_not_acl_listed_no_app_manifest_enabled() {
    // `None` here means "unchecked at runtime" (allowed) — see module docs.
    // If this assertion starts failing, an `__app-acl__` manifest leaked
    // into the build and every app command became ACL-gated, which would
    // deny all existing IPC without an exhaustive capability file.
    assert!(!allowed("send_message", MAIN));
    assert!(!allowed("open_session_window", MAIN));
    assert!(!allowed("close_session_window", SESSION));
}
