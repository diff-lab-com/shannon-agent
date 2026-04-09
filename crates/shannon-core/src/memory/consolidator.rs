// Consolidation logic for merging and pruning memories
use serde::{Deserialize, Serialize};

use super::error::MemoryError;
use super::store::MemoryStore;
use super::types::SessionMemoryConfig;

// ============================================================================
// Consolidation
// ============================================================================

/// Result of a memory consolidation pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationResult {
    /// Number of duplicate memories merged.
    pub duplicates_merged: usize,
    /// Number of stale memories removed.
    pub stale_removed: usize,
    /// Total memories before consolidation.
    pub before_count: usize,
    /// Total memories after consolidation.
    pub after_count: usize,
}

/// Deduplicates and merges memories in a [`MemoryStore`].
pub struct MemoryConsolidator {
    /// Jaccard similarity threshold above which two memories are considered duplicates.
    similarity_threshold: f64,
}

impl MemoryConsolidator {
    /// Create a new consolidator with the given similarity threshold.
    ///
    /// Two memories with Jaccard similarity above `similarity_threshold`
    /// are considered duplicates and will be merged.
    pub fn new(similarity_threshold: f64) -> Self {
        Self {
            similarity_threshold: similarity_threshold.clamp(0.0, 1.0),
        }
    }

    /// Create a consolidator with the default threshold of 0.8.
    pub fn default() -> Self {
        Self::new(0.8)
    }

    /// Run consolidation on the given memory store.
    ///
    /// 1. Merges duplicates (keeps the entry with higher confidence).
    /// 2. Removes entries older than the configured TTL.
    /// 3. Enforces per-category caps.
    pub fn consolidate(
        &self,
        store: &mut MemoryStore,
        config: &SessionMemoryConfig,
    ) -> Result<ConsolidationResult, MemoryError> {
        let before_count = store.len();

        // Phase 1: Merge duplicates
        let duplicates_merged = store.merge_duplicates(self.similarity_threshold)?;

        // Phase 2: Remove stale entries
        let stale_removed = store.remove_stale(config.memory_ttl)?;

        // Phase 3: Enforce per-category caps
        store.enforce_category_caps(config.max_memories_per_category);

        let after_count = store.len();

        Ok(ConsolidationResult {
            duplicates_merged,
            stale_removed,
            before_count,
            after_count,
        })
    }
}
