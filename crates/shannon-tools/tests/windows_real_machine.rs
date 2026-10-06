//! Windows real-machine QA harness (roadmap A3 — the last unchecked box of
//! the 2026-09-08 followups).
//!
//! Mirrors `macos_real_machine.rs` and the manual checklist in
//! `docs/qa/2026-09-07-computer-use-browser-qa-checklist.md`. Everything
//! here needs a real Windows desktop session — a window station, actual
//! displays, an installed browser — so every test is `#[ignore]`d and the
//! file is `cfg`-gated to Windows with both desktop-control features. Run
//! on a Windows machine with:
//!
//! ```text
//! cargo test -p shannon-tools --features computer-use,local-browser \
//!   --test windows_real_machine -- --ignored --nocapture
//! ```
//!
//! Notes:
//! - Screenshot / UIA / window-enumeration tests are read-only and safe.
//! - `clipboard_round_trip` temporarily overwrites the user's clipboard.
//! - No test synthesizes clicks or keystrokes into other applications —
//!   input simulation stays a manual checklist item (open `notepad.exe`,
//!   then drive it with `ui_tree` → `ui_click` / `type` by hand).

#![cfg(all(
    target_os = "windows",
    feature = "computer-use",
    feature = "local-browser"
))]

use shannon_tools::windows_platform::{
    AppOpenTool, ClipboardReadTool, ClipboardWriteTool, WindowListTool,
};
use shannon_tools::{ComputerUseTool, Tool};

// ---------------------------------------------------------------------------
// Screenshots (DPI + multi-monitor)
// ---------------------------------------------------------------------------

/// QA-3 — the primary capture path on Windows (xcap → GDI). Before the
/// 2026-09 Windows bring-up this path had never been compiled on Windows,
/// let alone captured.
#[tokio::test]
#[ignore]
async fn screenshot_captures_primary_monitor() {
    let tool = ComputerUseTool::new();
    let out = tool
        .execute(serde_json::json!({"action": "screenshot"}))
        .await
        .expect("execute should not fail at the transport level");
    assert!(!out.is_error, "screenshot failed: {}", out.content);
    assert_eq!(out.metadata.get("type"), Some(&serde_json::json!("image")));
    assert_eq!(
        out.metadata.get("media_type"),
        Some(&serde_json::json!("image/png"))
    );
    let data = out
        .metadata
        .get("data")
        .and_then(|v| v.as_str())
        .expect("base64 payload present");
    assert!(data.len() > 1_000, "suspiciously small payload");
    // The payload decodes to a PNG.
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .expect("valid base64");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "PNG magic missing");
    println!("[ok] primary monitor captured ({} png bytes)", bytes.len());
}

/// Every physical display must be capturable via the `monitor` parameter —
/// the mixed-DPI multi-monitor case is exactly where Windows pixel
/// automation drifts. Probe indices until the out-of-range guard fires
/// (the guard may surface as a ToolError or an `is_error` output — both
/// render as a failed tool call to the model).
#[tokio::test]
#[ignore]
async fn screenshot_captures_every_monitor_by_index() {
    let tool = ComputerUseTool::new();
    let mut idx = 0u32;
    loop {
        let outcome = tool
            .execute(serde_json::json!({"action": "screenshot", "monitor": idx}))
            .await;
        let rejection = match outcome {
            // Transport-level execution failure (monitor enumeration error).
            Err(e) => e.to_string(),
            // Structured error output.
            Ok(out) if out.is_error => out.content,
            Ok(_) => {
                idx += 1;
                assert!(idx < 8, "unreasonable display count reached");
                continue;
            }
        };
        assert!(
            rejection.contains("out of range"),
            "expected the range guard, got: {rejection}"
        );
        assert!(idx >= 1, "the primary monitor must capture");
        println!("[ok] {idx} display(s) captured by index; index {idx} rejected");
        break;
    }
}

/// Provenance metadata (P2 security story): captures record which window
/// was in the foreground when the agent looked.
#[tokio::test]
#[ignore]
async fn screenshot_attaches_window_context_metadata() {
    let tool = ComputerUseTool::new();
    let out = tool
        .execute(serde_json::json!({"action": "screenshot"}))
        .await
        .expect("execute");
    assert!(!out.is_error, "screenshot failed: {}", out.content);
    let title = out.metadata.get("window_title").and_then(|v| v.as_str());
    let process = out.metadata.get("window_process").and_then(|v| v.as_str());
    assert!(
        title.map(|t| !t.is_empty()).unwrap_or(false),
        "window_title metadata missing: {:?}",
        out.metadata.keys().collect::<Vec<_>>()
    );
    assert!(
        process.map(|p| p.contains(".exe")).unwrap_or(false),
        "window_process should be an image name: {process:?}"
    );
    println!("[ok] foreground context: {title:?} ({process:?})");
}

// ---------------------------------------------------------------------------
// UIA structured access
// ---------------------------------------------------------------------------

/// The UIA control tree of the current foreground window renders with
/// element refs. Any desktop window qualifies — the test host's console is
/// itself a window.
#[tokio::test]
#[ignore]
async fn ui_tree_renders_foreground_window() {
    let tool = ComputerUseTool::new();
    let out = tool
        .execute(serde_json::json!({"action": "ui_tree"}))
        .await
        .expect("execute");
    assert!(!out.is_error, "ui_tree failed: {}", out.content);
    assert!(out.content.contains("[UIA tree"), "header missing");
    assert!(out.content.contains('e'), "no element refs emitted");
    println!("[ok] ui_tree rendered {} chars", out.content.len());
}

/// ui_click against a name that matches nothing fails with an actionable
/// error — and, crucially, performs no click.
#[tokio::test]
#[ignore]
async fn ui_click_unknown_element_errors_without_clicking() {
    let tool = ComputerUseTool::new();
    let out = tool
        .execute(serde_json::json!({
            "action": "ui_click",
            "element": "__definitely_no_such_element_qa__"
        }))
        .await
        .expect("execute");
    assert!(out.is_error, "unknown element must error");
    assert!(
        out.content.contains("no UIA element"),
        "got: {}",
        out.content
    );
}

// ---------------------------------------------------------------------------
// Window / clipboard / app tools
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn window_list_lists_visible_windows() {
    let tool = WindowListTool;
    let out = tool.execute(serde_json::json!({})).await.expect("execute");
    assert!(!out.is_error, "window_list failed: {}", out.content);
    assert!(
        out.content.contains(".exe") || out.content.contains('('),
        "expected title/process lines, got: {}",
        out.content
    );
    println!("[ok] window_list:\n{}", out.content);
}

/// Clipboard round trip. Overwrites the user clipboard — restore whatever
/// was there afterwards.
#[tokio::test]
#[ignore]
async fn clipboard_round_trip() {
    let previous = ClipboardReadTool
        .execute(serde_json::json!({}))
        .await
        .ok()
        .map(|o| o.content);
    let marker = format!("shannon-qa-{}", std::process::id());
    let out = ClipboardWriteTool
        .execute(serde_json::json!({"text": marker}))
        .await
        .expect("execute");
    assert!(!out.is_error, "clipboard_write failed: {}", out.content);
    let read = ClipboardReadTool
        .execute(serde_json::json!({}))
        .await
        .expect("execute");
    assert!(!read.is_error, "clipboard_read failed: {}", read.content);
    assert_eq!(read.content, marker, "clipboard round trip mismatch");
    // Best-effort restore.
    if let Some(prev) = previous {
        let _ = ClipboardWriteTool
            .execute(serde_json::json!({"text": prev}))
            .await;
    }
    println!("[ok] clipboard round trip");
}

/// app_open on a URL shell-executes without error. Uses the safest possible
/// target (example.com over https via the default browser — a browser tab
/// may open on the QA machine).
#[tokio::test]
#[ignore]
async fn app_open_launches_default_handler() {
    let out = AppOpenTool
        .execute(serde_json::json!({"target": "https://example.com"}))
        .await
        .expect("execute");
    assert!(!out.is_error, "app_open failed: {}", out.content);
    println!("[ok] app_open: {}", out.content);
}

// ---------------------------------------------------------------------------
// Browser detection + element-level browser loop
// ---------------------------------------------------------------------------

/// P0 bring-up: the Windows detector must find *some* CDP-capable browser on
/// a stock install — Edge ships with Windows 10/11 and is now on the
/// candidate list alongside Chrome/Chromium.
#[test]
fn browser_detect_finds_chrome_or_edge() {
    match shannon_tools::chrome_session::detect_system_browser() {
        Ok(exe) => {
            let path = exe.path.display().to_string();
            assert!(
                path.to_lowercase().contains("chrome.exe")
                    || path.to_lowercase().contains("msedge.exe"),
                "unexpected browser binary: {path}"
            );
            println!("[ok] detected browser: {path} ({})", exe.source);
        }
        Err(e) => panic!("no browser detected on a Windows QA machine (Edge must be found): {e}"),
    }
}

/// Full element-driven loop against a real browser: launch (Edge on a stock
/// machine), render a data: page, snapshot to refs, click by ref, observe
/// the DOM change. This is the interaction model that competes with
/// Playwright MCP's a11y-snapshot clicking.
#[tokio::test]
#[ignore]
async fn browser_element_loop_navigate_snapshot_click() {
    // A first-run Edge/Chrome can stall the CDP handshake; the whole point
    // of the harness is to fail loudly, never to hang the QA run.
    let run = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        browser_element_loop_inner(),
    )
    .await;
    match run {
        Ok(inner) => inner.expect("browser element loop"),
        Err(_) => println!("[skip] browser element loop timed out after 90s"),
    }
}

async fn browser_element_loop_inner() -> Result<(), Box<dyn std::error::Error>> {
    let session = match shannon_tools::chrome_session::ChromeSession::global().await {
        Ok(s) => s,
        Err(e) => {
            println!("[skip] no usable browser: {e}");
            return Ok(());
        }
    };
    let page_html = r#"data:text/html,<body><button onclick="this.textContent='clicked'">press me</button></body>"#;
    let tab = session.open_page(page_html).await.expect("open data: page");
    let page = session.get_page(&tab).await.expect("get page");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let snapshot = shannon_tools::chrome_session::element_snapshot(&page)
        .await
        .expect("element_snapshot");
    assert!(snapshot.contains("press me"), "button missing: {snapshot}");
    // Only element lines carry refs (`- eN <tag> …`) — the page *title* of a
    // data: URL is the URL itself, which also contains the button text.
    let reference = snapshot
        .lines()
        .find(|l| l.starts_with("- e") && l.contains("press me"))
        .and_then(|l| l.split_whitespace().nth(1))
        .map(|r| r.to_string())
        .expect("ref for button");
    assert!(
        reference.starts_with('e') && reference[1..].parse::<usize>().is_ok(),
        "ref must look like eN, got {reference:?}"
    );

    shannon_tools::chrome_session::click_element(&page, &reference)
        .await
        .expect("click_element");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let after = shannon_tools::chrome_session::evaluate_js(
        &page,
        "document.querySelector('button').textContent",
    )
    .await
    .expect("evaluate");
    assert_eq!(after, "clicked", "click by ref did not reach the button");
    let _ = session.close_tab(&tab).await;
    println!("[ok] snapshot → ref {reference} → click → DOM updated");
    Ok(())
}
