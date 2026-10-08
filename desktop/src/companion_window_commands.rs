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
//!   window (356x260 per the design spec, opens unfocused so it never steals
//!   the user's typing) or focuses the existing one (dedupe by fixed label);
//! - [`set_companion_always_on_top`]`(enabled) -> ()` — the checkbox in the
//!   companion UI toggles the flag through this command instead of the JS
//!   window API, keeping the ACL surface one plain app command (same flow as
//!   every other command; no extra plugin permission for the window group);
//! - [`hide_companion_window`]`() -> ()` — the Esc half of the design-13
//!   window contract (「失焦自动收起 · Esc 关闭」); same Rust-command
//!   rationale as the always-on-top toggle.
//!
//! Window contract (design 13:105): the blur half is handled in `main.rs`'s
//! `on_window_event` — a `Focused(false)` on the `companion` label hides the
//! window. [`should_hide_on_blur`] arms only after a short grace window past
//! the last open/summon, because some platforms emit a spurious
//! `Focused(false)` right after an unfocused-at-creation window is shown;
//! hiding then would make the first summon look like a no-op.
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
use std::sync::Mutex;
use std::time::{Duration, Instant};
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

/// Window geometry: 356x260 per the design spec (13:102「356 × 260」),
/// bounded below so the capture box stays usable.
const COMPANION_WINDOW_SIZE: (f64, f64) = (356.0, 260.0);
const COMPANION_WINDOW_MIN_SIZE: (f64, f64) = (280.0, 200.0);

/// Fresh-creation guard for blur-to-hide: some platforms emit a spurious
/// `Focused(false)` right after an unfocused-at-creation window is shown;
/// hiding then would make the very first summon look like a no-op. The
/// blur-to-hide handler (main.rs `on_window_event`) only arms after this
/// grace window has passed since the last open/summon.
const BLUR_HIDE_GRACE: Duration = Duration::from_millis(750);

/// Timestamp of the last companion open/summon (`None` = never opened, in
/// which case blur-to-hide stays disarmed). Const `Mutex::new` — same
/// pattern as the `ENV_PROVIDER_CACHE` precedent in commands_config.
static OPENED_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// Record an open/summon of the companion window (called from
/// [`open_companion_window_inner`], the single creation/focus path).
fn note_companion_opened() {
    let mut opened = OPENED_AT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *opened = Some(Instant::now());
}

/// Whether a `Focused(false)` on the companion window right now should hide
/// it. Disarmed within `BLUR_HIDE_GRACE` of the last open/summon (see the
/// const's doc) and when the window was never opened through this module.
pub fn should_hide_on_blur() -> bool {
    let opened = OPENED_AT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match *opened {
        Some(at) => at.elapsed() >= BLUR_HIDE_GRACE,
        None => false,
    }
}

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
/// [`open_companion_window`] command, the tray「Quick Capture」entry and the
/// `open-companion` global shortcut (design 13:⌘⇧Space).
pub fn open_companion_window_inner(app: &tauri::AppHandle) -> Result<CompanionWindowInfo, String> {
    // Arm the blur-to-hide grace window before anything else — the spurious
    // `Focused(false)` we guard against fires right after `build()`.
    note_companion_opened();

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

/// Frozen contract: `hide_companion_window() -> ()`. The Esc path in the
/// companion UI (design 13:「失焦自动收起 · Esc 关闭」) goes through this
/// command instead of the JS window API — same rationale as
/// [`set_companion_always_on_top`]: window management stays on the Rust
/// side and the companion capability stays event-only. Hides rather than
/// closes, so the next ⌘⇧Space / tray summon is instant; errors when the
/// window is already gone ("Esc twice" race, not user-reachable state).
#[tauri::command]
pub async fn hide_companion_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window(COMPANION_WINDOW_LABEL)
        .ok_or_else(|| "companion window is not open".to_string())?;
    window
        .hide()
        .map_err(|e| format!("failed to hide companion window: {e}"))
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
    fn companion_window_geometry_matches_design() {
        // Design 13:102 —「伴随窗 · 置顶小窗 · 356 × 260」.
        assert_eq!(COMPANION_WINDOW_SIZE, (356.0, 260.0));
        assert!(COMPANION_WINDOW_MIN_SIZE.0 <= COMPANION_WINDOW_SIZE.0);
        assert!(COMPANION_WINDOW_MIN_SIZE.1 <= COMPANION_WINDOW_SIZE.1);
    }

    /// Blur-to-hide arming (main.rs consumes this before hiding on
    /// `Focused(false)`): disarmed before any open, armed only after the
    /// grace window passes the recorded open instant.
    #[test]
    fn blur_hide_arms_only_after_grace_past_open() {
        // Never opened → stay disarmed.
        // (Tests within the suite share the process-global OPENED_AT; this
        // assertion only holds before note_companion_opened runs, so order
        // it first and have the later half of the test write the instant.)
        assert!(!should_hide_on_blur());

        // Simulate an open in the past (beyond the grace window): armed.
        {
            let mut opened = OPENED_AT
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *opened = Some(Instant::now() - BLUR_HIDE_GRACE - Duration::from_millis(50));
        }
        assert!(should_hide_on_blur());

        // Fresh open (now): disarmed by the grace window.
        note_companion_opened();
        assert!(!should_hide_on_blur());
    }
}
