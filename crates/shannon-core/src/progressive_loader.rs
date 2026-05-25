//! # Progressive Context Loading
//!
//! Reduces first-token latency for large files by truncating content to fit
//! within configurable limits while preserving the most informative sections
//! (head and tail).
//!
//! ## Usage
//!
//! ```ignore
//! use shannon_core::progressive_loader::{ProgressiveLoaderConfig, truncate_content};
//!
//! let config = ProgressiveLoaderConfig::default();
//! let truncated = truncate_content(&huge_file_content, &config);
//! ```
//!
//! The [`truncate_content`] function keeps `head_lines` lines from the start and
//! `tail_lines` lines from the end of the input, inserting an omission notice in
//! between. Content that falls within `max_read_lines` is returned unchanged.

/// Configuration for progressive context loading.
#[derive(Debug, Clone)]
pub struct ProgressiveLoaderConfig {
    /// Maximum number of lines before truncation is applied.
    ///
    /// Files with fewer lines than this threshold are returned verbatim.
    pub max_read_lines: usize,

    /// Number of lines to preserve at the start of a truncated file.
    pub head_lines: usize,

    /// Number of lines to preserve at the end of a truncated file.
    pub tail_lines: usize,

    /// Whether to auto-summarize when the context budget is tight.
    ///
    /// When `true`, the system may apply summarisation heuristics to further
    /// compress truncated output. Currently a placeholder for future work.
    pub auto_summarize: bool,
}

impl Default for ProgressiveLoaderConfig {
    fn default() -> Self {
        Self {
            max_read_lines: 2000,
            head_lines: 50,
            tail_lines: 50,
            auto_summarize: true,
        }
    }
}

/// Truncate content to fit within budget, preserving head and tail.
///
/// If the content has `max_read_lines` or fewer lines it is returned unchanged.
/// Otherwise the output contains:
///
/// ```text
/// [Lines 1-{head_lines} of {total}]
/// ... first head_lines lines ...
///
/// ... {omitted} lines omitted (use offset/limit to read more) ...
///
/// ... last tail_lines lines ...
///
/// Total: {total} lines. Use offset/limit parameters to read specific sections.
/// ```
pub fn truncate_content(content: &str, config: &ProgressiveLoaderConfig) -> String {
    let all_lines: Vec<&str> = content.lines().collect();
    let total_lines = all_lines.len();

    if total_lines <= config.max_read_lines {
        return content.to_string();
    }

    let head_end = config.head_lines.min(total_lines);
    let tail_start = total_lines.saturating_sub(config.tail_lines);

    // Avoid overlap when head + tail would cover the entire file.
    if tail_start <= head_end {
        return content.to_string();
    }

    let omitted = total_lines - head_end - (total_lines - tail_start);

    let mut out = String::with_capacity(content.len() / 3);

    // Header
    out.push_str(&format!("[Lines 1-{head_end} of {total_lines}]\n"));

    // Head section
    for line in &all_lines[..head_end] {
        out.push_str(line);
        out.push('\n');
    }

    // Omission notice
    out.push('\n');
    out.push_str(&format!(
        "... {omitted} lines omitted (use offset/limit to read more) ...\n"
    ));
    out.push('\n');

    // Tail section
    for line in &all_lines[tail_start..] {
        out.push_str(line);
        out.push('\n');
    }

    // Footer
    out.push('\n');
    out.push_str(&format!(
        "Total: {total_lines} lines. Use offset/limit parameters to read specific sections."
    ));

    out
}

/// Calculate how many lines can fit within a token budget.
///
/// Uses a rough heuristic of 1 token per 4 characters. The function walks
/// the lines from the start and counts how many fit before the budget is
/// exhausted.
pub fn lines_for_token_budget(content: &str, budget_tokens: usize) -> usize {
    let chars_per_token = 4.0;
    let budget_chars = (budget_tokens as f64) * chars_per_token;

    let mut char_count: f64 = 0.0;
    let mut line_count = 0;

    for line in content.lines() {
        // +1 for the newline character that `lines()` strips.
        char_count += line.len() as f64 + 1.0;
        if char_count > budget_chars {
            break;
        }
        line_count += 1;
    }

    line_count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ProgressiveLoaderConfig::default();
        assert_eq!(config.max_read_lines, 2000);
        assert_eq!(config.head_lines, 50);
        assert_eq!(config.tail_lines, 50);
        assert!(config.auto_summarize);
    }

    #[test]
    fn test_truncate_short_content_unchanged() {
        let config = ProgressiveLoaderConfig {
            max_read_lines: 100,
            ..Default::default()
        };
        let content = "line 1\nline 2\nline 3";
        let result = truncate_content(content, &config);
        assert_eq!(result, content);
    }

    #[test]
    fn test_truncate_exactly_at_limit_unchanged() {
        let config = ProgressiveLoaderConfig {
            max_read_lines: 5,
            ..Default::default()
        };
        let content = "a\nb\nc\nd\ne"; // exactly 5 lines
        let result = truncate_content(content, &config);
        assert_eq!(result, content);
    }

    #[test]
    fn test_truncate_long_content() {
        let config = ProgressiveLoaderConfig {
            max_read_lines: 10,
            head_lines: 2,
            tail_lines: 2,
            auto_summarize: false,
        };

        // 20 lines of content
        let lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
        let content = lines.join("\n");

        let result = truncate_content(&content, &config);

        // Should contain head lines
        assert!(result.contains("line 1"), "should contain first head line");
        assert!(result.contains("line 2"), "should contain second head line");

        // Should contain tail lines
        assert!(
            result.contains("line 19"),
            "should contain second-to-last tail line"
        );
        assert!(result.contains("line 20"), "should contain last tail line");

        // Should NOT contain middle lines
        assert!(!result.contains("line 10"), "should omit middle lines");
        assert!(!result.contains("line 11"), "should omit middle lines");

        // Should have header
        assert!(result.contains("[Lines 1-2 of 20]"));

        // Should have footer
        assert!(result.contains("Total: 20 lines"));

        // Should have omission notice
        assert!(result.contains("lines omitted"));
    }

    #[test]
    fn test_truncate_preserves_head_and_tail() {
        let config = ProgressiveLoaderConfig {
            max_read_lines: 5,
            head_lines: 1,
            tail_lines: 1,
            auto_summarize: false,
        };

        let lines: Vec<String> = (0..100).map(|i| format!("L{i}")).collect();
        let content = lines.join("\n");
        let result = truncate_content(&content, &config);

        assert!(result.contains("L0"), "should contain first line");
        assert!(result.contains("L99"), "should contain last line");
        assert!(!result.contains("L50"), "should not contain middle");
        assert!(
            result.contains("98 lines omitted"),
            "should report omitted count"
        );
    }

    #[test]
    fn test_truncate_head_tail_overlap_returns_full() {
        // If head_lines + tail_lines >= total, return unchanged
        let config = ProgressiveLoaderConfig {
            max_read_lines: 3,
            head_lines: 10,
            tail_lines: 10,
            auto_summarize: false,
        };
        let content = "a\nb\nc\nd"; // 4 lines, > max_read_lines=3
        let result = truncate_content(content, &config);
        // head=10 covers all 4 lines, tail would start at max(0, 4-10)=0 which overlaps
        assert_eq!(result, content, "overlap should return content unchanged");
    }

    #[test]
    fn test_lines_for_token_budget() {
        // Each line is roughly 7 chars ("line NN\n"), ~1.75 tokens each.
        // Budget of 10 tokens = ~40 chars = ~5 lines.
        let lines: Vec<String> = (0..100).map(|i| format!("line {i}")).collect();
        let content = lines.join("\n");

        let count = lines_for_token_budget(&content, 10);
        assert!(
            count >= 3 && count <= 7,
            "expected roughly 5 lines, got {count}"
        );
    }

    #[test]
    fn test_lines_for_token_budget_exceeds_content() {
        let content = "short";
        let count = lines_for_token_budget(content, 10000);
        assert_eq!(count, 1, "single line should fit in large budget");
    }

    #[test]
    fn test_lines_for_token_budget_empty() {
        let count = lines_for_token_budget("", 100);
        assert_eq!(count, 0, "empty content has zero lines");
    }

    #[test]
    fn test_lines_for_token_budget_zero_budget() {
        let content = "some text here";
        let count = lines_for_token_budget(content, 0);
        assert_eq!(count, 0, "zero budget fits zero lines");
    }

    #[test]
    fn test_truncate_empty_content() {
        let config = ProgressiveLoaderConfig::default();
        let result = truncate_content("", &config);
        assert_eq!(result, "");
    }
}
