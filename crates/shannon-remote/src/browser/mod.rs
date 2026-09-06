//! Browser automation world (T14 Phase 1 foundation).
//!
//! Decision 2026-09-06: Shannon never bundles a Chromium binary — browser
//! automation reuses the user's system Chrome/Chromium/Edge. This module
//! currently provides system-browser detection; the chromiumoxide session
//! layer lands on top of [`detect`] next.

pub mod detect;

pub use detect::{
    BrowserExecutable, DetectError, candidate_paths, detect_system_browser, install_hint,
};
