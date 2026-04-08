//! # Shannon Core Memory
//!
//! Memory management and persistence system.
//!
//! This crate provides memory-related functionality:
//! - Memory storage and retrieval
//! - Project memory management
//! - Memory extraction and consolidation
//! - Session transcripts and history
//! - Team memory synchronization

pub mod memory;
pub mod project_memory;
pub mod session_history;
pub mod session_transcript;

// Re-export key types
pub use memory::{MemoryStore, Memory, MemorySearchOptions};
pub use project_memory::{ProjectMemoryManager, ProjectMemory};
pub use session_history::{SessionHistoryManager, SessionEntry};
pub use session_transcript::{TranscriptStore, Transcript};
