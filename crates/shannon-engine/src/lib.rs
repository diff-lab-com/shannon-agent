//! # Shannon Engine
//!
//! LLM API client, streaming adapter, and testing utilities extracted from
//! `shannon-core` as part of the D1 Phase 2 reorganization
//! (see `docs/architecture/D1-PHASE1.md`).
//!
//! ## Current state (PR-B)
//!
//! The `api` module (client, adapter, types, streaming, retry, error) has been
//! physically moved here from `shannon-core/src/api/`. It is a near-leaf module:
//! the only non-leaf dependency (`ShannonConfig` from `unified_config`) was
//! resolved by moving the `From<ShannonConfig> for LlmClientConfig` impl into
//! `shannon-core`, where both types are visible.
//!
//! The `testing::record_replay` module was also moved here because `api::client`
//! depends on it at runtime for fixture record/replay. It is a true leaf module
//! (only depends on serde + std).
//!
//! This crate does NOT depend on `shannon-core`, breaking the cycle that
//! blocked PR-A's physical move of `compact/`.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use shannon_engine::api::{LlmClient, LlmClientConfig};
//! ```

pub mod api;
pub mod hooks;
pub mod testing;
