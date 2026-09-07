//! [`BrowserProvider`] implementations (T14 Phase 3).
//!
//! Two providers ship here:
//!
//! - [`LocalSystemBrowserProvider`] — the user's installed
//!   Chrome/Chromium/Edge, launched locally (availability comes from the
//!   same detection the tools use).
//! - [`RemoteBrowserProvider`] — a Chrome already listening on a CDP
//!   endpoint (ws:// or http://), typically reached through an SSH port
//!   forward (`ssh -L 9222:remote:9222 …` + remote
//!   `chromium --headless --remote-debugging-port=9222`). Connecting
//!   itself is `chromiumoxide::Browser::connect`; this provider owns the
//!   reachability probe.

use crate::browser::detect::{BrowserExecutable, detect_system_browser};
use shannon_tool_interface::providers::{BrowserLocality, BrowserProvider};

/// The user's locally installed browser.
#[derive(Debug, Clone)]
pub struct LocalSystemBrowserProvider {
    exe: Option<BrowserExecutable>,
}

impl LocalSystemBrowserProvider {
    /// Probe once at construction; `available()` re-checks lazily via the
    /// stored path so a browser installed later is still picked up.
    pub fn new() -> Self {
        Self {
            exe: detect_system_browser().ok(),
        }
    }
}

impl Default for LocalSystemBrowserProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserProvider for LocalSystemBrowserProvider {
    fn name(&self) -> &str {
        "system-browser"
    }

    fn available(&self) -> bool {
        match &self.exe {
            Some(e) => e.path.is_file(),
            None => detect_system_browser().is_ok(),
        }
    }

    fn locality(&self) -> BrowserLocality {
        BrowserLocality::Local
    }
}

impl LocalSystemBrowserProvider {
    /// The detected browser executable, when available.
    pub fn executable(&self) -> Option<&BrowserExecutable> {
        self.exe.as_ref()
    }
}

/// A browser already listening on a CDP endpoint (ws:// or http://),
/// typically an SSH-forwarded remote Chrome.
#[derive(Debug, Clone)]
pub struct RemoteBrowserProvider {
    endpoint: String,
}

impl RemoteBrowserProvider {
    /// `endpoint` is a ws:// debugger URL or an http:// base (e.g.
    /// `http://127.0.0.1:9222` — chromiumoxide resolves `/json/version`
    /// to the websocket URL automatically).
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Reachability probe: an http endpoint answers `/json/version`; a
    /// ws endpoint is assumed reachable (the connect attempt surfaces
    /// errors).
    pub async fn reachable(&self) -> bool {
        if !self.endpoint.starts_with("http") {
            return true;
        }
        let url = format!("{}/json/version", self.endpoint.trim_end_matches('/'));
        reqwest::get(url)
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

impl BrowserProvider for RemoteBrowserProvider {
    fn name(&self) -> &str {
        "remote-browser"
    }

    fn available(&self) -> bool {
        // Synchronous context — report configured; the async `reachable()`
        // probe runs at connect time.
        !self.endpoint.is_empty()
    }

    fn locality(&self) -> BrowserLocality {
        BrowserLocality::Remote
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_local_provider_reports_a_name() {
        let p = LocalSystemBrowserProvider::new();
        assert_eq!(p.name(), "system-browser");
        assert_eq!(p.locality(), BrowserLocality::Local);
        // availability depends on the host; both values are legal.
        let _ = p.available();
    }

    #[test]
    fn test_remote_provider_shape() {
        let p = RemoteBrowserProvider::new("http://127.0.0.1:9222");
        assert_eq!(p.name(), "remote-browser");
        assert_eq!(p.locality(), BrowserLocality::Remote);
        assert_eq!(p.endpoint(), "http://127.0.0.1:9222");
        assert!(p.available());
    }
}
