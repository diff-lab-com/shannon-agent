//! Streaming output differential rendering tracker.
//!
//! Tracks content changes between rendering frames to minimize redundant
//! re-rendering. Computes deltas (unchanged prefix/suffix, changed middle)
//! so the terminal can perform minimal screen updates during streaming.

use std::hash::{Hash, Hasher};

/// Default minimum number of lines that must change before re-rendering.
const DEFAULT_MIN_DIFF_LINES: usize = 1;

/// Tracks streaming output for differential rendering.
///
/// Maintains a hash and line count of the last rendered content. When new
/// content arrives, `should_rerender` compares the new content against the
/// previous state to decide if the change is significant enough to warrant
/// a full re-render. `compute_delta` produces a structured diff that callers
/// can use for partial screen updates.
#[derive(Debug)]
pub struct StreamingDiffTracker {
    /// Hash of the last rendered content.
    last_hash: u64,
    /// Number of lines in the last rendered content.
    last_lines: usize,
    /// Minimum number of lines that must change before re-rendering.
    min_diff_lines: usize,
    /// The last rendered content (kept for delta computation).
    last_content: String,
}

impl Default for StreamingDiffTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingDiffTracker {
    /// Create a new tracker with default settings.
    pub fn new() -> Self {
        Self {
            last_hash: 0,
            last_lines: 0,
            min_diff_lines: DEFAULT_MIN_DIFF_LINES,
            last_content: String::new(),
        }
    }

    /// Create a new tracker with a custom minimum diff line threshold.
    pub fn with_min_diff_lines(min_diff_lines: usize) -> Self {
        Self {
            last_hash: 0,
            last_lines: 0,
            min_diff_lines: min_diff_lines.max(1),
            last_content: String::new(),
        }
    }

    /// Check if content has changed enough to warrant re-rendering.
    ///
    /// Returns `true` if the content hash differs from the last render and
    /// the number of changed lines meets the `min_diff_lines` threshold.
    /// Returns `true` if there is no previous content (first render).
    pub fn should_rerender(&self, new_content: &str) -> bool {
        // First render: always render.
        if self.last_hash == 0 && self.last_content.is_empty() {
            return true;
        }

        let new_hash = compute_hash(new_content);
        if new_hash == self.last_hash {
            return false;
        }

        // Content changed — check if enough lines differ.
        let old_lines: Vec<&str> = self.last_content.lines().collect();
        let new_lines: Vec<&str> = new_content.lines().collect();

        // If line count changed by more than threshold, re-render.
        let line_count_diff = if old_lines.len() > new_lines.len() {
            old_lines.len() - new_lines.len()
        } else {
            new_lines.len() - old_lines.len()
        };

        if line_count_diff >= self.min_diff_lines {
            return true;
        }

        // Count lines that actually changed in the overlapping region.
        let min_len = old_lines.len().min(new_lines.len());
        let mut changed_lines = line_count_diff;
        for i in 0..min_len {
            if old_lines[i] != new_lines[i] {
                changed_lines += 1;
            }
        }

        changed_lines >= self.min_diff_lines
    }

    /// Update tracking state after rendering.
    ///
    /// Call this after a successful render to record the current content state.
    pub fn update(&mut self, content: &str) {
        self.last_hash = compute_hash(content);
        self.last_lines = content.lines().count();
        self.last_content = content.to_string();
    }

    /// Compute the diff between old and new content for minimal update.
    ///
    /// Returns a `ContentDelta` describing the unchanged prefix lines, the
    /// unchanged suffix lines, and the changed lines in the middle. This
    /// enables callers to perform partial screen updates rather than
    /// redrawing the entire content area.
    pub fn compute_delta(&self, old: &str, new: &str) -> ContentDelta {
        let old_lines: Vec<&str> = old.lines().collect();
        let new_lines: Vec<&str> = new.lines().collect();

        if old_lines.is_empty() {
            return ContentDelta {
                unchanged_prefix_lines: 0,
                unchanged_suffix_lines: 0,
                changed_lines: new_lines.iter().map(|s| s.to_string()).collect(),
            };
        }

        if new_lines.is_empty() {
            return ContentDelta {
                unchanged_prefix_lines: 0,
                unchanged_suffix_lines: 0,
                changed_lines: Vec::new(),
            };
        }

        // Find the length of the unchanged prefix.
        let min_len = old_lines.len().min(new_lines.len());
        let mut prefix = 0;
        while prefix < min_len && old_lines[prefix] == new_lines[prefix] {
            prefix += 1;
        }

        // Find the length of the unchanged suffix (from the end).
        let max_suffix = min_len - prefix;
        let mut suffix = 0;
        while suffix < max_suffix {
            let old_idx = old_lines.len() - 1 - suffix;
            let new_idx = new_lines.len() - 1 - suffix;
            if old_lines[old_idx] == new_lines[new_idx] {
                suffix += 1;
            } else {
                break;
            }
        }

        // Extract the changed lines from new_content.
        let changed_start = prefix;
        let changed_end = new_lines.len().saturating_sub(suffix);
        let changed_lines: Vec<String> = if changed_start < changed_end {
            new_lines[changed_start..changed_end]
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            Vec::new()
        };

        ContentDelta {
            unchanged_prefix_lines: prefix,
            unchanged_suffix_lines: suffix,
            changed_lines,
        }
    }

    /// Get the number of lines in the last rendered content.
    pub fn last_line_count(&self) -> usize {
        self.last_lines
    }

    /// Reset the tracker to its initial state.
    pub fn reset(&mut self) {
        self.last_hash = 0;
        self.last_lines = 0;
        self.last_content.clear();
    }
}

/// Represents the difference between two versions of content.
///
/// The delta divides content into three regions:
/// 1. `unchanged_prefix_lines` — identical lines at the start.
/// 2. `changed_lines` — the new lines that differ (replacing old middle).
/// 3. `unchanged_suffix_lines` — identical lines at the end.
///
/// Callers can use this to re-render only the changed portion, preserving
/// the prefix and suffix on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentDelta {
    /// Number of identical lines at the start of both old and new content.
    pub unchanged_prefix_lines: usize,
    /// Number of identical lines at the end of both old and new content.
    pub unchanged_suffix_lines: usize,
    /// The new lines that differ in the middle region.
    pub changed_lines: Vec<String>,
}

impl ContentDelta {
    /// Returns `true` if there are no changes (delta is empty).
    pub fn is_empty(&self) -> bool {
        self.changed_lines.is_empty()
            && self.unchanged_prefix_lines == 0
            && self.unchanged_suffix_lines == 0
    }

    /// Returns the total number of lines in the new content implied by this delta.
    pub fn total_new_lines(&self) -> usize {
        self.unchanged_prefix_lines + self.changed_lines.len() + self.unchanged_suffix_lines
    }
}

/// Compute a fast, non-cryptographic hash for content comparison.
fn compute_hash(s: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_diff_should_rerender_first_render() {
        let tracker = StreamingDiffTracker::new();
        // First render with any content should return true.
        assert!(tracker.should_rerender("hello"));
    }

    #[test]
    fn test_streaming_diff_should_rerender_no_change() {
        let mut tracker = StreamingDiffTracker::new();
        tracker.update("hello\nworld");
        // Same content should not need re-rendering.
        assert!(!tracker.should_rerender("hello\nworld"));
    }

    #[test]
    fn test_streaming_diff_should_rerender_changed() {
        let mut tracker = StreamingDiffTracker::new();
        tracker.update("hello\nworld");
        // Changed content should need re-rendering.
        assert!(tracker.should_rerender("hello\nchanged"));
    }

    #[test]
    fn test_streaming_diff_should_rerender_added_lines() {
        let mut tracker = StreamingDiffTracker::new();
        tracker.update("line 1");
        assert!(tracker.should_rerender("line 1\nline 2\nline 3"));
    }

    #[test]
    fn test_streaming_diff_should_rerender_removed_lines() {
        let mut tracker = StreamingDiffTracker::new();
        tracker.update("line 1\nline 2\nline 3");
        assert!(tracker.should_rerender("line 1"));
    }

    #[test]
    fn test_streaming_diff_should_rerender_empty_after_content() {
        let mut tracker = StreamingDiffTracker::new();
        tracker.update("content");
        assert!(tracker.should_rerender(""));
    }

    #[test]
    fn test_streaming_diff_with_min_diff_lines_threshold() {
        let mut tracker = StreamingDiffTracker::with_min_diff_lines(3);
        tracker.update("line 1\nline 2\nline 3\nline 4\nline 5");

        // Changing only 1 line should not meet the threshold of 3.
        // (1 changed line < 3 threshold)
        assert!(!tracker.should_rerender("line 1\nline 2\nCHANGED\nline 4\nline 5"));

        // Changing 3 lines should meet the threshold.
        assert!(tracker.should_rerender("CHANGED1\nCHANGED2\nCHANGED3\nline 4\nline 5"));
    }

    #[test]
    fn test_streaming_diff_compute_delta() {
        let tracker = StreamingDiffTracker::new();
        let old = "line 1\nline 2\nline 3\nline 4\nline 5";
        let new = "line 1\nline 2\nCHANGED A\nCHANGED B\nline 5";
        let delta = tracker.compute_delta(old, new);

        assert_eq!(delta.unchanged_prefix_lines, 2); // "line 1", "line 2"
        assert_eq!(delta.unchanged_suffix_lines, 1); // "line 5"
        assert_eq!(delta.changed_lines, vec!["CHANGED A", "CHANGED B"]);
    }

    #[test]
    fn test_streaming_diff_compute_delta_no_change() {
        let tracker = StreamingDiffTracker::new();
        let content = "line 1\nline 2\nline 3";
        let delta = tracker.compute_delta(content, content);

        assert_eq!(delta.unchanged_prefix_lines, 3);
        assert_eq!(delta.unchanged_suffix_lines, 0);
        assert!(delta.changed_lines.is_empty());
    }

    #[test]
    fn test_streaming_diff_compute_delta_empty_old() {
        let tracker = StreamingDiffTracker::new();
        let delta = tracker.compute_delta("", "new line 1\nnew line 2");

        assert_eq!(delta.unchanged_prefix_lines, 0);
        assert_eq!(delta.unchanged_suffix_lines, 0);
        assert_eq!(delta.changed_lines, vec!["new line 1", "new line 2"]);
    }

    #[test]
    fn test_streaming_diff_compute_delta_empty_new() {
        let tracker = StreamingDiffTracker::new();
        let delta = tracker.compute_delta("old content", "");

        assert_eq!(delta.unchanged_prefix_lines, 0);
        assert_eq!(delta.unchanged_suffix_lines, 0);
        assert!(delta.changed_lines.is_empty());
    }

    #[test]
    fn test_streaming_diff_compute_delta_append_only() {
        let tracker = StreamingDiffTracker::new();
        let old = "line 1\nline 2";
        let new = "line 1\nline 2\nline 3\nline 4";
        let delta = tracker.compute_delta(old, new);

        // Prefix of 2 unchanged lines, no suffix overlap, 2 new lines added.
        assert_eq!(delta.unchanged_prefix_lines, 2);
        assert_eq!(delta.unchanged_suffix_lines, 0);
        assert_eq!(delta.changed_lines, vec!["line 3", "line 4"]);
    }

    #[test]
    fn test_streaming_diff_compute_delta_all_changed() {
        let tracker = StreamingDiffTracker::new();
        let old = "alpha\nbeta\ngamma";
        let new = "one\ntwo\nthree";
        let delta = tracker.compute_delta(old, new);

        assert_eq!(delta.unchanged_prefix_lines, 0);
        assert_eq!(delta.unchanged_suffix_lines, 0);
        assert_eq!(delta.changed_lines, vec!["one", "two", "three"]);
    }

    #[test]
    fn test_streaming_diff_compute_delta_suffix_only_change() {
        let tracker = StreamingDiffTracker::new();
        let old = "line 1\nline 2\nline 3";
        let new = "line 1\nline 2\nmodified";
        let delta = tracker.compute_delta(old, new);

        assert_eq!(delta.unchanged_prefix_lines, 2);
        assert_eq!(delta.unchanged_suffix_lines, 0);
        assert_eq!(delta.changed_lines, vec!["modified"]);
    }

    #[test]
    fn test_streaming_diff_update_and_reset() {
        let mut tracker = StreamingDiffTracker::new();
        tracker.update("some content");
        assert_eq!(tracker.last_line_count(), 1);
        assert!(!tracker.should_rerender("some content"));

        tracker.reset();
        assert_eq!(tracker.last_line_count(), 0);
        assert!(tracker.should_rerender("new content"));
    }

    #[test]
    fn test_content_delta_total_new_lines() {
        let delta = ContentDelta {
            unchanged_prefix_lines: 3,
            unchanged_suffix_lines: 2,
            changed_lines: vec!["a".to_string(), "b".to_string()],
        };
        assert_eq!(delta.total_new_lines(), 7);
    }

    #[test]
    fn test_content_delta_is_empty() {
        let empty = ContentDelta {
            unchanged_prefix_lines: 0,
            unchanged_suffix_lines: 0,
            changed_lines: vec![],
        };
        assert!(empty.is_empty());

        let non_empty = ContentDelta {
            unchanged_prefix_lines: 1,
            unchanged_suffix_lines: 0,
            changed_lines: vec![],
        };
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let a = compute_hash("test content");
        let b = compute_hash("test content");
        assert_eq!(a, b);

        let c = compute_hash("different content");
        assert_ne!(a, c);
    }

    #[test]
    fn test_streaming_diff_multiline_streaming() {
        // Simulates a streaming scenario where content grows incrementally.
        let mut tracker = StreamingDiffTracker::new();

        // First chunk arrives.
        let chunk1 = "Hello\n";
        assert!(tracker.should_rerender(chunk1));
        tracker.update(chunk1);

        // Second chunk appended.
        let chunk2 = "Hello\nWorld\n";
        assert!(tracker.should_rerender(chunk2));
        let delta = tracker.compute_delta(chunk1, chunk2);
        assert_eq!(delta.unchanged_prefix_lines, 1); // "Hello" unchanged
        // Trailing newlines produce empty string elements in .lines().
        assert!(delta.changed_lines.contains(&"World".to_string()));
        tracker.update(chunk2);

        // No change.
        assert!(!tracker.should_rerender(chunk2));
    }
}
