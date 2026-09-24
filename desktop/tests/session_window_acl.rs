//! P1-1 — compiled-in ACL contract for the session multi-window feature,
//! extended by review §P2-21 with the application-level ACL.
//!
//! `desktop/build.rs` generates `acl-manifests.json` + `capabilities.json`
//! from `desktop/capabilities/*.json` and `acl/app-permissions.json`, and
//! `tauri::generate_context!` bakes the resolved ACL into the binary.
//! These tests compile the app's real context and assert the exact
//! runtime-visible permission surface:
//!
//! - the JS event API (`plugin:event|listen` etc.) is allowed on `main`
//!   and on `session-*` windows — without this, streaming `query:*` events
//!   never reach any webview;
//! - window title sync (`plugin:window|set_title`) is allowed;
//! - dialog open/save are granted via the dedicated `file-dialogs`
//!   capability (review §P1-11);
//! - every application command is allowed on `main` and on `session-*`
//!   windows (review §P2-21: the `__app-acl__` manifest flips
//!   `has_app_manifest` to true, so app commands are ACL-checked and MUST
//!   be granted — file-level coverage is enforced by
//!   `app_command_acl_coverage.rs`, this is the compiled-context check);
//! - nothing else leaked: notification/shell/window-close stay denied, and
//!   unknown window labels / remote origins are denied everything. The
//!   app ships no remote-content windows (tauri.conf.json has no remote
//!   URLs), so remote denials are the origin-scoping half of §P2-21.

use tauri::ipc::Origin;

#[allow(dead_code)]
mod common;

use common::handler_inventory;

const MAIN: &str = "main";
const SESSION: &str = "session-00000000-0000-0000-0000-000000000000";

/// One `tauri::Context` per check — `generate_context!` embeds the ACL
/// resolved from the build script's OUT_DIR artifacts, so assertions run
/// against exactly what production will enforce.
fn allows(cmd: &str, window: &str, origin: Origin) -> bool {
    let mut context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    context
        .runtime_authority_mut()
        .resolve_access(cmd, window, window, &origin)
        .is_some()
}

fn allowed(cmd: &str, window: &str) -> bool {
    allows(cmd, window, Origin::Local)
}

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
fn plugin_surface_stays_minimal_beyond_event_window_dialog() {
    for window in [MAIN, SESSION] {
        // review §P1-11: dialog open/save are granted via the dedicated
        // `file-dialogs` capability, so the seven frontend call sites
        // (ChatInput attachments, session export, artifact save,
        // welcome/editor dir picker, persona-pack import/export) work.
        assert!(allowed("plugin:dialog|open", window), "dialog on {window}");
        assert!(
            allowed("plugin:dialog|save", window),
            "dialog save on {window}"
        );
        // Window close is done through the `close_session_window` app
        // command, so the raw window API stays closed. Shell and
        // notification plugin commands are not used by the frontend.
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

/// review §P2-21 — the full `generate_handler!` inventory must resolve on
/// both window groups against the compiled context. This is the runtime
/// twin of `app_command_acl_coverage.rs`' file-level sweep: it proves the
/// `__app-acl__` manifest + `app-commands` capability actually flowed
/// through `generate_context!` into the `RuntimeAuthority`.
#[test]
fn every_app_command_allowed_on_main_and_session_windows() {
    let inventory = handler_inventory();
    assert!(
        inventory.len() >= 200,
        "inventory implausibly small ({}): parser regression, see tests/common/mod.rs",
        inventory.len()
    );
    // single context: 255 commands x 2 windows against one compiled ACL
    let mut context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    let authority = context.runtime_authority_mut();
    for cmd in &inventory {
        for window in [MAIN, SESSION] {
            assert!(
                authority
                    .resolve_access(cmd, window, window, &Origin::Local)
                    .is_some(),
                "app command `{cmd}` is denied on window `{window}` — ACL coverage regressed; \
                 extend acl/app-permissions.json + capabilities/app-commands.json"
            );
        }
    }
}

/// review §P2-21 — origin/window scoping is the point of the app ACL:
/// unknown window labels (no capability matches) and remote origins (the
/// app loads no remote content) are denied everything, app commands and
/// plugin commands alike.
#[test]
fn unknown_windows_and_remote_origins_are_denied_everything() {
    let remote_url = "https://untrusted.example/".parse().expect("static URL");
    for (cmd, window) in [
        ("send_message", MAIN),
        ("send_message", SESSION),
        ("open_session_window", MAIN),
        ("close_session_window", SESSION),
        ("plugin:event|listen", MAIN),
        ("plugin:dialog|open", SESSION),
    ] {
        let remote = Origin::Remote {
            url: remote_url.clone(),
        };
        assert!(
            !allows(cmd, window, remote),
            "`{cmd}` must stay denied for remote origins"
        );
    }
    for (cmd, window) in [
        ("send_message", "untrusted-window"),
        ("get_config", "untrusted-window"),
        ("plugin:event|listen", "untrusted-window"),
        ("plugin:dialog|open", "untrusted-window"),
    ] {
        assert!(
            !allows(cmd, window, Origin::Local),
            "`{cmd}` must stay denied on unknown window label `{window}`"
        );
    }
}
