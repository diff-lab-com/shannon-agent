//! Browser session layer (B2 sink of `shannon-tools::chrome_session`).
//!
//! The builtin `browser_*` tools share one browser and a small registry of
//! pages. The session is lazily created on the first tool call inside a
//! process — the tools hold `Arc<ChromeSession>` via the registry and run
//! their actions through the same browser until the user closes the page or
//! the process shuts down.
//!
//! Two attach modes (B1-方案B):
//!
//! - **CDP attach** — when `SHANNON_BROWSER_CDP` is set to a non-empty
//!   `ws://` or `http://` endpoint (chromiumoxide resolves an http base via
//!   `/json/version`), the session connects to that already-running Chrome
//!   instead of launching one. Typical setup is a Chrome on an SSH remote
//!   plus a local port forward:
//!
//!     ```text
//!     ssh -L 9222:127.0.0.1:9222 user@host
//!     # on the remote:
//!     chromium --headless --remote-debugging-port=9222 \
//!              --user-data-dir=/tmp/shannon-cdp
//!     # locally:
//!     SHANNON_BROWSER_CDP=http://127.0.0.1:9222 shannon
//!     ```
//!
//!   No browser binary is needed locally in this mode.
//! - **Local launch** — the default: locate the user's installed
//!   Chrome/Chromium/Edge via [`crate::detect`] and launch it. When no
//!   browser is found, every tool returns the same actionable error and the
//!   user runs `/browser doctor` for install guidance.
//!
//! # Feature gating
//!
//! The whole live implementation (`Browser::launch`, `Browser::connect`,
//! `Page::click`, …) is gated behind `#[cfg(feature = "local-browser")]`
//! because the chromiumoxide crate pulls in a large cdp-generated module.
//! The stub keeps the public surface — `ChromeSession::global`,
//! `open_page`, `list_tabs`, `close_tab`, `get_page`, plus the `navigate` /
//! `click_at` / `type_text` / `press_key` / `scroll_at` / `page_text` /
//! `screenshot_png` / `console_messages` free functions — so consumers
//! compile under both `cargo check` and `cargo check --features
//! local-browser` without their own cfg.

/// Endpoint normalization shared by live and stub: trim whitespace, treat
/// empty/whitespace-only as unset.
fn normalize_endpoint(raw: Option<String>) -> Option<String> {
    raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[cfg(feature = "local-browser")]
mod live {
    use chromiumoxide::browser::{Browser, BrowserConfig};
    use futures::StreamExt;
    use serde::Serialize;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // Re-exported through this module so the session-level `pub use` below
    // keeps detect/install_hint reachable at the old `chrome_session::…`
    // paths.
    pub use crate::detect::{detect_system_browser, install_hint};

    /// Page type passed to every helper function in this module.
    pub type Page = chromiumoxide::Page;

    /// Identifier for a page within the running browser.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
    pub struct TabId(pub String);

    /// How the session attached to a browser.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum AttachMode {
        /// Launched the locally detected browser binary.
        LocalLaunch,
        /// Connected to a running Chrome over CDP at this endpoint
        /// (`SHANNON_BROWSER_CDP`).
        Cdp(String),
    }

    /// Process-wide live browser session.
    pub struct ChromeSession {
        browser: Browser,
        _handler_task: tokio::task::JoinHandle<()>,
        pages: Arc<Mutex<HashMap<TabId, chromiumoxide::Page>>>,
        /// Per-tab console message buffer (tail-capped at
        /// [`CONSOLE_BUFFER_CAP`] entries). Filled by a listener task
        /// spawned with each tab.
        console_logs: Arc<Mutex<HashMap<TabId, Vec<String>>>>,
        user_data_dir: PathBuf,
        attach: AttachMode,
    }

    /// Maximum console messages retained per tab (oldest dropped).
    const CONSOLE_BUFFER_CAP: usize = 500;

    /// The `SHANNON_BROWSER_CDP` endpoint, when configured to a non-empty
    /// value. Pure env read so doctor/status can render it without a
    /// session.
    pub fn cdp_endpoint() -> Option<String> {
        super::normalize_endpoint(std::env::var("SHANNON_BROWSER_CDP").ok())
    }

    /// One-glance description of how the next session would attach —
    /// rendered by `/browser status` / `/browser doctor`. Never starts a
    /// browser.
    pub fn attach_summary() -> String {
        match cdp_endpoint() {
            Some(ep) => format!("CDP attach (SHANNON_BROWSER_CDP): {ep}"),
            None => match detect_system_browser() {
                Ok(exe) => format!("local launch: {} ({})", exe.path.display(), exe.source),
                Err(e) => e.to_string(),
            },
        }
    }

    /// Actionable hint appended to CDP connect failures.
    fn cdp_hint(endpoint: &str) -> String {
        format!(
            "\nHint: the endpoint must be a Chrome/Chromium listening with \
--remote-debugging-port. For a browser on an SSH remote:\n  \
ssh -L 9222:127.0.0.1:9222 user@host   # local forward\n  \
chromium --headless --remote-debugging-port=9222 --user-data-dir=/tmp/shannon-cdp   # remote\n\
then point SHANNON_BROWSER_CDP at the forwarded port (current value: {endpoint})."
        )
    }

    impl ChromeSession {
        pub async fn global() -> Result<Arc<Self>, String> {
            static INIT: tokio::sync::OnceCell<Result<Arc<ChromeSession>, String>> =
                tokio::sync::OnceCell::const_new();
            INIT.get_or_init(Self::start).await.clone()
        }

        /// Attach per [`attach_summary`]'s precedence: CDP endpoint when
        /// configured, local launch otherwise.
        async fn start() -> Result<Arc<Self>, String> {
            match cdp_endpoint() {
                Some(ep) => Self::connect_cdp(ep).await,
                None => Self::launch_local().await,
            }
        }

        /// Connect to an already-running Chrome over CDP (`B1-方案B`).
        /// chromiumoxide resolves an `http://` base to the websocket URL
        /// via `/json/version` automatically.
        async fn connect_cdp(endpoint: String) -> Result<Arc<Self>, String> {
            let (browser, mut handler) = Browser::connect(endpoint.clone()).await.map_err(|e| {
                format!(
                    "failed to connect CDP endpoint {endpoint} ({}).{}",
                    e,
                    cdp_hint(&endpoint)
                )
            })?;
            let handler_task = tokio::spawn(async move {
                while let Some(h) = handler.next().await {
                    if h.is_err() {
                        break;
                    }
                }
            });
            tracing::info!(endpoint = %endpoint, "browser session attached over CDP");
            Ok(Arc::new(Self {
                browser,
                _handler_task: handler_task,
                pages: Arc::new(Mutex::new(HashMap::new())),
                console_logs: Arc::new(Mutex::new(HashMap::new())),
                // Remote attach has no local profile dir; the accessor is
                // only informational.
                user_data_dir: PathBuf::new(),
                attach: AttachMode::Cdp(endpoint),
            }))
        }

        /// Detect and launch the user's installed browser.
        async fn launch_local() -> Result<Arc<Self>, String> {
            let exe = detect_system_browser().map_err(|e| e.to_string())?;
            // Unique per Shannon process: Chrome refuses to start twice on
            // the same user-data-dir (SingletonLock), and two Shannon
            // windows sharing a profile would fight over it. Cross-session
            // persistence is opt-in via SHANNON_BROWSER_USER_DATA_DIR.
            let user_data_dir = std::env::var_os("SHANNON_BROWSER_USER_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::temp_dir().join(format!("shannon-browser-{}", uuid::Uuid::new_v4()))
                });
            std::fs::create_dir_all(&user_data_dir)
                .map_err(|e| format!("create user-data-dir {user_data_dir:?}: {e}"))?;
            let mut config = BrowserConfig::builder()
                .chrome_executable(exe.path.clone())
                .user_data_dir(user_data_dir.clone())
                .no_sandbox()
                .with_head()
                .window_size(1280, 800)
                .args(vec![
                    "--remote-debugging-port=0".to_string(),
                    "--disable-background-networking".to_string(),
                    "--disable-dev-shm-usage".to_string(),
                    "--disable-features=Translate,InfinitePrefetch".to_string(),
                    "--lang=en-US".to_string(),
                    "--no-first-run".to_string(),
                    "--no-default-browser-check".to_string(),
                ]);
            if let Ok(extra) = std::env::var("SHANNON_CHROMIUM_EXTRA_ARGS") {
                for tok in extra.split_whitespace() {
                    config = config.args(vec![tok.to_string()]);
                }
            }
            let config = config.build().map_err(|e| format!("build config: {e}"))?;
            let (browser, mut handler) = Browser::launch(config).await.map_err(|e| {
                let detail = e.to_string();
                let hint = if detail.contains("SingletonLock") {
                    "\nHint: the profile directory has a stale SingletonLock from a Chrome                      process that did not exit cleanly. Remove the lock file or set                      SHANNON_BROWSER_USER_DATA_DIR to a fresh directory."
                } else {
                    "\nRun `/browser doctor` for install hints."
                };
                format!("failed to launch {} ({}).{hint}", exe.path.display(), e)
            })?;
            let handler_task = tokio::spawn(async move {
                while let Some(h) = handler.next().await {
                    if h.is_err() {
                        break;
                    }
                }
            });
            Ok(Arc::new(Self {
                browser,
                _handler_task: handler_task,
                pages: Arc::new(Mutex::new(HashMap::new())),
                console_logs: Arc::new(Mutex::new(HashMap::new())),
                user_data_dir,
                attach: AttachMode::LocalLaunch,
            }))
        }

        pub fn user_data_dir(&self) -> &std::path::Path {
            &self.user_data_dir
        }

        /// How this session attached (local launch vs CDP endpoint).
        pub fn attach_mode(&self) -> &AttachMode {
            &self.attach
        }

        pub async fn open_page(self: &Arc<Self>, url: &str) -> Result<TabId, String> {
            let page = self
                .browser
                .new_page(url)
                .await
                .map_err(|e| format!("new_page({url}): {e}"))?;
            let id = TabId(uuid::Uuid::new_v4().to_string());
            // Attach a console listener so `browser_console` can read the
            // page's messages after the fact. Best-effort: a dropped
            // listener only means missed messages, never a failed session.
            if let Ok(mut stream) = page
                .event_listener::<chromiumoxide_cdp::cdp::js_protocol::runtime::EventConsoleApiCalled>()
                .await
            {
                let logs = Arc::clone(&self.console_logs);
                let tab = id.clone();
                tokio::spawn(async move {
                    while let Some(ev) = stream.next().await {
                        let mut guard = logs.lock().await;
                        let buf = guard.entry(tab.clone()).or_default();
                        for arg in &ev.args {
                            let kind = format!("{:?}", ev.r#type).to_lowercase();
                            let text = arg
                                .value
                                .as_ref()
                                .map(|v| match v {
                                    serde_json::Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                })
                                .or_else(|| arg.description.clone())
                                .unwrap_or_else(|| "<unserializable>".to_string());
                            buf.push(format!("[{kind}] {text}"));
                        }
                        while buf.len() > CONSOLE_BUFFER_CAP {
                            buf.remove(0);
                        }
                    }
                });
            }
            self.pages.lock().await.insert(id.clone(), page);
            Ok(id)
        }

        pub async fn list_tabs(&self) -> Vec<(TabId, String)> {
            let guard = self.pages.lock().await;
            let mut out = Vec::with_capacity(guard.len());
            for (id, page) in guard.iter() {
                let url = page.url().await.ok().flatten().unwrap_or_default();
                let title = page.get_title().await.ok().flatten().unwrap_or_default();
                out.push((id.clone(), format!("{title} — {url}")));
            }
            out
        }

        pub async fn close_tab(&self, id: &TabId) -> Result<(), String> {
            let page = self
                .pages
                .lock()
                .await
                .remove(id)
                .ok_or_else(|| format!("unknown tab: {}", id.0))?;
            page.close().await.map_err(|e| format!("close tab: {e}"))?;
            Ok(())
        }

        pub async fn get_page(&self, id: &TabId) -> Result<Page, String> {
            self.pages
                .lock()
                .await
                .get(id)
                .cloned()
                .ok_or_else(|| format!("unknown tab: {}", id.0))
        }

        /// Return the buffered console messages for a tab (oldest first).
        pub async fn console_messages(&self, id: &TabId) -> Vec<String> {
            self.console_logs
                .lock()
                .await
                .get(id)
                .cloned()
                .unwrap_or_default()
        }
    }

    pub async fn navigate(page: &Page, url: &str) -> Result<(), String> {
        page.goto(url)
            .await
            .map_err(|e| format!("navigate({url}): {e}"))?;
        Ok(())
    }

    pub async fn click_at(page: &Page, x: f64, y: f64) -> Result<(), String> {
        use chromiumoxide::layout::Point;
        page.click(Point::new(x, y))
            .await
            .map_err(|e| format!("click({x},{y}): {e}"))?;
        Ok(())
    }

    pub async fn type_text(page: &Page, text: &str) -> Result<(), String> {
        match page.find_element("*:focus").await {
            Ok(el) => el
                .type_str(text)
                .await
                .map(|_| ())
                .map_err(|e| format!("type_str: {e}")),
            Err(_) => {
                let escaped = text
                    .replace('\\', "\\\\")
                    .replace('`', "\\`")
                    .replace('$', "\\$")
                    .replace('\n', "\\n")
                    .replace('\r', "");
                let js = format!(
                    "(function(){{ document.execCommand('insertText', false, `{escaped}`); return true; }})()"
                );
                page.evaluate(js)
                    .await
                    .map(|_| ())
                    .map_err(|e| format!("type evaluate: {e}"))
            }
        }
    }

    pub async fn press_key(page: &Page, key: &str) -> Result<(), String> {
        use chromiumoxide_cdp::cdp::browser_protocol::input::{
            DispatchKeyEventParams, DispatchKeyEventType,
        };
        let dispatch = |t: DispatchKeyEventType| DispatchKeyEventParams {
            r#type: t,
            key: Some(key.to_string()),
            code: Some(key.to_string()),
            text: None,
            unmodified_text: None,
            auto_repeat: None,
            location: None,
            is_keypad: None,
            is_system_key: None,
            windows_virtual_key_code: Some(0),
            native_virtual_key_code: Some(0),
            modifiers: None,
            timestamp: None,
            key_identifier: None,
            commands: None,
        };
        page.execute(dispatch(DispatchKeyEventType::RawKeyDown))
            .await
            .map_err(|e| format!("key down: {e}"))?;
        page.execute(dispatch(DispatchKeyEventType::KeyUp))
            .await
            .map_err(|e| format!("key up: {e}"))?;
        Ok(())
    }

    pub async fn scroll_at(page: &Page, delta_y: f64) -> Result<(), String> {
        let js = format!("(function(){{ window.scrollBy(0, {delta_y}); return true; }})()");
        page.evaluate(js)
            .await
            .map(|_| ())
            .map_err(|e| format!("scroll: {e}"))
    }

    pub async fn page_text(page: &Page) -> Result<String, String> {
        let title = page
            .get_title()
            .await
            .map_err(|e| format!("get_title: {e}"))?
            .unwrap_or_default();
        let url = page
            .url()
            .await
            .map_err(|e| format!("url: {e}"))?
            .unwrap_or_default();
        let result = page
            .evaluate("document.body && document.body.innerText")
            .await
            .map_err(|e| format!("evaluate innerText: {e}"))?;
        let text = result
            .value()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        let truncated = if text.len() > 8000 {
            format!(
                "{}\n\n[truncated — {} more bytes]",
                &text[..8000],
                text.len() - 8000
            )
        } else {
            text
        };
        Ok(format!("{title}\n{url}\n\n{truncated}"))
    }

    pub async fn screenshot_png(page: &Page, full_page: bool) -> Result<Vec<u8>, String> {
        let params = chromiumoxide::page::ScreenshotParams::builder()
            .full_page(full_page)
            .build();
        page.screenshot(params)
            .await
            .map_err(|e| format!("screenshot: {e}"))
    }
}

#[cfg(not(feature = "local-browser"))]
mod stub {
    use std::path::PathBuf;

    /// Page type passed to every helper function in this module when
    /// the `local-browser` feature is off — wraps the zero-sized
    /// `StubPage` so tool call sites compile uniformly.
    pub type Page = StubPage;

    /// Zero-sized stub page — every helper that takes a live page
    /// accepts `&Page` (i.e. `&StubPage`) and returns the same
    /// `BROWSER_DISABLED` error string.
    pub struct StubPage;

    #[derive(Debug, Clone)]
    pub struct TabId(pub String);

    pub struct ChromeSession;

    const BROWSER_DISABLED: &str = "the local-browser feature is not enabled in this Shannon build. \
Rebuild with `--features local-browser` (or the CLI/desktop passthrough) to use the built-in browser tools.";

    impl ChromeSession {
        pub async fn global() -> Result<std::sync::Arc<Self>, String> {
            Err(BROWSER_DISABLED.to_string())
        }
        pub async fn open_page(&self, _url: &str) -> Result<TabId, String> {
            Err(BROWSER_DISABLED.to_string())
        }
        pub async fn list_tabs(&self) -> Vec<(TabId, String)> {
            Vec::new()
        }
        pub async fn close_tab(&self, _id: &TabId) -> Result<(), String> {
            Err(BROWSER_DISABLED.to_string())
        }
        pub async fn get_page(&self, _id: &TabId) -> Result<Page, String> {
            Err(BROWSER_DISABLED.to_string())
        }
        pub async fn console_messages(&self, _id: &TabId) -> Vec<String> {
            Vec::new()
        }
    }

    pub fn install_hint() -> &'static str {
        "the local-browser feature is not enabled in this Shannon build. \
Rebuild with `--features local-browser` to use the built-in browser tools."
    }

    pub fn detect_system_browser() -> Result<PathBuf, String> {
        Err(BROWSER_DISABLED.to_string())
    }

    /// Mirrors the live [`cdp_endpoint`] env read so doctor output is
    /// identical in stub builds.
    pub fn cdp_endpoint() -> Option<String> {
        super::normalize_endpoint(std::env::var("SHANNON_BROWSER_CDP").ok())
    }

    pub fn attach_summary() -> String {
        match cdp_endpoint() {
            Some(ep) => format!("CDP attach (SHANNON_BROWSER_CDP): {ep}"),
            None => BROWSER_DISABLED.to_string(),
        }
    }

    pub async fn navigate(_p: &Page, _url: &str) -> Result<(), String> {
        Err(BROWSER_DISABLED.to_string())
    }
    pub async fn click_at(_p: &Page, _x: f64, _y: f64) -> Result<(), String> {
        Err(BROWSER_DISABLED.to_string())
    }
    pub async fn type_text(_p: &Page, _text: &str) -> Result<(), String> {
        Err(BROWSER_DISABLED.to_string())
    }
    pub async fn press_key(_p: &Page, _key: &str) -> Result<(), String> {
        Err(BROWSER_DISABLED.to_string())
    }
    pub async fn scroll_at(_p: &Page, _d: f64) -> Result<(), String> {
        Err(BROWSER_DISABLED.to_string())
    }
    pub async fn page_text(_p: &Page) -> Result<String, String> {
        Err(BROWSER_DISABLED.to_string())
    }
    pub async fn screenshot_png(_p: &Page, _full_page: bool) -> Result<Vec<u8>, String> {
        Err(BROWSER_DISABLED.to_string())
    }
}

#[cfg(feature = "local-browser")]
pub use live::{
    AttachMode, ChromeSession, Page, TabId, attach_summary, cdp_endpoint, click_at,
    detect_system_browser, install_hint, navigate, page_text, press_key, screenshot_png, scroll_at,
    type_text,
};

#[cfg(not(feature = "local-browser"))]
pub use stub::{
    ChromeSession, Page, StubPage, TabId, attach_summary, cdp_endpoint, click_at,
    detect_system_browser, install_hint, navigate, page_text, press_key, screenshot_png, scroll_at,
    type_text,
};

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::normalize_endpoint;

    #[test]
    fn normalize_endpoint_trims_and_drops_empty() {
        assert_eq!(
            normalize_endpoint(Some("  http://127.0.0.1:9222 ".to_string())),
            Some("http://127.0.0.1:9222".to_string())
        );
        // Whitespace-only counts as unset.
        assert_eq!(normalize_endpoint(Some("   ".to_string())), None);
        assert_eq!(normalize_endpoint(None), None);
    }

    #[cfg(feature = "local-browser")]
    #[test]
    fn attach_mode_carries_endpoint() {
        // The enum carries the endpoint so status can render it verbatim.
        let mode = super::live::AttachMode::Cdp("http://127.0.0.1:9222".to_string());
        assert_eq!(
            format!("{mode:?}"),
            r#"Cdp("http://127.0.0.1:9222")"#.to_string()
        );
    }
}
