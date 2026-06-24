//! Testing utilities extracted from `shannon-core`.
//!
//! Currently only `record_replay` — moved here alongside `api/` because
//! `api::client` depends on it at runtime (fixture record/replay for
//! zero-cost CI testing). `record_replay` is a true leaf module (only
//! depends on serde + std) so it moves cleanly.

pub mod record_replay;
