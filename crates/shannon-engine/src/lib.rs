//! # Shannon Engine
//!
//! Context compression and conversation management for Shannon Code.
//!
//! This crate is the future home of the query engine, state, permissions,
//! and compact modules (D1 Phase 2 — see `docs/architecture/D1-PHASE1.md`).
//!
//! ## Current state (PR-A)
//!
//! Only the `compact` module is exposed. The code physically remains in
//! `shannon-core` because `compact/` has compile-time dependencies on
//! `shannon-core`'s `api`, `hooks`, and `context_budget` modules. Moving
//! the source files into this crate would create a cyclic dependency
//! (`shannon-core → shannon-engine` for the backward-compat shim, and
//! `shannon-engine → shannon-core` for the three dep modules).
//!
//! To break the cycle, the extraction is staged:
//! 1. **PR-A (this PR)**: establish `shannon-engine` as the canonical
//!    import path for `compact`, re-exported from `shannon-core`. External
//!    consumers can begin migrating to `shannon_engine::compact`.
//! 2. **PR-B+**: once `hooks/`, `api/`, and `context_budget` are extracted
//!    into `shannon-engine` (or stubbed via traits in `shannon-types`),
//!    physically move `compact/` source into this crate.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use shannon_engine::compact::{CompactEngine, CompactConfig};
//! ```

pub use shannon_core::compact;
pub use shannon_core::compact::*;
