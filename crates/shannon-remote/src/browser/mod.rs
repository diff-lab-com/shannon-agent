//! Browser automation world (T14).
//!
//! Decision 2026-09-06: Shannon never bundles a Chromium binary — browser
//! automation reuses the user's system Chrome/Chromium/Edge. This module
//! provides system-browser detection plus the [`BrowserProvider`]
//! implementations (local system browser; remote CDP endpoint behind an
//! SSH forward). The chromiumoxide session layer that consumes them lives
//! in `shannon-tools::chrome_session`.

pub mod detect;
pub mod provider;

pub use detect::{
    BrowserExecutable, DetectError, candidate_paths, detect_system_browser, install_hint,
};
pub use provider::{LocalSystemBrowserProvider, RemoteBrowserProvider};
