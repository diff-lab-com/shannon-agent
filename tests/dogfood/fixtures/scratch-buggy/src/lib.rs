//! Scratch repo for dogfood write tasks — a tiny, always-green math/strings
//! crate. Copied per task into a fresh workspace (see scripts/dogfood/runner.py);
//! never edited in place.
//!
//! Public API: re-exports from [`math`] and [`strings`].

pub mod math;
pub mod strings;

pub use math::{add, is_positive, mean, multiply};
pub use strings::shout;
