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
//! - [`MemoryStore`]: CRUD + search + persistence for memory entries
//! - [`AutoDreamService`]: Pattern-based extraction of memories from conversations
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
pub use shannon_engine::api::{Message, MessageContent};

pub use auto_dream::AutoDreamService;
pub use compaction_trigger::{
    CompactSummary, CompactionState, DEFAULT_MAX_AGE, DEFAULT_MAX_SESSIONS, DEFAULT_TOKEN_BUDGET,
    MemoryCompactionTrigger,
};
pub use consolidator::{ConsolidationResult, MemoryConsolidator};
pub use error::MemoryError;
pub use store::{AddOutcome, GLOBAL_SCOPE, MemoryDoctorStats, MemoryStore};
pub use types::{MemoryCategory, MemoryEntry, MemoryType, SessionMemoryConfig};

pub mod tools;

// Private modules
mod auto_dream;
mod compaction_trigger;
mod consolidator;
mod error;
mod store;
mod types;

// Re-export the private error type as public
pub use error::MemoryError as Error;

/// Whether automatic memory extraction is enabled.
///
/// Two switches existed with zero consumers — the `auto_memory` feature flag
/// (`SHANNON_FEATURE_AUTO_MEMORY` / `settings.json` `features`) and the
/// `auto_memory` key in `config.toml` (`Settings`) — while the engine
/// extracted unconditionally. Both are now honored: either `false`
/// disables. Missing/unreadable config falls back to enabled (both default
/// true).
pub fn auto_memory_enabled() -> bool {
    if !crate::feature_flags::FeatureFlagManager::new()
        .is_enabled(&crate::feature_flags::flags::AUTO_MEMORY)
    {
        return false;
    }
    let mut manager = crate::SettingsManager::new();
    match manager.load() {
        Ok(()) => manager.settings().auto_memory,
        Err(_) => true,
    }
}
