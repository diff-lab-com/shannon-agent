//! # Shannon Core Memory
//!
//! This crate provides memory management systems for the Shannon CLI.
//!
//! ## Modules
//!
//! - [`types`] - Memory types (MemoryEntry, MemoryCategory, MemoryType, SessionMemoryConfig)
//! - [`store`] - Persistent storage for memory entries (MemoryStore)
//! - [`auto_dream`] - Pattern-based memory extraction (AutoDreamService)
//! - [`consolidator`] - Memory deduplication and cleanup (MemoryConsolidator)
//! - [`error`] - Memory operation errors (MemoryError)

pub mod error;
pub mod store;
pub mod types;
pub mod auto_dream;
pub mod consolidator;

// Re-exports
pub use types::{
    MemoryCategory, MemoryEntry, MemoryType, SessionMemoryConfig,
};
pub use store::MemoryStore;
pub use auto_dream::AutoDreamService;
pub use consolidator::{MemoryConsolidator, ConsolidationResult};
pub use error::MemoryError;
