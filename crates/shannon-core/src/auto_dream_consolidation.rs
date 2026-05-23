//! # Auto-Dream Consolidation
//!
//! Provides a lock-based consolidation system for the auto-dream memory service.
//! Consolidation merges duplicate memories, removes stale entries, and builds
//! prompts for AI-assisted memory cleanup.
//!
//! Based on Claude Code's `consolidationLock.ts` and `consolidationPrompt.ts`.
//!
//! ## Architecture
//!
//! - [`ConsolidationLock`]: Mutual-exclusion lock with minimum interval enforcement
//! - [`ConsolidationGuard`]: RAII guard that tracks consolidation duration
//! - [`ConsolidationPrompt`]: Builds prompts for AI-assisted memory consolidation
//! - [`ConsolidationConfig`]: Configuration for consolidation behavior
//! - [`ConsolidationResult`]: Result statistics from a consolidation pass

use crate::memory::{MemoryCategory, MemoryEntry};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during auto-dream consolidation.
#[derive(Error, Debug)]
pub enum ConsolidationError {
    #[error("Consolidation already in progress")]
    AlreadyInProgress,

    #[error("Consolidation interval not elapsed: last run {last:?}, min interval {min_secs}s")]
    IntervalNotElapsed {
        last: Option<DateTime<Utc>>,
        min_secs: u64,
    },

    #[error("No memories to consolidate")]
    NoMemories,

    #[error("Consolidation failed: {0}")]
    Failed(String),
}

// ============================================================================
// Consolidation Lock
// ============================================================================

/// RAII guard returned when a consolidation lock is successfully acquired.
///
/// While this guard is held, no other consolidation can run. When dropped,
/// the lock is released and the last-consolidation timestamp is updated.
pub struct ConsolidationGuard {
    in_progress: Arc<AtomicBool>,
    last_consolidation: Arc<Mutex<Option<DateTime<Utc>>>>,
    start_time: DateTime<Utc>,
}

impl ConsolidationGuard {
    /// Returns the elapsed time since this guard was created.
    pub fn elapsed_ms(&self) -> u64 {
        let elapsed = Utc::now() - self.start_time;
        elapsed.num_milliseconds() as u64
    }
}

impl Drop for ConsolidationGuard {
    fn drop(&mut self) {
        // Update last consolidation timestamp
        if let Ok(mut guard) = self.last_consolidation.lock() {
            *guard = Some(Utc::now());
        }
        // Release the lock
        self.in_progress.store(false, Ordering::SeqCst);
    }
}

/// Mutual-exclusion lock for memory consolidation operations.
///
/// Ensures that consolidation runs at most once at a time, and enforces a
/// minimum interval between consecutive consolidation passes.
pub struct ConsolidationLock {
    in_progress: Arc<AtomicBool>,
    last_consolidation: Arc<Mutex<Option<DateTime<Utc>>>>,
    min_interval: Duration,
}

impl ConsolidationLock {
    /// Create a new consolidation lock with the given minimum interval.
    ///
    /// # Arguments
    ///
    /// * `min_interval` - Minimum duration between consolidation passes.
    ///   `try_acquire` will return `None` if this interval has not elapsed.
    pub fn new(min_interval: Duration) -> Self {
        Self {
            in_progress: Arc::new(AtomicBool::new(false)),
            last_consolidation: Arc::new(Mutex::new(None)),
            min_interval,
        }
    }

    /// Try to acquire the consolidation lock.
    ///
    /// Returns `Some(ConsolidationGuard)` if the lock was successfully acquired,
    /// which means:
    /// 1. No other consolidation is currently in progress.
    /// 2. The minimum interval since the last consolidation has elapsed (or this
    ///    is the first consolidation).
    ///
    /// Returns `None` if either condition is not met.
    pub fn try_acquire(&self) -> Option<ConsolidationGuard> {
        // Check if consolidation is already in progress
        if self.in_progress.load(Ordering::SeqCst) {
            return None;
        }

        // Check minimum interval
        if let Ok(guard) = self.last_consolidation.lock() {
            if let Some(last) = *guard {
                let elapsed = Utc::now() - last;
                if elapsed < self.min_interval {
                    return None;
                }
            }
        }

        // Try to acquire the lock (compare-and-swap)
        if self
            .in_progress
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            Some(ConsolidationGuard {
                in_progress: Arc::clone(&self.in_progress),
                last_consolidation: Arc::clone(&self.last_consolidation),
                start_time: Utc::now(),
            })
        } else {
            None
        }
    }

    /// Check whether a consolidation is currently in progress.
    pub fn is_in_progress(&self) -> bool {
        self.in_progress.load(Ordering::SeqCst)
    }

    /// Get the time of the last consolidation, if any.
    pub fn last_consolidation(&self) -> Option<DateTime<Utc>> {
        self.last_consolidation.lock().ok().and_then(|g| *g)
    }

    /// Reset the lock (for testing purposes).
    pub fn reset(&self) {
        self.in_progress.store(false, Ordering::SeqCst);
        if let Ok(mut guard) = self.last_consolidation.lock() {
            *guard = None;
        }
    }
}

// ============================================================================
// Consolidation Config
// ============================================================================

/// Configuration for auto-dream consolidation behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationConfig {
    /// Whether consolidation is enabled.
    pub enabled: bool,
    /// Minimum seconds between consolidation passes.
    pub min_interval_secs: u64,
    /// Trigger consolidation when memory count exceeds this threshold.
    pub max_memories_before_consolidation: usize,
    /// Jaccard similarity threshold for duplicate detection.
    pub similarity_threshold: f64,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_interval_secs: 300, // 5 minutes
            max_memories_before_consolidation: 200,
            similarity_threshold: 0.8,
        }
    }
}

impl ConsolidationConfig {
    /// Create a disabled configuration.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }
}

// ============================================================================
// Consolidation Prompt Builder
// ============================================================================

/// Builds prompts for AI-assisted memory consolidation.
///
/// Given a list of memories, this constructs a prompt that instructs an AI
/// model to identify duplicates, stale entries, and merge candidates.
pub struct ConsolidationPrompt;

impl ConsolidationPrompt {
    /// Build a consolidation prompt for the given memories.
    ///
    /// The prompt instructs the model to analyze the memories and output
    /// structured JSON indicating which entries to merge, remove, or keep.
    pub fn build(memories: &[MemoryEntry]) -> String {
        if memories.is_empty() {
            return String::new();
        }

        let memory_list = memories
            .iter()
            .enumerate()
            .map(|(i, m)| {
                format!(
                    "[{}] Category: {} | Confidence: {:.2} | Created: {} | Accesses: {} | Content: {}",
                    i,
                    m.category,
                    m.confidence,
                    m.created_at.format("%Y-%m-%d %H:%M"),
                    m.access_count,
                    m.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "You are a memory consolidation assistant. Analyze the following memories \
             and identify duplicates, stale entries, and merge candidates.\n\n\
             ## Consolidation Rules\n\n\
             {}\n\n\
             ## Memories to Analyze\n\n\
             {}\n\n\
             ## Output Format\n\n\
             Respond with a JSON object:\n\
             {{\n\
               \"keep\": [indices to keep],\n\
               \"merge\": [[group of indices to merge into one]],\n\
               \"remove\": [indices to remove as stale]\n\
             }}\n\n\
             Provide your analysis and JSON output:",
            Self::build_rules(),
            memory_list
        )
    }

    /// Build a condensed consolidation prompt for a small number of memories.
    ///
    /// Uses a more compact format when there are few memories to process.
    pub fn build_condensed(memories: &[MemoryEntry]) -> String {
        if memories.is_empty() {
            return String::new();
        }

        let summary = memories
            .iter()
            .map(|m| {
                format!(
                    "- [{}] \"{}\" (cat={}, conf={:.2}, age={}d, hits={})",
                    m.id.chars().take(8).collect::<String>(),
                    m.content.chars().take(80).collect::<String>(),
                    m.category,
                    m.confidence,
                    (Utc::now() - m.created_at).num_days(),
                    m.access_count
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "Quick consolidation review for {} memories:\n\n{}\n\n\
             Rules: {}\n\n\
             List indices to merge or remove.",
            memories.len(),
            summary,
            Self::build_rules()
        )
    }

    /// Return the consolidation rules text.
    ///
    /// These rules define the criteria for merging, keeping, and removing
    /// memories during consolidation.
    pub fn build_rules() -> String {
        "1. DUPLICATE DETECTION: Two memories are duplicates if they express \
         the same information, even with different wording. Prefer keeping \
         the one with higher confidence and more recent access.\n\n\
         2. STALE ENTRIES: Remove memories that:\n\
            - Have not been accessed in 30+ days AND have low confidence (<0.5)\n\
            - Are contradicted by more recent memories of the same category\n\
            - Contain information that is no longer relevant\n\n\
         3. MERGE CRITERIA: Merge memories that:\n\
            - Share the same category and overlap in content\n\
            - One is a more detailed/updated version of the other\n\
            - Together they form a more complete picture\n\n\
         4. PRESERVE IMPORTANT MEMORIES: Never remove memories that:\n\
            - Have high confidence (>0.8) AND recent access\n\
            - Are the only memory in their category for a project\n\
            - Contain unique information not found in other memories\n\n\
         5. CONFLICT RESOLUTION: When two memories contradict:\n\
            - Keep the more recent one\n\
            - If timestamps are similar, keep the one with higher confidence\n\
            - If both are high-confidence, keep both and flag for review"
            .to_string()
    }

    /// Build a category summary prompt for understanding memory distribution.
    pub fn build_category_summary(memories: &[MemoryEntry]) -> String {
        let mut by_category: std::collections::HashMap<MemoryCategory, usize> =
            std::collections::HashMap::new();
        for m in memories {
            *by_category.entry(m.category.clone()).or_insert(0) += 1;
        }

        let summary = by_category
            .iter()
            .map(|(cat, count)| format!("- {cat}: {count}"))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "Memory distribution:\n{}\n\nTotal: {} memories",
            summary,
            memories.len()
        )
    }
}

// ============================================================================
// Consolidation Result (enhanced)
// ============================================================================

/// Detailed result of an auto-dream consolidation pass.
///
/// Extends the basic [`crate::memory::ConsolidationResult`] with duration
/// tracking and a richer set of statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedConsolidationResult {
    /// Total memories before consolidation.
    pub memories_before: usize,
    /// Total memories after consolidation.
    pub memories_after: usize,
    /// Number of duplicate memories merged.
    pub duplicates_merged: usize,
    /// Number of stale memories removed.
    pub stale_removed: usize,
    /// Consolidation duration in milliseconds.
    pub duration_ms: u64,
}

impl EnhancedConsolidationResult {
    /// Create a new consolidation result.
    pub fn new(
        memories_before: usize,
        memories_after: usize,
        duplicates_merged: usize,
        stale_removed: usize,
        duration_ms: u64,
    ) -> Self {
        Self {
            memories_before,
            memories_after,
            duplicates_merged,
            stale_removed,
            duration_ms,
        }
    }

    /// Calculate the reduction percentage.
    pub fn reduction_percentage(&self) -> f64 {
        if self.memories_before == 0 {
            return 0.0;
        }
        let removed = self.memories_before - self.memories_after;
        (removed as f64 / self.memories_before as f64) * 100.0
    }
}

impl std::fmt::Display for EnhancedConsolidationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Consolidation complete: {} -> {} memories ({:.1}% reduction), \
             {} merged, {} stale removed, took {}ms",
            self.memories_before,
            self.memories_after,
            self.reduction_percentage(),
            self.duplicates_merged,
            self.stale_removed,
            self.duration_ms,
        )
    }
}

// ============================================================================
// Consolidation eligibility checker
// ============================================================================

/// Check whether consolidation should be triggered based on memory count
/// and configuration.
pub fn should_consolidate(memory_count: usize, config: &ConsolidationConfig) -> bool {
    if !config.enabled {
        return false;
    }
    memory_count >= config.max_memories_before_consolidation
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryEntry;

    // ---------------------------------------------------------------------------
    // ConsolidationLock tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_lock_new() {
        let lock = ConsolidationLock::new(Duration::seconds(60));
        assert!(!lock.is_in_progress());
        assert!(lock.last_consolidation().is_none());
    }

    #[test]
    fn test_lock_acquire_first_time() {
        let lock = ConsolidationLock::new(Duration::seconds(60));
        let guard = lock.try_acquire();
        assert!(guard.is_some());
        assert!(lock.is_in_progress());
    }

    #[test]
    fn test_lock_prevents_double_acquire() {
        let lock = ConsolidationLock::new(Duration::seconds(60));
        let _guard = lock.try_acquire().unwrap();
        // Second acquire should fail
        let second = lock.try_acquire();
        assert!(second.is_none());
        assert!(lock.is_in_progress());
    }

    #[test]
    fn test_lock_release_on_drop() {
        let lock = ConsolidationLock::new(Duration::seconds(60));
        {
            let _guard = lock.try_acquire().unwrap();
            assert!(lock.is_in_progress());
        }
        assert!(!lock.is_in_progress());
        assert!(lock.last_consolidation().is_some());
    }

    #[test]
    fn test_lock_interval_enforcement() {
        let lock = ConsolidationLock::new(Duration::seconds(10));
        // First acquisition
        {
            let _guard = lock.try_acquire().unwrap();
        }
        // Immediately try again -- should fail because interval hasn't elapsed
        let second = lock.try_acquire();
        assert!(second.is_none());
    }

    #[test]
    fn test_lock_interval_allows_after_elapsed() {
        let lock = ConsolidationLock::new(Duration::milliseconds(50));
        {
            let _guard = lock.try_acquire().unwrap();
        }
        // Wait a bit for the interval to elapse
        std::thread::sleep(std::time::Duration::from_millis(100));
        let second = lock.try_acquire();
        assert!(second.is_some());
    }

    #[test]
    fn test_lock_reset() {
        let lock = ConsolidationLock::new(Duration::seconds(60));
        {
            let _guard = lock.try_acquire().unwrap();
        }
        lock.reset();
        assert!(!lock.is_in_progress());
        assert!(lock.last_consolidation().is_none());
        // Should be able to acquire immediately after reset
        assert!(lock.try_acquire().is_some());
    }

    #[test]
    fn test_guard_elapsed_ms() {
        let lock = ConsolidationLock::new(Duration::seconds(60));
        let guard = lock.try_acquire().unwrap();
        // Should return a reasonable value (>= 0)
        assert!(guard.elapsed_ms() < 1000);
    }

    // ---------------------------------------------------------------------------
    // ConsolidationConfig tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_config_default() {
        let config = ConsolidationConfig::default();
        assert!(config.enabled);
        assert_eq!(config.min_interval_secs, 300);
        assert_eq!(config.max_memories_before_consolidation, 200);
        assert!((config.similarity_threshold - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_config_disabled() {
        let config = ConsolidationConfig::disabled();
        assert!(!config.enabled);
        assert_eq!(config.min_interval_secs, 300);
    }

    #[test]
    fn test_config_serialization() {
        let config = ConsolidationConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ConsolidationConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.min_interval_secs, config.min_interval_secs);
    }

    // ---------------------------------------------------------------------------
    // ConsolidationPrompt tests
    // ---------------------------------------------------------------------------

    fn make_test_memories() -> Vec<MemoryEntry> {
        vec![
            MemoryEntry::new(
                "proj",
                MemoryCategory::Preference,
                "Always use tabs for indentation",
            ),
            MemoryEntry::new(
                "proj",
                MemoryCategory::Decision,
                "Use PostgreSQL for the database",
            ),
            MemoryEntry::new(
                "proj",
                MemoryCategory::Error,
                "The fix was adding a mutex to prevent race conditions",
            ),
        ]
    }

    #[test]
    fn test_build_prompt_nonempty() {
        let memories = make_test_memories();
        let prompt = ConsolidationPrompt::build(&memories);
        assert!(!prompt.is_empty());
        assert!(prompt.contains("memory consolidation"));
        assert!(prompt.contains("[0]"));
        assert!(prompt.contains("[1]"));
        assert!(prompt.contains("[2]"));
    }

    #[test]
    fn test_build_prompt_empty() {
        let prompt = ConsolidationPrompt::build(&[]);
        assert!(prompt.is_empty());
    }

    #[test]
    fn test_build_prompt_includes_rules() {
        let memories = make_test_memories();
        let prompt = ConsolidationPrompt::build(&memories);
        assert!(prompt.contains("DUPLICATE DETECTION"));
        assert!(prompt.contains("STALE ENTRIES"));
        assert!(prompt.contains("MERGE CRITERIA"));
    }

    #[test]
    fn test_build_prompt_includes_memory_details() {
        let memories = make_test_memories();
        let prompt = ConsolidationPrompt::build(&memories);
        assert!(prompt.contains("Always use tabs"));
        assert!(prompt.contains("PostgreSQL"));
        assert!(prompt.contains("mutex"));
    }

    #[test]
    fn test_build_rules_not_empty() {
        let rules = ConsolidationPrompt::build_rules();
        assert!(!rules.is_empty());
        assert!(rules.contains("DUPLICATE DETECTION"));
        assert!(rules.contains("STALE ENTRIES"));
        assert!(rules.contains("MERGE CRITERIA"));
        assert!(rules.contains("PRESERVE IMPORTANT"));
        assert!(rules.contains("CONFLICT RESOLUTION"));
    }

    #[test]
    fn test_build_condensed_prompt() {
        let memories = make_test_memories();
        let prompt = ConsolidationPrompt::build_condensed(&memories);
        assert!(!prompt.is_empty());
        assert!(prompt.contains("Quick consolidation"));
        assert!(prompt.contains("3 memories"));
    }

    #[test]
    fn test_build_condensed_empty() {
        let prompt = ConsolidationPrompt::build_condensed(&[]);
        assert!(prompt.is_empty());
    }

    #[test]
    fn test_build_category_summary() {
        let memories = make_test_memories();
        let summary = ConsolidationPrompt::build_category_summary(&memories);
        assert!(summary.contains("preference: 1"));
        assert!(summary.contains("decision: 1"));
        assert!(summary.contains("error: 1"));
        assert!(summary.contains("Total: 3"));
    }

    // ---------------------------------------------------------------------------
    // EnhancedConsolidationResult tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_enhanced_result_new() {
        let result = EnhancedConsolidationResult::new(100, 85, 10, 5, 250);
        assert_eq!(result.memories_before, 100);
        assert_eq!(result.memories_after, 85);
        assert_eq!(result.duplicates_merged, 10);
        assert_eq!(result.stale_removed, 5);
        assert_eq!(result.duration_ms, 250);
    }

    #[test]
    fn test_enhanced_result_reduction_percentage() {
        let result = EnhancedConsolidationResult::new(100, 75, 15, 10, 100);
        let pct = result.reduction_percentage();
        assert!((pct - 25.0).abs() < 0.01);
    }

    #[test]
    fn test_enhanced_result_reduction_zero_count() {
        let result = EnhancedConsolidationResult::new(0, 0, 0, 0, 0);
        assert_eq!(result.reduction_percentage(), 0.0);
    }

    #[test]
    fn test_enhanced_result_display() {
        let result = EnhancedConsolidationResult::new(100, 85, 10, 5, 250);
        let display = format!("{result}");
        assert!(display.contains("100 -> 85"));
        assert!(display.contains("15.0% reduction"));
        assert!(display.contains("10 merged"));
        assert!(display.contains("5 stale removed"));
        assert!(display.contains("250ms"));
    }

    #[test]
    fn test_enhanced_result_serialization() {
        let result = EnhancedConsolidationResult::new(50, 40, 7, 3, 120);
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: EnhancedConsolidationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.memories_before, 50);
        assert_eq!(deserialized.duration_ms, 120);
    }

    // ---------------------------------------------------------------------------
    // should_consolidate tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_should_consolidate_disabled() {
        let config = ConsolidationConfig::disabled();
        assert!(!should_consolidate(1000, &config));
    }

    #[test]
    fn test_should_consolidate_below_threshold() {
        let config = ConsolidationConfig::default();
        assert!(!should_consolidate(50, &config));
    }

    #[test]
    fn test_should_consolidate_at_threshold() {
        let config = ConsolidationConfig::default();
        assert!(should_consolidate(200, &config));
    }

    #[test]
    fn test_should_consolidate_above_threshold() {
        let config = ConsolidationConfig::default();
        assert!(should_consolidate(500, &config));
    }

    // ---------------------------------------------------------------------------
    // ConsolidationError tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_error_already_in_progress() {
        let err = ConsolidationError::AlreadyInProgress;
        assert!(err.to_string().contains("already in progress"));
    }

    #[test]
    fn test_error_interval_not_elapsed() {
        let err = ConsolidationError::IntervalNotElapsed {
            last: Some(Utc::now()),
            min_secs: 300,
        };
        let msg = err.to_string();
        assert!(msg.contains("interval not elapsed"));
        assert!(msg.contains("300"));
    }

    #[test]
    fn test_error_no_memories() {
        let err = ConsolidationError::NoMemories;
        assert!(err.to_string().contains("No memories"));
    }
}
