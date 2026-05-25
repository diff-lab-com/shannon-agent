//! Test infrastructure for Shannon integration and regression testing.
//!
//! This module provides:
//! - **mock_dsl**: Unified mock response builder for Anthropic, OpenAI, Ollama
//! - **test_env**: TestShannonBuilder for one-call test environment setup
//! - **snapshot**: Request shape snapshot helpers for regression detection
//! - **record_replay**: Record/Replay system for zero-cost CI testing

pub mod mock_dsl;
pub mod record_replay;
pub mod snapshot;
pub mod test_env;
