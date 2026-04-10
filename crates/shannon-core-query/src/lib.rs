//! # Shannon Core Query
//!
//! Query processing and orchestration engine.
//!
//! This crate provides the core query engine that coordinates:
//! - Query context and state management
//! - LLM API interactions
//! - Tool execution
//! - Message compaction and optimization
//! - Response streaming
//!
//! ## Re-exports

pub mod query_engine;
pub mod compact;

// Re-export key types
pub use query_engine::{QueryEngine, QueryContext, QueryOptions, QueryResponse};
pub use compact::{CompactEngine, CompactStrategy, MessageGroup};
