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

impl Default for MemoryConsolidator {
    fn default() -> Self {
        Self::new(0.8)
    }
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::memory::types::{MemoryCategory, MemoryEntry};
    use chrono::{DateTime, Duration, Utc};
    use tempfile::TempDir;

    #[allow(clippy::too_many_arguments)]
    fn make_entry_with_timestamps(
        id: &str,
        project: &str,
        category: MemoryCategory,
        content: &str,
        confidence: f64,
        created_at: DateTime<Utc>,
        accessed_at: DateTime<Utc>,
        access_count: u32,
    ) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            project: project.to_string(),
            category,
            content: content.to_string(),
            tags: vec![],
            confidence,
            created_at,
            accessed_at,
            access_count,
        }
    }

    fn make_entry_with_confidence(
        project: &str,
        category: MemoryCategory,
        content: &str,
        confidence: f64,
    ) -> MemoryEntry {
        MemoryEntry::with_confidence(project, category, content, confidence, vec![]).unwrap()
    }

    #[test]
    fn test_default_threshold_is_0_8() {
        let consolidator = MemoryConsolidator::default();
        assert!((consolidator.similarity_threshold - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_custom_threshold_clamps_negative() {
        let consolidator = MemoryConsolidator::new(-1.0);
        assert!((consolidator.similarity_threshold - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_custom_threshold_clamps_above_one() {
        let consolidator = MemoryConsolidator::new(2.5);
        assert!((consolidator.similarity_threshold - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_custom_threshold_keeps_valid_value() {
        let consolidator = MemoryConsolidator::new(0.65);
        assert!((consolidator.similarity_threshold - 0.65).abs() < f64::EPSILON);
    }

    #[test]
    fn test_consolidate_merges_duplicates() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation",
                0.9,
            ))
            .unwrap();
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation",
                0.7,
            ))
            .unwrap();
        let config = SessionMemoryConfig::default();
        let consolidator = MemoryConsolidator::default();
        let result = consolidator.consolidate(&mut store, &config).unwrap();
        assert_eq!(result.before_count, 2);
        assert_eq!(result.duplicates_merged, 1);
        assert_eq!(result.after_count, 1);
    }

    #[test]
    fn test_consolidate_removes_stale() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let old = make_entry_with_timestamps(
            "1",
            "p",
            MemoryCategory::Context,
            "old entry",
            1.0,
            Utc::now() - Duration::days(100),
            Utc::now(),
            0,
        );
        let fresh = make_entry_with_timestamps(
            "2",
            "p",
            MemoryCategory::Context,
            "fresh entry",
            1.0,
            Utc::now(),
            Utc::now(),
            0,
        );
        store.add(old).unwrap();
        store.add(fresh).unwrap();
        let config = SessionMemoryConfig::default();
        let consolidator = MemoryConsolidator::default();
        let result = consolidator.consolidate(&mut store, &config).unwrap();
        assert_eq!(result.stale_removed, 1);
        assert_eq!(result.after_count, 1);
    }

    #[test]
    fn test_consolidate_enforces_category_caps() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        for i in 0..4u32 {
            let entry = make_entry_with_timestamps(
                &format!("p{i}"),
                "proj",
                MemoryCategory::Preference,
                &format!("preference entry {i}"),
                0.8,
                Utc::now(),
                Utc::now(),
                i,
            );
            store.add(entry).unwrap();
        }
        let config = SessionMemoryConfig {
            max_memories_per_category: 2,
            ..SessionMemoryConfig::default()
        };
        let consolidator = MemoryConsolidator::default();
        let result = consolidator.consolidate(&mut store, &config).unwrap();
        assert!(result.after_count <= 2);
    }

    #[test]
    fn test_consolidate_empty_store() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let config = SessionMemoryConfig::default();
        let consolidator = MemoryConsolidator::default();
        let result = consolidator.consolidate(&mut store, &config).unwrap();
        assert_eq!(result.before_count, 0);
        assert_eq!(result.duplicates_merged, 0);
        assert_eq!(result.stale_removed, 0);
        assert_eq!(result.after_count, 0);
    }

    #[test]
    fn test_consolidate_all_phases_combined() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "always use rust language",
                0.9,
            ))
            .unwrap();
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "always use rust language",
                0.6,
            ))
            .unwrap();
        let stale = make_entry_with_timestamps(
            "stale",
            "p",
            MemoryCategory::Context,
            "stale data",
            1.0,
            Utc::now() - Duration::days(100),
            Utc::now(),
            0,
        );
        store.add(stale).unwrap();
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Decision,
                "use PostgreSQL",
                0.85,
            ))
            .unwrap();
        let config = SessionMemoryConfig {
            max_memories_per_category: 50,
            ..SessionMemoryConfig::default()
        };
        let consolidator = MemoryConsolidator::default();
        let result = consolidator.consolidate(&mut store, &config).unwrap();
        assert_eq!(result.before_count, 4);
        assert_eq!(result.duplicates_merged, 1);
        assert_eq!(result.stale_removed, 1);
        assert_eq!(result.after_count, 2);
    }

    #[test]
    fn test_consolidation_result_serialization() {
        let result = ConsolidationResult {
            duplicates_merged: 2,
            stale_removed: 1,
            before_count: 10,
            after_count: 7,
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: ConsolidationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.duplicates_merged, 2);
        assert_eq!(deserialized.stale_removed, 1);
        assert_eq!(deserialized.before_count, 10);
        assert_eq!(deserialized.after_count, 7);
    }
}
