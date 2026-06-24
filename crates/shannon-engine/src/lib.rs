//! # Shannon Engine
//!
//! LLM API client, streaming adapter, context compression, and testing
//! utilities extracted from `shannon-core` as part of the D1 Phase 2
//! reorganization (see `docs/architecture/D1-PHASE1.md`).
//!
//! ## Current state (PR-E)
//!
//! The `api` module (client, adapter, types, streaming, retry, error) was
//! physically moved here from `shannon-core/src/api/` (PR-B). It is a
//! near-leaf module: the only non-leaf dependency (`ShannonConfig` from
//! `unified_config`) was resolved by moving the `From<ShannonConfig> for
//! LlmClientConfig` impl into `shannon-core`, where both types are visible.
//!
//! The `testing::record_replay` module was also moved here (PR-B) because
//! `api::client` depends on it at runtime for fixture record/replay.
//!
//! The `hooks` module was moved here (PR-C).
//!
//! The `compact`, `context_budget`, and `context_pressure` modules were moved
//! here (PR-D). They form a tightly coupled cluster:
//! - `compact` depends on `context_budget` (MessagePriority)
//! - `context_budget` depends on `context_pressure` (PressureLevel)
//! - `context_pressure` depends on `compact` (CompactStrategy)
//!
//! All three only depend on `api` (already here) and each other, so they
//! could be extracted together now that the cycle with `shannon-core` is broken.
//!
//! The `permissions`, `permission_profile`, and `permission_classifier`
//! modules were moved here (PR-E). They form a tightly coupled cluster:
//! - `permissions` depends on `permission_profile` (PermissionProfile, ProfileRules)
//! - `permission_classifier` is standalone (no `crate::` imports)
//! - `permission_profile` is standalone (no `crate::` imports)
//!
//! The `permissions` module had one production dependency on
//! `settings::PermissionRules` — resolved by decoupling
//! `PermissionRuleChecker::from_rules(&PermissionRules)` to
//! `PermissionRuleChecker::from_rule_strings(&[String], &[String], &[String])`.
//! A backward-compatible bridge is provided in `shannon-core` via the
//! `PermissionRuleCheckerExt` trait.
//!
//! This crate does NOT depend on `shannon-core`.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use shannon_engine::api::{LlmClient, LlmClientConfig};
//! use shannon_engine::compact::{CompactEngine, CompactConfig};
//! ```

pub mod api;
pub mod compact;
pub mod context_budget;
pub mod context_pressure;
pub mod custom_profiles;
pub mod hooks;
pub mod llm_classifier;
pub mod permission_classifier;
pub mod permission_profile;
pub mod permissions;
pub mod state;
pub mod streaming_tool_executor;
pub mod testing;
