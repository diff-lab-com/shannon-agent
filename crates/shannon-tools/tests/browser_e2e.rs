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
    // Cheap probe: reuse the session's own detection. SHANNON_BROWSER_PATH
    // lets a developer point at a specific binary.
    std::env::var_os("SHANNON_BROWSER_PATH").is_some() || which_browser().is_some()
}

fn which_browser() -> Option<std::path::PathBuf> {
    for p in [
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
    ] {
        let p = std::path::Path::new(p);
        if p.is_file() {
            return Some(p.to_path_buf());
        }
    }
    None
}

#[tokio::test]
async fn browser_e2e_navigate_screenshot_interact() {
    if !browser_available() {
        println!("skipping: no compatible browser detected on this host");
        return;
    }

    let session = ChromeSession::global()
        .await
        .expect("browser session should launch");

    // 1) Navigate.
    let tab = session
        .open_page("https://example.com")
        .await
        .expect("open example.com");
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

    // 5) Click + type into the page's (non-existent) focused element:
    //    click somewhere harmless, then evaluate that typing landed via
    //    execCommand is only observable with a focused editable — here we
    //    just exercise the code path for panics.
    chrome_session::click_at(&page, 100.0, 100.0)
        .await
        .expect("click");
    let _ = chrome_session::type_text(&page, "hello").await;

    // 6) Console listener: inject a console.log and expect it in the
    //    session buffer (Runtime.consoleAPICalled → listener task).
    page.evaluate("console.log('e2e-console-marker')")
        .await
        .expect("evaluate console.log");
    // Give the CDP event a moment to reach our listener task.
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

    // 8) Tabs listing includes our tab; close it.
    let tabs = session.list_tabs().await;
    assert!(
        tabs.iter().any(|(id, _)| *id == tab),
        "tab should be listed"
    );
    session.close_tab(&tab).await.expect("close tab");
    let tabs = session.list_tabs().await;
    assert!(!tabs.iter().any(|(id, _)| *id == tab), "tab should be gone");
}
