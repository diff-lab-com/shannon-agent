//! Live system-browser session backed by chromiumoxide (T14 Phase 1).
//!
//! The 7 builtin browser tools share one browser process and a small
//! registry of pages. The session is lazily created on the first tool
//! call inside a process — the tools hold `Arc<ChromeSession>` via the
//! registry and run their actions through the same Chrome instance until
//! the user closes the page or the process shuts down.
//!
//! Browser binaries are NEVER bundled: this module locates the user's
//! installed Chrome/Chromium/Edge via [`detect_system_browser`]. When no
//! browser is found, every tool returns the same actionable error and
//! the user runs `/browser doctor` for install guidance.
//!
//! # Feature gating
//!
//! The whole live implementation (`Browser::launch`, `Page::click`, etc.)
//! is gated behind `#[cfg(feature = "local-browser")]` because the
//! chromiumoxide crate pulls in a large cdp-generated module. The
//! stub keeps the public surface — `ChromeSession::global`,
//! `open_page`, `list_tabs`, `close_tab`, `get_page`, plus the
//! `navigate` / `click_at` / `type_text` / `press_key` / `scroll_at` /
//! `page_text` / `screenshot_png` / `console_messages` free functions —
//! so the 7 tools in `browser_tools.rs` compile under both `cargo
//! check` and `cargo check --features local-browser`.

use std::path::PathBuf;

#[cfg(feature = "local-browser")]
mod live {
    use chromiumoxide::browser::{Browser, BrowserConfig};
    use futures::StreamExt;
    use serde::Serialize;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// Page type passed to every helper function in this module.
    pub type Page = chromiumoxide::Page;

    /// Identifier for a page within the running browser.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
    pub struct TabId(pub String);

    /// Process-wide live browser session.
    pub struct ChromeSession {
        browser: Browser,
        _handler_task: tokio::task::JoinHandle<()>,
        pages: Arc<Mutex<HashMap<TabId, chromiumoxide::Page>>>,
        user_data_dir: PathBuf,
    }

    // ── browser detection (inlined) ──────────────────────────────
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct BrowserExecutable {
        pub path: PathBuf,
        pub source: &'static str,
    }

    fn home() -> String {
        std::env::var("HOME").unwrap_or_default()
    }

    fn is_executable_file(path: &Path) -> bool {
        path.is_file()
            && std::fs::metadata(path)
                .map(|m| {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        m.permissions().mode() & 0o111 != 0
                    }
                    #[cfg(not(unix))]
                    {
                        true
                    }
                })
                .unwrap_or(false)
    }

    pub fn install_hint() -> &'static str {
        if cfg!(target_os = "macos") {
            "To enable browser control, install one of:\n  • brew: brew install --cask chromium\n  • or download Google Chrome from https://www.google.com/chrome/"
        } else if cfg!(target_os = "windows") {
            "To enable browser control, install one of:\n  • winget: winget install Google.Chrome\n  • or download Chrome from https://www.google.com/chrome/"
        } else {
            "To enable browser control, install one of:\n  • apt:    sudo apt install chromium-browser\n  • dnf:    sudo dnf install chromium\n  • pacman: sudo pacman -S chromium\n  • snap:   sudo snap install chromium\n  • Or use the Playwright MCP instead: /browser setup"
        }
    }

    fn candidate_paths() -> Vec<(PathBuf, &'static str)> {
        let mut out: Vec<(PathBuf, &'static str)> = Vec::new();
        if let Some(p) = std::env::var_os("SHANNON_BROWSER_PATH")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
        {
            out.push((p, "env"));
        }
        if cfg!(target_os = "macos") {
            for base in ["/Applications", &format!("{}/Applications", home())] {
                for app in [
                    "Google Chrome.app/Contents/MacOS/Google Chrome",
                    "Chromium.app/Contents/MacOS/Chromium",
                    "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
                ] {
                    out.push((PathBuf::from(base).join(app), "macos-app"));
                }
            }
            for p in ["/opt/homebrew/bin/chromium", "/usr/local/bin/chromium"] {
                out.push((PathBuf::from(p), "macos-app"));
            }
        } else if cfg!(target_os = "windows") {
            for base in [
                std::env::var("ProgramFiles").unwrap_or_default(),
                std::env::var("ProgramFiles(x86)").unwrap_or_default(),
                std::env::var("LOCALAPPDATA").unwrap_or_default(),
            ]
            .into_iter()
            .filter(|b| !b.is_empty())
            {
                for rel in [
                    r"Google\Chrome\Application\chrome.exe",
                    r"Chromium\Application\chrome.exe",
                ] {
                    out.push((PathBuf::from(&base).join(rel), "windows-path"));
                }
            }
        } else {
            for p in [
                "/usr/bin/google-chrome",
                "/usr/bin/google-chrome-stable",
                "/usr/bin/chromium",
                "/usr/bin/chromium-browser",
                "/usr/bin/microsoft-edge",
                "/snap/bin/chromium",
                &format!("{}/.local/bin/chromium", home()),
            ] {
                out.push((PathBuf::from(p), "linux-path"));
            }
        }
        out
    }

    pub fn detect_system_browser() -> Result<BrowserExecutable, String> {
        let candidates = candidate_paths();
        let searched: Vec<PathBuf> = candidates.iter().map(|(p, _)| p.clone()).collect();
        for (path, source) in candidates {
            let usable = if source == "env" {
                path.exists()
            } else {
                is_executable_file(&path)
            };
            if usable {
                return Ok(BrowserExecutable { path, source });
            }
        }
        let detail = searched
            .iter()
            .map(|p| format!("  • {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n");
        Err(format!(
            "No compatible browser found. Searched:\n{detail}\n\n{INSTALL_HINT}",
            INSTALL_HINT = install_hint()
        ))
    }

    impl ChromeSession {
        pub async fn global() -> Result<Arc<Self>, String> {
            static INIT: tokio::sync::OnceCell<Result<Arc<ChromeSession>, String>> =
                tokio::sync::OnceCell::const_new();
            INIT.get_or_init(Self::launch).await.clone()
        }

        async fn launch() -> Result<Arc<Self>, String> {
            let exe = detect_system_browser()?;
            let user_data_dir = std::env::var_os("SHANNON_BROWSER_USER_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("shannon-browser"));
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
                format!(
                    "failed to launch {} ({}). Run `/browser doctor` for install hints.",
                    exe.path.display(),
                    e
                )
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
                user_data_dir,
            }))
        }

        pub fn user_data_dir(&self) -> &std::path::Path {
            &self.user_data_dir
        }

        pub async fn open_page(self: &Arc<Self>, url: &str) -> Result<TabId, String> {
            let page = self
                .browser
                .new_page(url)
                .await
                .map_err(|e| format!("new_page({url}): {e}"))?;
            let id = TabId(uuid::Uuid::new_v4().to_string());
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

    pub async fn press_key(_page: &Page, key: &str) -> Result<(), String> {
        Err(format!(
            "press_key({key}): not yet wired — T14 Phase 2 adds CDP key events"
        ))
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

    pub async fn screenshot_png(page: &Page) -> Result<Vec<u8>, String> {
        page.screenshot(chromiumoxide::page::ScreenshotParams::default())
            .await
            .map_err(|e| format!("screenshot: {e}"))
    }

    pub async fn console_messages(_page: &Page) -> Vec<String> {
        Vec::new()
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
    }

    pub fn install_hint() -> &'static str {
        "the local-browser feature is not enabled in this Shannon build. \
Rebuild with `--features local-browser` to use the built-in browser tools."
    }

    pub fn detect_system_browser() -> Result<PathBuf, String> {
        Err(BROWSER_DISABLED.to_string())
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
    pub async fn screenshot_png(_p: &Page) -> Result<Vec<u8>, String> {
        Err(BROWSER_DISABLED.to_string())
    }
    pub async fn console_messages(_p: &Page) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(feature = "local-browser")]
pub use live::{
    ChromeSession, Page, TabId, click_at, console_messages, detect_system_browser, install_hint,
    navigate, page_text, press_key, screenshot_png, scroll_at, type_text,
};

#[cfg(not(feature = "local-browser"))]
pub use stub::{
    ChromeSession, Page, StubPage, TabId, click_at, console_messages, detect_system_browser,
    install_hint, navigate, page_text, press_key, screenshot_png, scroll_at, type_text,
};
