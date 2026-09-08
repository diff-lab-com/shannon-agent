//! Browser automation foundation shared by `shannon-tools` and
//! `shannon-remote` (B2 sink of the former duplicated layers).
//!
//! Decision 2026-09-06: Shannon never bundles a Chromium binary — browser
//! automation reuses the user's system Chrome/Chromium/Edge, or attaches to
//! an already-running Chrome over CDP (`SHANNON_BROWSER_CDP`, typically an
//! `ssh -L` forwarded port). Three modules:
//!
//! - [`detect`] — locate the installed browser, with doctor-renderable
//!   failure output. No chromiumoxide dependency.
//! - [`provider`] — the [`BrowserProvider`](shannon_tool_interface::providers::BrowserProvider)
//!   implementations (local system browser; remote CDP endpoint). No
//!   chromiumoxide dependency.
//! - [`session`] — the chromiumoxide session the builtin `browser_*` tools
//!   drive. Feature-gated behind `local-browser` because the generated CDP
//!   module is large; without the feature a stub with the same surface
//!   compiles so consumers need no cfg.
//!
//! This crate is a leaf: it depends only on `shannon-tool-interface`, so
//! both `shannon-tools` (tool structs) and `shannon-remote` (world
//! assembly) can consume it without a cycle.

pub mod detect;
pub mod provider;
pub mod session;
