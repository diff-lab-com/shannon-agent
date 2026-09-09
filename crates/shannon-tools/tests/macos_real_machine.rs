//! macOS real-machine QA harness (roadmap A2/A3).
//!
//! Closes the verification debt tracked in
//! `docs/plans/2026-09-08-followups-roadmap.md` (A2: applescript real
//! execution, A3: computer-use runtime verification) and mirrors the manual
//! checklist in `docs/qa/2026-09-07-computer-use-browser-qa-checklist.md`
//! (QA-1). Everything here needs a real macOS session — TCC prompts, a
//! window server, actual displays — so every test is `#[ignore]`d and the
//! file is `cfg`-gated to macOS. Run on a Mac with:
//!
//! ```text
//! cargo test -p shannon-tools --features computer-use \
//!   --test macos_real_machine -- --ignored --nocapture
//! ```
//!
//! TCC notes: the Automation prompt is attributed to the *responsible
//! process* (the terminal/IDE hosting the cargo run), so the first run of
//! each app-targeting test needs a human to click Allow. The
//! `computer_type_lands_in_textedit` test additionally requires the host app
//! to hold the Accessibility (Input Monitoring-style) grant; when it is
//! missing, CGEvent posts are silently dropped and the test fails with an
//! explicit diagnostic — that silent-drop mode is itself documented behavior
//! (the known "no preflight" gap).

#![cfg(all(target_os = "macos", feature = "computer-use"))]

use shannon_tools::{AppleScriptTool, ComputerUseTool, Tool};

// C-level check of the host app's TCC Accessibility grant. Declared here
// rather than pulled in via a dependency so the QA harness stays
// dependency-free; ApplicationServices is a system umbrella framework.
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
}

fn accessibility_granted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

// ---------------------------------------------------------------------------
// QA-1: applescript tool (roadmap A2)
// ---------------------------------------------------------------------------

/// QA-1 #1 — pure script, no target app, no TCC involved.
#[tokio::test]
#[ignore]
async fn applescript_pure_script_returns_result() {
    let tool = AppleScriptTool::new();
    let result = tool
        .execute(serde_json::json!({"script": "return 1+1"}))
        .await
        .expect("execute should not fail at the transport level");
    assert!(!result.is_error, "unexpected error: {}", result.content);
    assert!(result.content.contains('2'), "got: {}", result.content);
    println!("[ok] return 1+1 -> {}", result.content.trim());
}

/// QA-1 #4 — JXA path.
#[tokio::test]
#[ignore]
async fn applescript_jxa_returns_result() {
    let tool = AppleScriptTool::new();
    let result = tool
        .execute(serde_json::json!({
            "target": "applescript",
            "language": "JavaScript",
            "script": "1+1"
        }))
        .await
        .unwrap();
    assert!(!result.is_error, "unexpected error: {}", result.content);
    assert!(result.content.contains('2'), "got: {}", result.content);
    println!("[ok] JXA 1+1 -> {}", result.content.trim());
}

/// QA-1 #7 — 30s ceiling on a hanging script. The timeout surfaces as
/// `ToolError::ExecutionFailed` (not an `is_error` ToolOutput like the
/// osascript nonzero-exit path — both render as tool failures to the model).
#[tokio::test]
#[ignore]
async fn applescript_timeout_is_an_error() {
    let tool = AppleScriptTool::new();
    let result = tool
        .execute(serde_json::json!({"script": "delay 60"}))
        .await;
    match result {
        Ok(out) => {
            assert!(out.is_error, "delay 60 must surface as an error");
            assert!(
                out.content.contains("timed out after 30s"),
                "got: {}",
                out.content
            );
        }
        Err(shannon_tools::ToolError::ExecutionFailed(msg)) => {
            assert!(msg.contains("timed out after 30s"), "got: {msg}");
            println!("[ok] timeout surfaced as ExecutionFailed: {msg}");
        }
        Err(other) => panic!("unexpected error shape: {other}"),
    }
}

/// QA-1 #2 — app-targeting script; first run triggers the Automation TCC
/// prompt for Notes. Passes only once the grant exists (or the user allows).
#[tokio::test]
#[ignore]
async fn applescript_notes_automation_tcc() {
    let tool = AppleScriptTool::new();
    let result = tool
        .execute(serde_json::json!({
            "script": "tell application \"Notes\" to count notes"
        }))
        .await
        .unwrap();
    assert!(
        !result.is_error,
        "Notes automation failed (denied TCC yields osascript -1743): {}",
        result.content
    );
    println!("[ok] Notes note count: {}", result.content.trim());
}

/// QA-1 #3 (positive half, env-gated): run with the target app's Automation
/// grant switched OFF in System Settings → the tool must surface osascript's
/// "not authorized" (-1743) failure instead of hanging or pretending
/// success. Usage:
/// `SHANNON_QA_DENIED_APP=Notes cargo test ... --ignored applescript_denied_app_reports_not_authorized`
#[tokio::test]
#[ignore]
async fn applescript_denied_app_reports_not_authorized() {
    let app = std::env::var("SHANNON_QA_DENIED_APP").unwrap_or_default();
    assert!(!app.is_empty(), "set SHANNON_QA_DENIED_APP=<AppName>");
    let tool = AppleScriptTool::new();
    let result = tool
        .execute(serde_json::json!({
            "script": format!("tell application \"{app}\" to count notes")
        }))
        .await
        .unwrap();
    assert!(
        result.is_error,
        "denied app must error, got: {}",
        result.content
    );
    assert!(
        result.content.contains("-1743") || result.content.contains("not allow"),
        "error should carry not-authorized semantics, got: {}",
        result.content
    );
    println!("[ok] denied-app error: {}", result.content.trim());
}

/// QA-1 #5 — Shortcuts path, env-gated so we never execute an arbitrary
/// user shortcut: `SHANNON_QA_SHORTCUT=<name> cargo test ... --ignored`
#[tokio::test]
#[ignore]
async fn applescript_shortcuts_run_named() {
    let name = std::env::var("SHANNON_QA_SHORTCUT").unwrap_or_default();
    assert!(!name.is_empty(), "set SHANNON_QA_SHORTCUT=<shortcut name>");
    let tool = AppleScriptTool::new();
    let result = tool
        .execute(serde_json::json!({"target": "shortcuts", "name": name}))
        .await
        .unwrap();
    assert!(!result.is_error, "shortcut run failed: {}", result.content);
    println!("[ok] shortcut output: {}", result.content.trim());
}

// ---------------------------------------------------------------------------
// A3: computer tool runtime verification (xcap capture + enigo input)
// ---------------------------------------------------------------------------

/// Screenshot: real pixels, downscaled into the 1024x768 reference frame.
/// Set `SHANNON_QA_DUMP=/tmp/x.png` to also write the payload for eyeballing.
#[tokio::test]
#[ignore]
async fn computer_screenshot_captures_real_screen() {
    let tool = ComputerUseTool::new();
    let result = tool
        .execute(serde_json::json!({"action": "screenshot"}))
        .await
        .unwrap();
    assert!(!result.is_error, "screenshot failed: {}", result.content);

    let (w, h) = (
        result.metadata["width"].as_u64().unwrap(),
        result.metadata["height"].as_u64().unwrap(),
    );
    assert!(w <= 1024 && h <= 768, "dims {w}x{h} exceed reference frame");
    println!("[ok] screenshot downscaled to {w}x{h}");

    // Real screens compress to well over 30KB at 1024x768; a uniform frame
    // (the classic missing-Screen-Recording symptom) does not.
    use base64::Engine as _;
    let data = result.metadata["data"].as_str().unwrap();
    let png = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap();
    assert!(
        png.starts_with(&[0x89, b'P', b'N', b'G']),
        "payload is not a PNG"
    );
    println!(
        "[info] payload {} bytes ({}KB)",
        png.len(),
        png.len() / 1024
    );
    assert!(
        png.len() > 30 * 1024,
        "PNG suspiciously small ({}) — blank frame? check Screen Recording grant",
        png.len()
    );

    if let Ok(dump) = std::env::var("SHANNON_QA_DUMP") {
        std::fs::write(&dump, &png).unwrap();
        println!("[info] dumped payload to {dump}");
    }
}

/// Full input chain: enigo typing must land in the frontmost app. TextEdit
/// is driven via AppleScript (activate + fresh document), the marker is
/// typed through the `computer` tool, then read back via AppleScript.
/// Requires the Accessibility grant on the hosting app; without it CGEvents
/// are silently dropped and the mismatch below is the documented failure
/// mode (no preflight exists yet).
#[tokio::test]
#[ignore]
async fn computer_type_lands_in_textedit() {
    let ax = accessibility_granted();
    println!("[info] AXIsProcessTrusted = {ax}");

    let script_tool = AppleScriptTool::new();
    let setup = script_tool
        .execute(serde_json::json!({
            "script": "tell application \"TextEdit\"\nactivate\nif (count of documents) is 0 then make new document\nset text of document 1 to \"\"\nend tell"
        }))
        .await
        .unwrap();
    assert!(!setup.is_error, "TextEdit setup failed: {}", setup.content);

    let marker = format!("shannon-qa-{}", std::process::id());
    let computer = ComputerUseTool::new();
    let typed = computer
        .execute(serde_json::json!({"action": "type", "text": marker}))
        .await
        .unwrap();
    assert!(!typed.is_error, "type action failed: {}", typed.content);

    let readback = script_tool
        .execute(serde_json::json!({
            "script": "tell application \"TextEdit\" to get text of document 1"
        }))
        .await
        .unwrap();
    let text = readback.content.trim().to_string();
    if !ax && !text.contains(&marker) {
        panic!(
            "CONFIRMED known gap: Accessibility grant missing, so CGEvent input was \
             silently dropped (type reported success, TextEdit got {text:?}). \
             Grant Accessibility to the hosting app and re-run."
        );
    }
    assert!(
        text.contains(&marker),
        "marker not found in TextEdit (got {text:?}) — AX trusted: {ax}"
    );
    println!("[ok] typed marker {marker} landed in TextEdit");
}

/// Click: the action must succeed against the real window server; the
/// effect (what received the click) is intentionally not asserted — the
/// type test above proves the event pipeline end to end. Coordinates are
/// given in the 1024x768 reference frame (center) — the same space the
/// model uses.
#[tokio::test]
#[ignore]
async fn computer_click_succeeds() {
    let computer = ComputerUseTool::new();
    let result = computer
        .execute(serde_json::json!({
            "action": "click",
            "coordinate": [512, 384]
        }))
        .await
        .unwrap();
    assert!(!result.is_error, "click failed: {}", result.content);
    println!(
        "[ok] click at reference (512, 384): {}",
        result.content.trim()
    );
}

/// Diagnostic: what does xcap enumerate on this host, and do the reported
/// logical dims match the captured pixel dims (i.e. is the click-scaling
/// contract's screen_size() leg healthy)?
#[tokio::test]
#[ignore]
async fn debug_monitor_enumeration() {
    match xcap::Monitor::all() {
        Ok(monitors) => {
            println!("[info] monitors: {}", monitors.len());
            for m in &monitors {
                println!(
                    "[info] id={:?} name={:?} {}x{} scale={:?}",
                    m.id(),
                    m.name(),
                    m.width()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|e| format!("ERR:{e}")),
                    m.height()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|e| format!("ERR:{e}")),
                    m.scale_factor()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|e| format!("ERR:{e}")),
                );
            }
        }
        Err(e) => println!("[info] Monitor::all() failed: {e}"),
    }
}
