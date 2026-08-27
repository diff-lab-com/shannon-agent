//! Test infrastructure for Shannon integration and regression testing.
//!
//! This module provides:
//! - **mock_dsl**: Unified mock response builder for Anthropic, OpenAI, Ollama
//! - **test_env**: TestShannonBuilder for one-call test environment setup
//! - **eval_runner**: Tiered L1 evaluation suite runner (§4.4; TOML tasks →
//!   sandboxed runs → dual JSON/Markdown reports)
//! - **eval_metrics**: per-task cost/trajectory metrics from the L0 log plus
//!   TOML-driven failure classification (§4.7 W2-M2)
//! - **eval_benchmarks**: external benchmark trio adapters — Terminal-Bench,
//!   SWE-bench Verified 50 subset, self-built regression pool (§4.13 W2-M3),
//!   with pinned-workload fingerprints, n=3 variance discipline and
//!   citability gating
//! - **dashboard**: static offline HTML trend board over `runs/*/report.json`
//!   (§4.15 W2-M4; version×metric matrix + chronological run sequence)
//! - **snapshot**: Request shape snapshot helpers for regression detection
//! - **record_replay**: Record/Replay system for zero-cost CI testing
//!   (moved to `shannon-engine`; re-exported here for backward compat)

pub mod dashboard;
pub mod eval_benchmarks;
pub mod eval_metrics;
pub mod eval_runner;
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
