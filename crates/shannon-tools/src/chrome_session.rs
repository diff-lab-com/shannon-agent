//! Browser session facade (B2).
//!
//! The session layer — chromiumoxide launch/connect, page registry, console
//! buffering — lives in [`shannon_browser::session`], the leaf crate shared
//! with `shannon-remote`'s browser providers. This module is a pure
//! re-export so `browser_tools` and the tool registry keep their
//! `crate::chrome_session::…` paths; the live/stub split is resolved inside
//! `shannon-browser` by its `local-browser` feature (forwarded from this
//! crate's feature of the same name).

pub use shannon_browser::session::*;
