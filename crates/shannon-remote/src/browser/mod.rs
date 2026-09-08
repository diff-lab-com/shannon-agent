//! Browser automation world (T14 + B2).
//!
//! Decision 2026-09-06: Shannon never bundles a Chromium binary — browser
//! automation reuses the user's system Chrome/Chromium/Edge. Detection and
//! the [`BrowserProvider`](shannon_tool_interface::providers::BrowserProvider)
//! implementations moved to the `shannon-browser` leaf crate (B2) so
//! `shannon-tools`' session layer and this crate's providers share one
//! implementation; the paths below are kept stable for `shannon-ui` and
//! the world assembly. The chromiumoxide session that consumes the
//! providers lives in `shannon-browser::session` behind its
//! `local-browser` feature.

pub use shannon_browser::detect;
pub use shannon_browser::provider;

pub use detect::{
    BrowserExecutable, DetectError, candidate_paths, detect_system_browser, install_hint,
};
pub use provider::{LocalSystemBrowserProvider, RemoteBrowserProvider};
