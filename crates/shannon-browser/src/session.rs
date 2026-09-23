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

// ── Temp-profile lifecycle (review §P3-7) ──────────────────────────────
//
// The local-launch path creates a throwaway
// `<temp>/shannon-browser-<uuid>` profile per process. Live sessions are
// process-global (`static OnceCell`), so a plain `Drop` rarely fires — the
// explicit drop cleanup plus the startup sweep below together make sure the
// directories do not accumulate forever.

/// Directory-name prefix of the Shannon-managed temporary browser profiles
/// created by the local-launch path.
pub(crate) const TEMP_PROFILE_PREFIX: &str = "shannon-browser-";

/// Startup sweep horizon (review §P3-7): managed profiles under the OS temp
/// root whose mtime is older than this are removed when a new session
/// launches. Long enough that a concurrently running Shannon session's live
/// profile is never swept in practice.
pub(crate) const STALE_PROFILE_MAX_AGE: std::time::Duration =
    std::time::Duration::from_secs(24 * 60 * 60);

/// True when `path`'s file name marks it as a Shannon-created temporary
/// browser profile ([`TEMP_PROFILE_PREFIX`]). Name-based only — callers
/// additionally check `is_dir` where it matters. This is the second line of
/// defense: the primary guard for a user-configured
/// `SHANNON_BROWSER_USER_DATA_DIR` is the session's `temp_profile` flag,
/// which is only set for the throwaway path this process generated itself.
pub(crate) fn is_managed_temp_profile(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| name.starts_with(TEMP_PROFILE_PREFIX))
}

/// Best-effort recursive removal of one Shannon-managed temporary profile
/// directory. Refuses paths that do not carry the [`TEMP_PROFILE_PREFIX`]
/// name (user-configured `SHANNON_BROWSER_USER_DATA_DIR` must never be
/// touched), logs instead of panicking, and returns whether it removed
/// anything.
pub(crate) fn remove_temp_profile(dir: &std::path::Path) -> bool {
    if !is_managed_temp_profile(dir) {
        tracing::debug!(
            profile = %dir.display(),
            "refusing to remove non-Shannon browser profile directory"
        );
        return false;
    }
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {
            tracing::debug!(profile = %dir.display(), "removed temporary browser profile");
            true
        }
        Err(e) => {
            tracing::warn!(
                profile = %dir.display(),
                "failed to remove temporary browser profile: {e}"
            );
            false
        }
    }
}

/// Best-effort startup sweep (review §P3-7): delete Shannon-managed temp
/// browser profiles under `root` whose mtime is older than `max_age` —
/// leftovers from crashed or killed runs. Unrelated temp entries (wrong
/// name, not a directory, younger than the horizon) are left untouched.
/// Returns the number of directories removed.
pub(crate) fn sweep_stale_temp_profiles(
    root: &std::path::Path,
    max_age: std::time::Duration,
) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    let now = std::time::SystemTime::now();
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_managed_temp_profile(&path) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let stale = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age >= max_age);
        if stale && remove_temp_profile(&path) {
            removed += 1;
        }
    }
    if removed > 0 {
        tracing::info!(
            root = %root.display(),
            removed,
            "swept stale temporary browser profiles"
        );
    }
    removed
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
        /// Review §P3-7: true only when `user_data_dir` is the throwaway
        /// `<temp>/shannon-browser-<uuid>` profile this process generated
        /// itself — the only kind [`Drop`] may ever delete.
        temp_profile: bool,
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

    impl Drop for ChromeSession {
        fn drop(&mut self) {
            // Review §P3-7: delete the throwaway profile when the session is
            // torn down. Only a self-created `<temp>/shannon-browser-<uuid>`
            // dir is ever removed — a user-configured
            // `SHANNON_BROWSER_USER_DATA_DIR` and the empty CDP-attach path
            // are never touched. Best-effort: the process-global session in
            // its `static OnceCell` may never drop (statics are not dropped
            // at exit), so the startup sweep in [`launch_local`] is the
            // backstop that keeps stale profiles from accumulating forever.
            if self.temp_profile {
                super::remove_temp_profile(&self.user_data_dir);
            }
        }
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
                temp_profile: false,
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
            // Review §P3-7: sweep profiles orphaned by previous crashed or
            // killed runs before adding our own.
            super::sweep_stale_temp_profiles(&std::env::temp_dir(), super::STALE_PROFILE_MAX_AGE);
            let temp_profile = std::env::var_os("SHANNON_BROWSER_USER_DATA_DIR").is_none();
            let user_data_dir = std::env::var_os("SHANNON_BROWSER_USER_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::temp_dir().join(format!(
                        "{}{}",
                        super::TEMP_PROFILE_PREFIX,
                        uuid::Uuid::new_v4()
                    ))
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
                temp_profile,
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
        // Chromium only synthesizes real key events when the platform fields
        // are populated — a zero virtual key code makes most sites (and
        // every IME-dependent surface) ignore the press.
        let (name, code, vk, text) = normalize_key(key);
        let dispatch = |t: DispatchKeyEventType| DispatchKeyEventParams {
            r#type: t,
            key: Some(name.clone()),
            code: Some(code.clone()),
            text: text.clone(),
            unmodified_text: None,
            auto_repeat: None,
            location: None,
            is_keypad: None,
            is_system_key: None,
            windows_virtual_key_code: Some(vk),
            native_virtual_key_code: Some(vk),
            modifiers: None,
            timestamp: None,
            key_identifier: None,
            commands: None,
        };
        // Keys that produce text use KeyDown with `text` set (Chromium
        // inserts the character); the rest use RawKeyDown/KeyUp.
        let (down, up) = if text.is_some() {
            (DispatchKeyEventType::KeyDown, DispatchKeyEventType::KeyUp)
        } else {
            (
                DispatchKeyEventType::RawKeyDown,
                DispatchKeyEventType::KeyUp,
            )
        };
        page.execute(dispatch(down))
            .await
            .map_err(|e| format!("key down: {e}"))?;
        page.execute(dispatch(up))
            .await
            .map_err(|e| format!("key up: {e}"))?;
        Ok(())
    }

    /// Normalize a key name to `(key, code, virtual-key-code, text)`.
    /// Accepts CDP-style names ("Enter", "ArrowDown", "a", "F5") plus the
    /// friendly aliases "down"/"up"/"left"/"right"/"esc".
    fn normalize_key(key: &str) -> (String, String, i64, Option<String>) {
        let (name, code, vkey, printable) = match key {
            "Enter" | "Return" | "enter" | "return" => ("Enter", "Enter", 0x0D, "\r"),
            "Tab" | "tab" => ("Tab", "Tab", 0x09, "\t"),
            "Escape" | "esc" | "Esc" | "escape" => ("Escape", "Escape", 0x1B, ""),
            "Backspace" | "backspace" => ("Backspace", "Backspace", 0x08, ""),
            "Delete" | "del" | "Del" | "delete" => ("Delete", "Delete", 0x2E, ""),
            " " | "Space" | "space" => (" ", "Space", 0x20, " "),
            "ArrowLeft" | "Left" | "left" => ("ArrowLeft", "ArrowLeft", 0x25, ""),
            "ArrowUp" | "Up" | "up" => ("ArrowUp", "ArrowUp", 0x26, ""),
            "ArrowRight" | "Right" | "right" => ("ArrowRight", "ArrowRight", 0x27, ""),
            "ArrowDown" | "Down" | "down" => ("ArrowDown", "ArrowDown", 0x28, ""),
            "Home" | "home" => ("Home", "Home", 0x24, ""),
            "End" | "end" => ("End", "End", 0x23, ""),
            "PageUp" | "pageup" => ("PageUp", "PageUp", 0x21, ""),
            "PageDown" | "pagedown" => ("PageDown", "PageDown", 0x22, ""),
            k @ ("F1" | "F2" | "F3" | "F4" | "F5" | "F6" | "F7" | "F8" | "F9" | "F10" | "F11"
            | "F12") => {
                let n: u32 = k[1..].parse().unwrap_or(1);
                (k, k, (0x70 + n - 1) as i64, "")
            }
            _ => {
                // Printable single character: VK == uppercase ASCII for
                // letters/digits, else the best-effort char code.
                let ch = key.chars().next().unwrap_or('\0');
                let upper = ch.to_ascii_uppercase() as i64;
                (key, key, upper, key)
            }
        };
        let text = if printable.is_empty() {
            None
        } else {
            Some(printable.to_string())
        };
        (name.to_string(), code.to_string(), vkey, text)
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
            // review §P2-8: walk back to the nearest char boundary so a
            // multi-byte character straddling byte 8000 doesn't panic
            // with "byte index 8000 is not a char boundary". Pages with
            // CJK / emoji content hit this routinely.
            let mut cut = 8000;
            while cut > 0 && !text.is_char_boundary(cut) {
                cut -= 1;
            }
            format!(
                "{}\n\n[truncated — {} more bytes]",
                &text[..cut],
                text.len() - cut
            )
        } else {
            text
        };
        Ok(format!("{title}\n{url}\n\n{truncated}"))
    }

    /// JS that indexes visible interactive elements and returns a JSON
    /// array of `{ref, tag, role, text, x, y, w, h}`. Refs (`e1`..`eN`) are
    /// positional and stay valid until the DOM changes — call
    /// [`element_snapshot`] again after navigation.
    const ELEMENT_INDEX_JS: &str = r#"(function(){
        const sel = 'a[href], button, input, select, textarea, summary, [role="button"], [role="link"], [role="checkbox"], [role="radio"], [role="tab"], [role="menuitem"], [role="option"], [onclick], [contenteditable="true"]';
        const nodes = Array.from(document.querySelectorAll(sel)).filter(el => {
            const r = el.getBoundingClientRect();
            if (r.width <= 0 || r.height <= 0) return false;
            const st = getComputedStyle(el);
            return st.visibility !== 'hidden' && st.display !== 'none';
        }).slice(0, 150);
        return nodes.map((el, i) => {
            const r = el.getBoundingClientRect();
            const label = (el.innerText || el.value || el.placeholder ||
                el.getAttribute('aria-label') || el.getAttribute('title') || '').trim();
            return {
                ref: 'e' + (i + 1),
                tag: el.tagName.toLowerCase(),
                role: el.getAttribute('role') || '',
                text: label.slice(0, 80),
                x: Math.round(r.x + window.scrollX),
                y: Math.round(r.y + window.scrollY),
                w: Math.round(r.width),
                h: Math.round(r.height)
            };
        });
    })()"#;

    /// Interactive-element index of the page: refs usable with
    /// `browser_click`'s `ref` and `browser_fill`. Far more reliable than
    /// screenshot-guessed coordinates and cheaper than an 8KB innerText
    /// dump when the task is "find and press the button".
    pub async fn element_snapshot(page: &Page) -> Result<String, String> {
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
            .evaluate(ELEMENT_INDEX_JS)
            .await
            .map_err(|e| format!("element index: {e}"))?;
        let items = result
            .value()
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
        let mut out = format!("{title}\n{url}\n");
        match items.as_array() {
            Some(list) if !list.is_empty() => {
                out.push_str(
                    "\n[interactive elements — use ref with browser_click/browser_fill]\n",
                );
                for item in list {
                    out.push_str(&format!(
                        "- {} <{}>{} {:?} @ ({},{}) {}x{}\n",
                        item["ref"].as_str().unwrap_or("?"),
                        item["tag"].as_str().unwrap_or("?"),
                        {
                            let role = item["role"].as_str().unwrap_or("");
                            if role.is_empty() {
                                String::new()
                            } else {
                                format!(" role={role}")
                            }
                        },
                        item["text"].as_str().unwrap_or(""),
                        item["x"].as_i64().unwrap_or(0),
                        item["y"].as_i64().unwrap_or(0),
                        item["w"].as_i64().unwrap_or(0),
                        item["h"].as_i64().unwrap_or(0),
                    ));
                }
            }
            _ => out.push_str("\n(no visible interactive elements found)\n"),
        }
        Ok(out)
    }

    /// Click the element with the given ref by dispatching real mouse
    /// events at its viewport center (hover handlers, focus, form
    /// semantics all fire — unlike a bare `el.click()`).
    pub async fn click_element(page: &Page, reference: &str) -> Result<(), String> {
        let js = format!(
            r#"(function(){{
                const sel = {sel};
                const nodes = Array.from(document.querySelectorAll(sel)).filter(el => {{
                    const r = el.getBoundingClientRect();
                    return r.width > 0 && r.height > 0;
                }});
                const el = nodes[{idx}];
                if (!el) return null;
                el.scrollIntoView({{block: 'center'}});
                const r = el.getBoundingClientRect();
                return JSON.stringify({{x: r.x + r.width/2, y: r.y + r.height/2}});
            }})()"#,
            // Mirrors ELEMENT_INDEX_JS's selector so refs line up.
            sel = element_selector_js(),
            idx = ref_index(reference)?,
        );
        let result = page
            .evaluate(js)
            .await
            .map_err(|e| format!("element lookup: {e}"))?;
        let payload = result
            .value()
            .and_then(|v| v.as_str().map(String::from))
            .ok_or_else(|| {
                format!("no element for ref {reference} (DOM changed? re-run browser_snapshot)")
            })?;
        let parsed: serde_json::Value =
            serde_json::from_str(&payload).map_err(|e| format!("element payload: {e}"))?;
        let (x, y) = (
            parsed["x"].as_f64().unwrap_or(0.0),
            parsed["y"].as_f64().unwrap_or(0.0),
        );
        click_at(page, x, y).await
    }

    /// Fill the element at `reference` with `text` (inputs/textareas via
    /// the native setter + input/change events so React/Vue see it;
    /// contenteditable via text insertion). Falls back to focusing and
    /// typing through CDP for exotic widgets.
    pub async fn fill_element(page: &Page, reference: &str, text: &str) -> Result<(), String> {
        let escaped = text
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\n', "\\n")
            .replace('\r', "");
        let js = format!(
            r#"(function(){{
                const sel = {sel};
                const nodes = Array.from(document.querySelectorAll(sel)).filter(el => {{
                    const r = el.getBoundingClientRect();
                    return r.width > 0 && r.height > 0;
                }});
                const el = nodes[{idx}];
                if (!el) return 'missing';
                el.scrollIntoView({{block: 'center'}});
                el.focus();
                const value = '{value}';
                if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') {{
                    const proto = el.tagName === 'INPUT' ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype;
                    const setter = Object.getOwnPropertyDescriptor(proto, 'value').set;
                    setter.call(el, value);
                    el.dispatchEvent(new Event('input', {{bubbles: true}}));
                    el.dispatchEvent(new Event('change', {{bubbles: true}}));
                    return 'filled';
                }}
                if (el.isContentEditable) {{
                    el.textContent = value;
                    el.dispatchEvent(new Event('input', {{bubbles: true}}));
                    return 'filled';
                }}
                return 'unsupported';
            }})()"#,
            sel = element_selector_js(),
            idx = ref_index(reference)?,
            value = escaped,
        );
        let result = page.evaluate(js).await.map_err(|e| format!("fill: {e}"))?;
        match result
            .value()
            .and_then(|v| v.as_str().map(String::from))
            .as_deref()
        {
            Some("filled") => Ok(()),
            Some("missing") => Err(format!(
                "no element for ref {reference} (DOM changed? re-run browser_snapshot)"
            )),
            Some("unsupported") => Err(format!(
                "element {reference} is not fillable; focus it with browser_click and use browser_type instead"
            )),
            _ => Err("fill failed with unexpected payload".into()),
        }
    }

    /// Run a JavaScript expression in the page and return its value
    /// serialized (strings verbatim; everything else as JSON).
    pub async fn evaluate_js(page: &Page, expression: &str) -> Result<String, String> {
        let result = page
            .evaluate(expression)
            .await
            .map_err(|e| format!("evaluate: {e}"))?;
        match result.value().cloned() {
            Some(serde_json::Value::String(s)) => Ok(s),
            Some(v) => serde_json::to_string(&v).map_err(|e| format!("serialize result: {e}")),
            None => Ok("undefined".to_string()),
        }
    }

    /// The element-index selector as a JS string literal, so the click/fill
    /// helpers match refs against the exact same node list as
    /// [`ELEMENT_INDEX_JS`].
    fn element_selector_js() -> String {
        const SEL: &str = "a[href], button, input, select, textarea, summary, [role=\"button\"], [role=\"link\"], [role=\"checkbox\"], [role=\"radio\"], [role=\"tab\"], [role=\"menuitem\"], [role=\"option\"], [onclick], [contenteditable=\"true\"]";
        serde_json::to_string(SEL).unwrap_or_else(|_| "''".to_string())
    }

    /// `e12` → `11` (0-based positional index into the element list).
    fn ref_index(reference: &str) -> Result<usize, String> {
        reference
            .strip_prefix('e')
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|n| *n >= 1)
            .map(|n| n - 1)
            .ok_or_else(|| format!("invalid element ref {reference:?} — refs look like \"e3\" from browser_snapshot"))
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
    pub async fn element_snapshot(_p: &Page) -> Result<String, String> {
        Err(BROWSER_DISABLED.to_string())
    }
    pub async fn click_element(_p: &Page, _reference: &str) -> Result<(), String> {
        Err(BROWSER_DISABLED.to_string())
    }
    pub async fn fill_element(_p: &Page, _reference: &str, _text: &str) -> Result<(), String> {
        Err(BROWSER_DISABLED.to_string())
    }
    pub async fn evaluate_js(_p: &Page, _expression: &str) -> Result<String, String> {
        Err(BROWSER_DISABLED.to_string())
    }
    pub async fn screenshot_png(_p: &Page, _full_page: bool) -> Result<Vec<u8>, String> {
        Err(BROWSER_DISABLED.to_string())
    }
}

#[cfg(feature = "local-browser")]
pub use live::{
    AttachMode, ChromeSession, Page, TabId, attach_summary, cdp_endpoint, click_at, click_element,
    detect_system_browser, element_snapshot, evaluate_js, fill_element, install_hint, navigate,
    page_text, press_key, screenshot_png, scroll_at, type_text,
};

#[cfg(not(feature = "local-browser"))]
pub use stub::{
    ChromeSession, Page, StubPage, TabId, attach_summary, cdp_endpoint, click_at, click_element,
    detect_system_browser, element_snapshot, evaluate_js, fill_element, install_hint, navigate,
    page_text, press_key, screenshot_png, scroll_at, type_text,
};

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::normalize_endpoint;
    use std::time::Duration;

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

    // ── Temp-profile cleanup (review §P3-7) ─────────────────────────────

    fn backdate(path: &std::path::Path, age: Duration) {
        let t = filetime::FileTime::from_system_time(
            std::time::SystemTime::now()
                .checked_sub(age)
                .expect("clock before test horizon"),
        );
        filetime::set_file_mtime(path, t).expect("set mtime");
    }

    #[test]
    fn managed_profile_recognized_by_prefix_name() {
        assert!(super::is_managed_temp_profile(std::path::Path::new(
            "/tmp/shannon-browser-1234-abcd"
        )));
        // The name check matches the file name only; a directory NOT carrying
        // the managed name is never a managed profile (this is the
        // `remove_temp_profile` guard; a user dir that would happen to carry
        // the name is protected by the session's `temp_profile` flag).
        assert!(!super::is_managed_temp_profile(std::path::Path::new(
            "/tmp/.tmpXYZUserData"
        )));
        assert!(!super::is_managed_temp_profile(std::path::Path::new(
            "/home/u/browser-profile"
        )));
    }

    #[test]
    fn remove_temp_profile_deletes_managed_dir_and_content_but_refuses_others() {
        let root = tempfile::tempdir().unwrap();

        // A managed profile with nested content is removed recursively.
        let managed = root
            .path()
            .join("shannon-browser-11111111-2222-3333-4444-555555555555");
        std::fs::create_dir_all(managed.join("Default")).unwrap();
        std::fs::write(managed.join("Default").join("Cookies"), b"x").unwrap();
        assert!(super::remove_temp_profile(&managed));
        assert!(!managed.exists());

        // A directory WITHOUT the managed prefix is never touched — this is
        // what protects a user-configured SHANNON_BROWSER_USER_DATA_DIR.
        let user_dir = root.path().join("my-precious-profile");
        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::write(user_dir.join("Cookies"), b"x").unwrap();
        assert!(!super::remove_temp_profile(&user_dir));
        assert!(user_dir.exists());
    }

    #[test]
    fn sweep_removes_only_stale_managed_profile_dirs() {
        let root = tempfile::tempdir().unwrap();
        let stale_age = Duration::from_secs(25 * 60 * 60);

        // Old managed profile (with content) — must go.
        let old = root.path().join("shannon-browser-aaaa-old");
        std::fs::create_dir_all(old.join("Default")).unwrap();
        std::fs::write(old.join("Default").join("Cookies"), b"x").unwrap();
        backdate(&old, stale_age);

        // Fresh managed profile (live session) — must stay.
        let fresh = root.path().join("shannon-browser-bbbb-new");
        std::fs::create_dir_all(&fresh).unwrap();

        // Old file (not a directory) carrying the prefix — must stay.
        let old_file = root.path().join("shannon-browser-cccc-file");
        std::fs::write(&old_file, b"x").unwrap();
        backdate(&old_file, stale_age);

        // Old dir without the prefix — must stay.
        let unrelated = root.path().join("unrelated-old");
        std::fs::create_dir_all(&unrelated).unwrap();
        backdate(&unrelated, stale_age);

        let removed = super::sweep_stale_temp_profiles(root.path(), super::STALE_PROFILE_MAX_AGE);
        assert_eq!(removed, 1, "exactly the stale managed profile is removed");
        assert!(!old.exists());
        assert!(fresh.exists());
        assert!(old_file.exists());
        assert!(unrelated.exists());

        // A horizon at/below the backdated age catches the fresh dir too —
        // sweep honours the caller's max_age rather than a hard-coded value.
        let removed_all = super::sweep_stale_temp_profiles(root.path(), Duration::ZERO);
        assert_eq!(
            removed_all, 1,
            "fresh profile removed only under a zero horizon"
        );
        assert!(!fresh.exists());
    }
}
