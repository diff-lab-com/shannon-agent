//! Test infrastructure for Shannon integration and regression testing.
//!
//! This module provides:
//! - **mock_dsl**: Unified mock response builder for Anthropic, OpenAI, Ollama
//! - **test_env**: TestShannonBuilder for one-call test environment setup
//! - **snapshot**: Request shape snapshot helpers for regression detection
//! - **record_replay**: Record/Replay system for zero-cost CI testing
//!   (moved to `shannon-engine`; re-exported here for backward compat)

pub mod mock_dsl;
pub mod scenario;
pub mod snapshot;
pub mod test_env;

#[deprecated(
    since = "0.5.6",
    note = "moved to shannon-engine; use `shannon_engine::testing::record_replay` directly"
)]
pub mod record_replay {
    pub use ::shannon_engine::testing::record_replay::*;
}
