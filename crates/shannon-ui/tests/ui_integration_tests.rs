//! Integration tests for shannon-ui covering:
//! - Markdown table detection, formatting, and rendering
//! - Streaming diff tracking and delta computation
//! - Stream buffer mode transitions
//! - Tool format utility functions

use shannon_ui::markdown_table::{detect_table, format_table, render_table};
use shannon_ui::stream_buffer::{StreamBuffer, StreamMode};
use shannon_ui::streaming_diff::{ContentDelta, StreamingDiffTracker};
use shannon_ui::tool_format::{
    DiffStats, format_diff_summary, looks_like_json, parse_diff_stats, strip_ansi,
};

// =========================================================================
// 1. Markdown Table Rendering
// =========================================================================

mod markdown_table_integration {
    use super::*;

    #[test]
    fn test_detect_table_in_mixed_content() {
        let content = "\
Some introductory text here.

| Name  | Age | City |
| ----  | --- | ---- |
| Alice | 30  | NYC  |
| Bob   | 25  | LA   |

More text after the table.";

        let range = detect_table(content).unwrap();
        // Table starts at line 2 (after blank line), ends at line 5
        assert_eq!(range.start_line, 2);
        assert_eq!(range.end_line, 5);
    }

    #[test]
    fn test_detect_table_at_document_boundaries() {
        // Table at the very start
        let start = "| H1 | H2 |\n| -- | -- |\n| a  | b  |";
        let range = detect_table(start).unwrap();
        assert_eq!(range.start_line, 0);
        assert_eq!(range.end_line, 2);

        // Table at the very end with prefix text
        let end = "Preamble\n| X | Y |\n| - | - |\n| 1 | 2 |";
        let range = detect_table(end).unwrap();
        assert_eq!(range.start_line, 1);
        assert_eq!(range.end_line, 3);
    }

    #[test]
    fn test_detect_table_rejects_invalid_structures() {
        // Only header, no separator
        assert!(detect_table("| H1 | H2 |\n| a | b |").is_none());

        // Single line
        assert!(detect_table("| A |").is_none());

        // Empty content
        assert!(detect_table("").is_none());

        // No pipes at all
        assert!(detect_table("Just plain text\nNo table here").is_none());
    }

    #[test]
    fn test_detect_table_with_alignment_markers() {
        let content = "| Left | Center | Right |\n| :--- | :---: | ---: |\n| a | b | c |";
        let range = detect_table(content).unwrap();
        assert_eq!(range.start_line, 0);
        assert_eq!(range.end_line, 2);
    }

    #[test]
    fn test_format_table_basic_reformatting() {
        let raw = "\
| Name  | Age |
| ----- | --- |
| Alice | 30  |
| Bob   | 25  |";

        let result = format_table(raw, 80);

        // Should contain box-drawing borders
        assert!(result.contains('\u{250C}'), "Should have top-left corner");
        assert!(result.contains('\u{2510}'), "Should have top-right corner");
        assert!(
            result.contains('\u{2514}'),
            "Should have bottom-left corner"
        );
        assert!(
            result.contains('\u{2518}'),
            "Should have bottom-right corner"
        );

        // Should contain the data
        assert!(result.contains("Alice"));
        assert!(result.contains("Bob"));
        assert!(result.contains("30"));
        assert!(result.contains("25"));
    }

    #[test]
    fn test_format_table_respects_max_width() {
        let raw = "| Short | VeryLongColumnNameThatMightOverflowTheTerminalWidth |\n| --- | --- |\n| a | b |";
        let result_narrow = format_table(raw, 40);
        let result_wide = format_table(raw, 120);

        assert!(!result_narrow.is_empty());
        assert!(!result_wide.is_empty());
        // Both should produce valid output without panicking
    }

    #[test]
    fn test_format_table_numeric_right_alignment() {
        let raw = "| Item | Price | Count |\n| ---- | ----- | ----- |\n| Apple | 1.50 | 100 |\n| Banana | 0.75 | 200 |";
        let result = format_table(raw, 80);

        // Data values should be present
        assert!(result.contains("Apple"));
        assert!(result.contains("1.50"));
        assert!(result.contains("200"));
    }

    #[test]
    fn test_format_table_too_few_lines_returns_raw() {
        assert_eq!(format_table("| only header |", 80), "| only header |");
        assert_eq!(format_table("", 80), "");
    }

    #[test]
    fn test_render_table_with_custom_widths() {
        let header = &["Name", "Age", "City"];
        let rows = vec![
            vec!["Alice".to_string(), "30".to_string(), "NYC".to_string()],
            vec!["Bob".to_string(), "25".to_string(), "LA".to_string()],
        ];

        let result = render_table(header, &rows, Some(vec![10, 5, 8]));

        assert!(result.contains("Alice"));
        assert!(result.contains("Bob"));
        // Box-drawing characters present
        assert!(result.contains('\u{2502}')); // vertical bar
    }

    #[test]
    fn test_render_table_empty_header_returns_empty() {
        let header: &[&str] = &[];
        let rows: Vec<Vec<String>> = vec![];
        assert!(render_table(header, &rows, None).is_empty());
    }

    #[test]
    fn test_render_table_single_column() {
        let header = &["Only"];
        let rows = vec![vec!["value".to_string()]];
        let result = render_table(header, &rows, None);
        assert!(result.contains("Only"));
        assert!(result.contains("value"));
    }

    #[test]
    fn test_render_table_truncates_long_values() {
        let long_value = "x".repeat(200);
        let header = &["Data"];
        let rows = vec![vec![long_value.clone()]];
        let result = render_table(header, &rows, None);

        // Should be truncated with ellipsis, not contain the full 200-char string
        assert!(
            result.contains('\u{2026}'),
            "Long values should be truncated"
        );
        assert!(!result.contains(&long_value));
    }

    #[test]
    fn test_render_table_uneven_row_lengths() {
        let header = &["A", "B", "C"];
        let rows = vec![
            vec!["a".to_string()],
            vec!["b".to_string(), "c".to_string()],
            vec!["d".to_string(), "e".to_string(), "f".to_string()],
        ];
        let result = render_table(header, &rows, None);
        assert!(result.contains('a'));
        assert!(result.contains('f'));
    }
}

// =========================================================================
// 2. Streaming Diff
// =========================================================================

mod streaming_diff_integration {
    use super::*;

    #[test]
    fn test_multi_chunk_streaming_simulation() {
        let mut tracker = StreamingDiffTracker::new();

        // Chunk 1: Initial content
        let chunk1 = "Hello, world!";
        assert!(tracker.should_rerender(chunk1));
        tracker.update(chunk1);
        assert_eq!(tracker.last_line_count(), 1);

        // Chunk 2: Content grows
        let chunk2 = "Hello, world!\nHow are you?";
        assert!(tracker.should_rerender(chunk2));
        let delta = tracker.compute_delta(chunk1, chunk2);
        assert_eq!(delta.unchanged_prefix_lines, 1);
        assert_eq!(delta.changed_lines, vec!["How are you?"]);
        tracker.update(chunk2);

        // Chunk 3: More growth
        let chunk3 = "Hello, world!\nHow are you?\nI am fine.";
        assert!(tracker.should_rerender(chunk3));
        tracker.update(chunk3);

        // No change
        assert!(!tracker.should_rerender(chunk3));
    }

    #[test]
    fn test_compute_delta_large_content_change() {
        let tracker = StreamingDiffTracker::new();

        let old_lines: Vec<String> = (0..100).map(|i| format!("old line {i}")).collect();
        let new_lines: Vec<String> = (0..100).map(|i| format!("new line {i}")).collect();

        let old = old_lines.join("\n");
        let new = new_lines.join("\n");

        let delta = tracker.compute_delta(&old, &new);

        // No shared prefix or suffix (all lines changed)
        assert_eq!(delta.unchanged_prefix_lines, 0);
        assert_eq!(delta.unchanged_suffix_lines, 0);
        assert_eq!(delta.changed_lines.len(), 100);
    }

    #[test]
    fn test_compute_delta_prefix_and_suffix_unchanged() {
        let tracker = StreamingDiffTracker::new();

        let old = "header\nline A\nline B\nline C\nfooter";
        let new = "header\nCHANGED X\nCHANGED Y\nfooter";

        let delta = tracker.compute_delta(old, new);

        assert_eq!(delta.unchanged_prefix_lines, 1); // "header"
        assert_eq!(delta.unchanged_suffix_lines, 1); // "footer"
        assert_eq!(delta.changed_lines, vec!["CHANGED X", "CHANGED Y"]);
        assert_eq!(delta.total_new_lines(), 4);
    }

    #[test]
    fn test_threshold_prevents_minor_updates() {
        let mut tracker = StreamingDiffTracker::with_min_diff_lines(3);

        tracker.update("line 1\nline 2\nline 3\nline 4\nline 5");

        // Only 1 line changed: below threshold
        assert!(!tracker.should_rerender("line 1\nline 2\nCHANGED\nline 4\nline 5"));

        // 3 lines changed: meets threshold
        assert!(tracker.should_rerender("C1\nC2\nC3\nline 4\nline 5"));
    }

    #[test]
    fn test_reset_allows_fresh_start() {
        let mut tracker = StreamingDiffTracker::new();
        tracker.update("some content");
        assert_eq!(tracker.last_line_count(), 1);

        tracker.reset();
        assert_eq!(tracker.last_line_count(), 0);

        // After reset, should act like first render
        assert!(tracker.should_rerender("new content"));
    }

    #[test]
    fn test_content_delta_is_empty_check() {
        let empty = ContentDelta {
            unchanged_prefix_lines: 0,
            unchanged_suffix_lines: 0,
            changed_lines: vec![],
        };
        assert!(empty.is_empty());

        let with_prefix = ContentDelta {
            unchanged_prefix_lines: 2,
            unchanged_suffix_lines: 0,
            changed_lines: vec![],
        };
        assert!(!with_prefix.is_empty());

        let with_changes = ContentDelta {
            unchanged_prefix_lines: 0,
            unchanged_suffix_lines: 0,
            changed_lines: vec!["changed".to_string()],
        };
        assert!(!with_changes.is_empty());
    }

    #[test]
    fn test_content_delta_total_new_lines() {
        let delta = ContentDelta {
            unchanged_prefix_lines: 5,
            unchanged_suffix_lines: 3,
            changed_lines: vec!["a".to_string(), "b".to_string()],
        };
        assert_eq!(delta.total_new_lines(), 10);
    }

    #[test]
    fn test_identical_content_no_rerender() {
        let mut tracker = StreamingDiffTracker::new();
        let content = "line 1\nline 2\nline 3";
        tracker.update(content);
        assert!(!tracker.should_rerender(content));
    }

    #[test]
    fn test_empty_to_content_triggers_render() {
        let tracker = StreamingDiffTracker::new();
        assert!(tracker.should_rerender(""));
        assert!(tracker.should_rerender("anything"));
    }

    #[test]
    fn test_compute_delta_all_same_is_no_change() {
        let tracker = StreamingDiffTracker::new();
        let content = "a\nb\nc";
        let delta = tracker.compute_delta(content, content);
        assert!(delta.changed_lines.is_empty());
        assert_eq!(delta.unchanged_prefix_lines, 3);
    }
}

// =========================================================================
// 3. Stream Buffer
// =========================================================================

mod stream_buffer_integration {
    use super::*;

    #[test]
    fn test_smooth_mode_drains_individually() {
        let mut buf = StreamBuffer::new();

        buf.push_chunk("alpha");
        buf.push_chunk("beta");
        buf.push_chunk("gamma");

        assert!(buf.needs_render());
        assert_eq!(buf.drain_for_render().unwrap(), "alpha");
        assert_eq!(buf.drain_for_render().unwrap(), "beta");
        assert_eq!(buf.drain_for_render().unwrap(), "gamma");
        assert!(buf.drain_for_render().is_none());
        assert!(!buf.needs_render());
    }

    #[test]
    fn test_accumulated_text_preserves_all() {
        let mut buf = StreamBuffer::new();
        buf.push_chunk("Hello ");
        buf.push_chunk("World");
        buf.push_chunk("!");

        assert_eq!(buf.accumulated_text(), "Hello World!");

        // Drain doesn't lose accumulated text
        let _ = buf.drain_for_render();
        let _ = buf.drain_for_render();
        let _ = buf.drain_for_render();
        assert_eq!(buf.accumulated_text(), "Hello World!");
    }

    #[test]
    fn test_reset_clears_all_state() {
        let mut buf = StreamBuffer::new();
        buf.push_chunk("data");
        let _ = buf.drain_for_render();
        buf.push_chunk("more");
        buf.reset();

        assert!(buf.accumulated_text().is_empty());
        assert!(!buf.needs_render());
        assert_eq!(buf.current_mode(), StreamMode::Smooth);
        assert!(buf.drain_for_render().is_none());
    }

    #[test]
    fn test_empty_push_is_ignored() {
        let mut buf = StreamBuffer::new();
        buf.push_chunk("");
        assert!(!buf.needs_render());
        assert!(buf.drain_for_render().is_none());
    }

    #[test]
    fn test_default_is_smooth() {
        let buf = StreamBuffer::default();
        assert_eq!(buf.current_mode(), StreamMode::Smooth);
        assert!(buf.accumulated_text().is_empty());
    }
}

// =========================================================================
// 4. Tool Format Utilities
// =========================================================================

mod tool_format_integration {
    use super::*;

    #[test]
    fn test_looks_like_json_objects_and_arrays() {
        assert!(looks_like_json("{\"key\": \"value\"}"));
        assert!(looks_like_json("[1, 2, 3]"));
        assert!(looks_like_json("  {\"indented\": true}  "));

        assert!(!looks_like_json("plain text"));
        assert!(!looks_like_json(""));
        assert!(!looks_like_json("not json at all"));
    }

    #[test]
    fn test_parse_diff_stats_valid_input() {
        let diff_output = "\
--- a/old_file.rs
+++ b/new_file.rs
 unchanged context
+added line 1
+added line 2
-removed line 1
--- a/another_file.rs
+++ b/another_file.rs
+added line 3
-removed line 2
-removed line 3";
        let stats = parse_diff_stats(diff_output);
        assert_eq!(stats.files_changed, 2);
        assert_eq!(stats.additions, 3);
        assert_eq!(stats.deletions, 3);
    }

    #[test]
    fn test_parse_diff_stats_additions_only() {
        let diff_output = "\
+added line 1
+added line 2
+added line 3
+added line 4
+added line 5";
        let stats = parse_diff_stats(diff_output);
        assert_eq!(stats.files_changed, 1);
        assert_eq!(stats.additions, 5);
        assert_eq!(stats.deletions, 0);
    }

    #[test]
    fn test_parse_diff_stats_no_changes() {
        let diff_output = "";
        let stats = parse_diff_stats(diff_output);
        assert_eq!(stats.files_changed, 0);
        assert_eq!(stats.additions, 0);
        assert_eq!(stats.deletions, 0);
    }

    #[test]
    fn test_format_diff_summary_output() {
        let stats = DiffStats {
            files_changed: 2,
            additions: 10,
            deletions: 3,
        };
        let summary = format_diff_summary(&stats);
        assert!(summary.contains("2"));
        assert!(summary.contains("10"));
        assert!(summary.contains("3"));
    }

    #[test]
    fn test_strip_ansi_removes_escape_codes() {
        let colored = "\x1b[31mRed text\x1b[0m and \x1b[32mgreen\x1b[0m";
        let stripped = strip_ansi(colored);
        assert_eq!(stripped, "Red text and green");
        assert!(!stripped.contains('\x1b'));
    }

    #[test]
    fn test_strip_ansi_preserves_plain_text() {
        let plain = "Hello, World!";
        assert_eq!(strip_ansi(plain), "Hello, World!");
    }
}
