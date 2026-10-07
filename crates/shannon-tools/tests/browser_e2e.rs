//! End-to-end browser integration test (T14 Phase 1).
//!
//! Drives the real system Chrome/Chromium through the same session layer
//! the builtin tools use: launch → navigate → title/text → screenshot →
//! click/type → close. **Self-skips** when no compatible browser is
//! detected (ubuntu CI runners have none) so the default gate stays green.
//!
//! Run locally with:
//! ```sh
//! cargo test -p shannon-tools --features local-browser --test browser_e2e -- --nocapture
//! ```

#![cfg(feature = "local-browser")]

use shannon_tools::chrome_session::{self, ChromeSession};

fn browser_available() -> bool {
    // Reuse the session's own detection (platform candidate lists live in
    // shannon-browser::detect). SHANNON_BROWSER_PATH lets a developer point
    // at a specific binary.
    std::env::var_os("SHANNON_BROWSER_PATH").is_some()
        || chrome_session::detect_system_browser().is_ok()
}

/// The ORIGINAL flow: navigate → title/text → screenshot → click/type →
/// console → tabs. Network-dependent; the https step self-skips offline.
async fn scenario_network_flow(
    session: &std::sync::Arc<ChromeSession>,
) -> Option<shannon_tools::chrome_session::TabId> {
    // 1) Navigate.
    let tab = match session.open_page("https://example.com").await {
        Ok(tab) => tab,
        Err(e) => {
            // Offline hosts (air-gapped CI) can't reach example.com — that's
            // an environment property, not a session bug.
            println!("network flow skipped: no route to example.com ({e})");
            return None;
        }
    };
    let page = session.get_page(&tab).await.expect("page handle");

    // 2) Title + text snapshot.
    chrome_session::navigate(&page, "https://example.com")
        .await
        .expect("navigate");
    let text = chrome_session::page_text(&page).await.expect("page_text");
    assert!(
        text.contains("Example Domain"),
        "snapshot should contain the page title, got: {text}"
    );

    // 3) Screenshot (viewport) — non-trivial PNG.
    let png = chrome_session::screenshot_png(&page, false)
        .await
        .expect("screenshot");
    assert!(
        png.len() > 1000,
        "screenshot should be a real PNG, got {} bytes",
        png.len()
    );
    assert_eq!(&png[1..4], b"PNG", "PNG magic bytes");

    // 4) Full-page screenshot accepts the flag.
    let full = chrome_session::screenshot_png(&page, true)
        .await
        .expect("full page");
    assert!(!full.is_empty());

    // 5) Click + type: exercise the code paths.
    chrome_session::click_at(&page, 100.0, 100.0)
        .await
        .expect("click");
    let _ = chrome_session::type_text(&page, "hello").await;

    // 6) Console listener: inject a console.log and expect it in the
    //    session buffer (Runtime.consoleAPICalled → listener task).
    page.evaluate("console.log('e2e-console-marker')")
        .await
        .expect("evaluate console.log");
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let msgs = session.console_messages(&tab).await;
        if msgs.iter().any(|m| m.contains("e2e-console-marker")) {
            break;
        }
    }
    let msgs = session.console_messages(&tab).await;
    assert!(
        msgs.iter().any(|m| m.contains("e2e-console-marker")),
        "console buffer should contain the marker, got {msgs:?}"
    );

    // 7) Key dispatch: press Enter via the CDP key event path.
    chrome_session::press_key(&page, "Enter")
        .await
        .expect("press_key");

    // 8) Tabs listing includes our tab.
    let tabs = session.list_tabs().await;
    assert!(
        tabs.iter().any(|(id, _)| *id == tab),
        "tab should be listed"
    );
    Some(tab)
}

/// A page exercising every remote/mobile-delegation capability at once: a
/// dropdown, a hover target, an async-loading region, a file input, and
/// enough body text for wait_for. Encoded as a base64 data URL — a raw
/// `data:` URL would end at the first `#` in the CSS (fragment separator).
const FIXTURE_HTML: &str = r#"<html><head><title>fixture</title><style>
#menu { display: none; }
#trigger:hover + #menu { display: block; }
</style></head><body>
<h1>Fixture Page</h1>
<select id="ship"><option value="">choose</option><option value="ground">Ground</option><option value="air">Air</option></select>
<div id="trigger" role="button">hover me</div><div id="menu">menu revealed</div>
<div id="async"></div>
<button onclick="setTimeout(() => document.getElementById('async').innerText = 'LOADING DONE', 400)">load</button>
<input type="file" id="picker" />
<script>document.title = 'Fixture Ready';</script>
</body></html>"#;

fn fixture_url() -> String {
    use base64::Engine as _;
    format!(
        "data:text/html;charset=utf-8;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(FIXTURE_HTML)
    )
}

/// The remote/mobile-delegation scenarios against the hermetic fixture:
/// select_option / hover / wait_for / upload / jpeg+png screenshot / pdf /
/// wheel scroll / modifier key combos.
async fn scenario_remote_delegation(session: &std::sync::Arc<ChromeSession>) {
    let tab = session
        .open_page(&fixture_url())
        .await
        .expect("open fixture page");
    let page = session.get_page(&tab).await.expect("page");

    // ── browser_select_option ────────────────────────────────────────────
    // The element list labels a <select> with its options' text, not its id.
    let snapshot = chrome_session::element_snapshot(&page)
        .await
        .expect("snapshot");
    assert!(
        snapshot.contains("Ground"),
        "fixture must expose the dropdown: {snapshot}"
    );
    chrome_session::select_option(&page, "e1", "Air")
        .await
        .expect("select by label");
    let value = chrome_session::evaluate_js(&page, "document.getElementById('ship').value")
        .await
        .expect("read value");
    assert_eq!(value, "air", "selection by visible label must land");
    // Selecting by option value works too; a bogus option is an error.
    chrome_session::select_option(&page, "e1", "ground")
        .await
        .expect("select by value");
    assert!(
        chrome_session::select_option(&page, "e1", "teleportation")
            .await
            .is_err(),
        "unknown option must error"
    );

    // ── browser_hover ────────────────────────────────────────────────────
    // The menu is hidden until a real mousemove lands on the trigger.
    let before = chrome_session::evaluate_js(
        &page,
        "getComputedStyle(document.getElementById('menu')).display",
    )
    .await
    .expect("display before");
    assert_eq!(before, "none");
    chrome_session::hover_element(&page, "e2")
        .await
        .expect("hover trigger");
    let after = chrome_session::evaluate_js(
        &page,
        "getComputedStyle(document.getElementById('menu')).display",
    )
    .await
    .expect("display after");
    assert_eq!(after, "block", "hover must reveal the CSS :hover menu");

    // ── browser_wait_for ─────────────────────────────────────────────────
    // Click the load button, then wait for the async region instead of
    // polling screenshots — the remote/mobile progress-loop pattern.
    chrome_session::click_element(&page, "e3")
        .await
        .expect("click load");
    chrome_session::wait_for_text(&page, "LOADING DONE", std::time::Duration::from_secs(5))
        .await
        .expect("async text should appear");
    // A missing text times out (bounded, error not panic).
    assert!(
        chrome_session::wait_for_text(
            &page,
            "this text never appears",
            std::time::Duration::from_millis(600)
        )
        .await
        .is_err(),
        "timeout must produce an error"
    );

    // ── browser_upload ───────────────────────────────────────────────────
    let payload = b"hello upload".to_vec();
    chrome_session::upload_files(&page, 0, &[("notes.txt".to_string(), payload)])
        .await
        .expect("upload file");
    let observed = chrome_session::evaluate_js(
        &page,
        "(function(){ const f = document.getElementById('picker').files; return f.length + ':' + f[0].name; })()",
    )
    .await
    .expect("read files");
    assert_eq!(observed, "1:notes.txt", "file must be attached in-page");
    // An out-of-range input index errors instead of panicking.
    assert!(
        chrome_session::upload_files(&page, 7, &[("x.txt".to_string(), b"x".to_vec())])
            .await
            .is_err()
    );

    // ── browser_screenshot (png + jpeg) and browser_pdf ──────────────────
    let png = chrome_session::screenshot_with_format(&page, false, "png", 60)
        .await
        .expect("png screenshot");
    assert_eq!(&png[1..4], b"PNG", "png magic bytes");
    let jpeg = chrome_session::screenshot_with_format(&page, false, "jpeg", 60)
        .await
        .expect("jpeg screenshot");
    assert!(
        jpeg.len() > 2 && jpeg[0] == 0xFF && jpeg[1] == 0xD8,
        "jpeg must start with the SOI marker"
    );
    assert!(
        jpeg.len() < png.len(),
        "jpeg q60 should be smaller than png ({} vs {})",
        jpeg.len(),
        png.len()
    );
    assert!(
        chrome_session::screenshot_with_format(&page, false, "webp", 60)
            .await
            .is_err(),
        "unsupported formats are rejected up front"
    );
    let pdf = chrome_session::pdf_bytes(&page).await.expect("pdf bytes");
    assert!(pdf.len() > 500, "pdf should have real content");
    assert_eq!(&pdf[0..5], b"%PDF-", "pdf magic bytes");
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("fixture.pdf");
    std::fs::write(&out, &pdf).expect("write pdf");
    assert!(out.metadata().expect("pdf file").len() > 500);

    // ── browser_scroll (real wheel) + browser_press_key (combos) ─────────
    // Tall page so the wheel has something to scroll.
    chrome_session::evaluate_js(
        &page,
        "(function(){ const d = document.createElement('div'); d.style.height='4000px'; document.body.appendChild(d); return true; })()",
    )
    .await
    .expect("tall page");
    chrome_session::scroll_at(&page, 800.0)
        .await
        .expect("wheel down");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let y = chrome_session::evaluate_js(&page, "window.scrollY")
        .await
        .expect("scrollY");
    let y: f64 = y.parse().unwrap_or(0.0);
    assert!(y > 0.0, "wheel event must scroll the page, got scrollY={y}");
    chrome_session::press_key(&page, "End")
        .await
        .expect("End key");
    assert!(
        chrome_session::press_key(&page, "ctrl+shift+notakey")
            .await
            .is_err(),
        "unknown key in combo must error"
    );
    chrome_session::press_key(&page, "ctrl+a")
        .await
        .expect("ctrl+a dispatch");

    // Dropping only the fixture tab keeps the browser alive for the
    // network scenario that may follow.
    session.close_tab(&tab).await.ok();
}

/// EVERYTHING runs in ONE `#[tokio::test]`: `ChromeSession::global()` is a
/// process-global whose CDP handler task is bound to the runtime that first
/// created it — a second `#[tokio::test]` would inherit a dead connection
/// once the first test's runtime is dropped. One runtime → one browser →
/// sequential scenarios.
#[tokio::test(flavor = "multi_thread")]
async fn browser_e2e_remote_delegation_capabilities() {
    if !browser_available() {
        println!("skipping: no compatible browser detected on this host");
        return;
    }
    let session = ChromeSession::global()
        .await
        .expect("browser session should launch");

    scenario_remote_delegation(&session).await;
    let network_tab = scenario_network_flow(&session).await;

    // Closing the network tab is safe with the fixture tab still gone:
    // this is the test's tail — Chrome exiting afterwards harms nothing.
    if let Some(tab) = network_tab {
        session.close_tab(&tab).await.ok();
    }
}
