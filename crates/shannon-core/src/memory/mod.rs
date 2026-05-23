//! # Auto-Dream: Automatic Memory Extraction and Persistence
//!
//! This module provides a system for automatically extracting important information
//! from conversations and persisting it across sessions.
//!
//! ## Architecture
//!
//! Memories are stored as JSON files under `~/.shannon/memories/`, one file per
//! project (keyed by a hash of the project path).
//!
//! - [`MemoryStore`](store::MemoryStore): CRUD + search + persistence for memory entries
//! - [`AutoDreamService`](auto_dream::AutoDreamService): Pattern-based extraction of memories from conversations
//!
//! ## Memory Categories
//!
//! Memories are classified into categories for better retrieval:
//! - **Preference**: User preferences ("always use tabs not spaces")
//! - **Pattern**: Code patterns observed
//! - **Decision**: Architectural decisions made
//! - **Error**: Recurring errors and solutions
//! - **Context**: Project-specific context

// Re-export all public types to maintain `crate::memory::*` paths
pub use crate::api::{Message, MessageContent};

pub use auto_dream::AutoDreamService;
pub use consolidator::{ConsolidationResult, MemoryConsolidator};
pub use error::MemoryError;
pub use store::MemoryStore;
pub use types::{MemoryCategory, MemoryEntry, MemoryType, SessionMemoryConfig};

// Private modules
mod auto_dream;
mod consolidator;
mod error;
mod store;
mod types;

// Re-export the private error type as public
pub use error::MemoryError as Error;
