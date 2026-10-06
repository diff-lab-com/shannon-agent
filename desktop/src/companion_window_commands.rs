//! Office Wave 3 C3 — companion Quick Capture window.
//!
//! A single small (`companion` label) always-on-top-capable window holding a
//! minimal capture box. The window loads the same SPA at the `/companion`
//! route (NOT `index.html#...` — the app uses `BrowserRouter`, so a hash
//! fragment would fall into the `*` → `/chat` redirect; Tauri 2.x serves
//! `index.html` for extension-less unknown paths, so `/companion` works in
//! dev (Vite SPA fallback) and production (asset fallback) alike).
//!
//! Frozen command contract (mirrors the P1-1 session-window style):
//! - [`open_companion_window`]`() -> { label }` — creates the `companion`
//!   window (420x320, opens unfocused so it never steals the user's typing)
//!   or focuses the existing one (dedupe by fixed label);
//! - [`set_companion_always_on_top`]`(enabled) -> ()` — the checkbox in the
//!   companion UI toggles the flag through this command instead of the JS
//!   window API, keeping the ACL surface one plain app command (same flow as
//!   every other command; no extra plugin permission for the window group).
//!
//! Cross-window messaging: the companion frontend emits
//! [`COMPANION_PROMPT_EVENT`] targeted at `main` (via `emitTo`, so session
//! windows never see it); the main window's frontend listener (in
//! `ui/src/lib/companionBridge.ts`, mounted from `App.tsx`) pushes the text
//! into the chat composer as a DRAFT — never auto-sent (same trust contract
//! as the Wave 2 composer bridge).
//!
//! No persistence: unlike session windows, the companion is not restored on
//! relaunch — it is a transient scratchpad, and a stale restore entry for a
//! window nobody asked for would be worse than reopening it manually.
//!
//! ACL note: window creation/focus/always-on-top all happen on the Rust
//! side (not ACL-gated). The command surface is granted through
//! `acl/app-permissions.json` (`app-window-management` set) and
//! `capabilities/app-commands.json`, whose `windows` list includes the
//! `companion` label because the same SPA bundle (with AppContext's startup
//! invokes) boots in the companion window too — the exact rationale the
//! session windows were granted under. The JS event API reaches `companion`
//! through the dedicated `capabilities/companion-window.json` (emit only).

use serde::Serialize;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Fixed label of the companion window. Also the security boundary for
/// [`set_companion_always_on_top`] — the command only ever touches this
/// window, never `main` or a `session-*` window.
pub const COMPANION_WINDOW_LABEL: &str = "companion";

/// Route (inside the SPA) the companion window boots with. Mirrored by the
/// frontend route in `ui/src/App.tsx` and by `COMPANION_ROUTE` in
/// `ui/src/lib/companionBridge.ts`.
pub const COMPANION_ROUTE: &str = "/companion";

/// Cross-window event the companion UI emits (targeted at `main`) when the
/// user hits Send. Mirrored by `COMPANION_PROMPT_EVENT` in
/// `ui/src/lib/companionBridge.ts`.
pub const COMPANION_PROMPT_EVENT: &str = "shannon:companion-prompt";

/// Window geometry: 420x320 default (task brief), bounded below so the
/// capture box stays usable.
const COMPANION_WINDOW_SIZE: (f64, f64) = (420.0, 320.0);
const COMPANION_WINDOW_MIN_SIZE: (f64, f64) = (280.0, 200.0);

/// Frozen DTO — `{ label }` (camelCase per the window-contract convention).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionWindowInfo {
    pub label: String,
}

fn focus(window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Create (or focus) the `companion` window. Shared by the
/// [`open_companion_window`] command and the tray「Quick Capture」entry.
pub fn open_companion_window_inner(app: &tauri::AppHandle) -> Result<CompanionWindowInfo, String> {
    // Dedupe: an existing companion window is focused instead of recreated.
    if let Some(existing) = app.get_webview_window(COMPANION_WINDOW_LABEL) {
        focus(&existing);
        return Ok(CompanionWindowInfo {
            label: COMPANION_WINDOW_LABEL.to_string(),
        });
    }

    let window = WebviewWindowBuilder::new(
        app,
        COMPANION_WINDOW_LABEL,
        WebviewUrl::App(COMPANION_ROUTE.into()),
    )
    .title("Shannon — Quick Capture")
    .inner_size(COMPANION_WINDOW_SIZE.0, COMPANION_WINDOW_SIZE.1)
    .min_inner_size(COMPANION_WINDOW_MIN_SIZE.0, COMPANION_WINDOW_MIN_SIZE.1)
    .resizable(true)
    // Quick capture must not steal focus from whatever the user is typing
    // in when they summon it. Stays behind the current window until clicked.
    .focused(false)
    // Floats above other windows by default (the point of a scratchpad);
    // the checkbox in the companion UI toggles this at runtime through
    // `set_companion_always_on_top`.
    .always_on_top(true)
    .build()
    .map_err(|e| format!("failed to create companion window: {e}"))?;

    // The builder's focused(false) covers creation; an explicitly summoned
    // companion (command/tray) SHOULD come forward, but gently — show +
    // focus after the fact without unminimize churn on first open.
    let _ = window.show();

    Ok(CompanionWindowInfo {
        label: COMPANION_WINDOW_LABEL.to_string(),
    })
}

/// Frozen contract: `open_companion_window() -> { label }`.
#[tauri::command]
pub async fn open_companion_window(app: tauri::AppHandle) -> Result<CompanionWindowInfo, String> {
    open_companion_window_inner(&app)
}

/// Frozen contract: `set_companion_always_on_top(enabled) -> ()`. Only ever
/// acts on the `companion` window; errors when it is not open (the checkbox
/// only exists inside that window, so this is a "window closed mid-toggle"
/// race, not a user-reachable state).
#[tauri::command]
pub async fn set_companion_always_on_top(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let window = app
        .get_webview_window(COMPANION_WINDOW_LABEL)
        .ok_or_else(|| "companion window is not open".to_string())?;
    window
        .set_always_on_top(enabled)
        .map_err(|e| format!("failed to toggle companion always-on-top: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn companion_label_and_route_are_stable_contract() {
        // The frontend (companionBridge.ts + App.tsx route) and the ACL
        // capability files hardcode these strings; changing them is a
        // cross-stack break, so the contract is pinned here.
        assert_eq!(COMPANION_WINDOW_LABEL, "companion");
        assert_eq!(COMPANION_ROUTE, "/companion");
        assert_eq!(COMPANION_PROMPT_EVENT, "shannon:companion-prompt");
        assert!(COMPANION_WINDOW_LABEL != "main");
        assert!(!COMPANION_WINDOW_LABEL.starts_with("session-"));
    }

    #[test]
    fn companion_window_info_serializes_camel_case() {
        let info = CompanionWindowInfo {
            label: "companion".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert_eq!(json, r#"{"label":"companion"}"#);
    }

    /// Window-creation behavior that does not need a live AppHandle is
    /// pinned as constants (the builder path itself needs the tauri test
    /// runtime; the session-window precedent tests the same pure surface).
    #[test]
    fn companion_window_geometry_matches_brief() {
        assert_eq!(COMPANION_WINDOW_SIZE, (420.0, 320.0));
        assert!(COMPANION_WINDOW_MIN_SIZE.0 <= COMPANION_WINDOW_SIZE.0);
        assert!(COMPANION_WINDOW_MIN_SIZE.1 <= COMPANION_WINDOW_SIZE.1);
    }
}
