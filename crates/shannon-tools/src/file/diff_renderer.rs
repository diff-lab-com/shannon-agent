//! # File Edit UI Diff Renderer
//!
//! Terminal-colored diff rendering for file edits, inspired by Claude Code's
//! FileEditTool UI. Provides both unified and side-by-side diff views with
//! color-coded output suitable for terminal display.
//!
//! ## Architecture
//!
//! - [`DiffLine`]: A single line in a diff (context, addition, or deletion)
//! - [`DiffHunk`]: A contiguous group of changed lines with a header
//! - [`DiffStats`]: Aggregate statistics for additions, deletions, changes
//! - [`DiffRenderer`]: The main renderer that produces terminal-colored output
//!
//! ## Example
//!
//! ```
//! use shannon_tools::file::diff_renderer::{DiffRenderer, DiffHunk, DiffLine, DiffLineType, DiffStats};
//!
//! let hunks = vec![
//!     DiffHunk {
//!         old_start: 1, old_count: 2, new_start: 1, new_count: 2,
//!         header: "@@ -1,2 +1,2 @@".to_string(),
//!         lines: vec![
//!             DiffLine { line_type: DiffLineType::Context, content: "fn main() {".to_string(), line_number_old: Some(1), line_number_new: Some(1) },
//!             DiffLine { line_type: DiffLineType::Delete, content: "    old();".to_string(), line_number_old: Some(2), line_number_new: None },
//!             DiffLine { line_type: DiffLineType::Add, content: "    new();".to_string(), line_number_new: Some(2), line_number_old: None },
//!         ],
//!     },
//! ];
//!
//! let renderer = DiffRenderer::new();
//! let output = renderer.render_diff(&hunks, "/path/to/file.rs");
//! assert!(output.contains("fn main"));
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// DiffLineType
// ---------------------------------------------------------------------------

/// The type of a single diff line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffLineType {
    /// Unchanged context line.
    Context,
    /// Added line (green).
    Add,
    /// Deleted line (red).
    Delete,
    /// Hunk header.
    Header,
}

impl fmt::Display for DiffLineType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Context => write!(f, " "),
            Self::Add => write!(f, "+"),
            Self::Delete => write!(f, "-"),
            Self::Header => write!(f, "@"),
        }
    }
}

// ---------------------------------------------------------------------------
// DiffLine
// ---------------------------------------------------------------------------

/// A single line in a diff.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffLine {
    /// The type of change.
    pub line_type: DiffLineType,
    /// The text content (without the leading +/- prefix).
    pub content: String,
    /// Line number in the old file (if applicable).
    pub line_number_old: Option<usize>,
    /// Line number in the new file (if applicable).
    pub line_number_new: Option<usize>,
}

// ---------------------------------------------------------------------------
// DiffHunk
// ---------------------------------------------------------------------------

/// A contiguous group of changed lines.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffHunk {
    /// Starting line number in the old file.
    pub old_start: usize,
    /// Number of lines from the old file in this hunk.
    pub old_count: usize,
    /// Starting line number in the new file.
    pub new_start: usize,
    /// Number of lines from the new file in this hunk.
    pub new_count: usize,
    /// Hunk header string (e.g. "@@ -1,3 +1,4 @@ ...").
    pub header: String,
    /// Individual lines in this hunk.
    pub lines: Vec<DiffLine>,
}

impl DiffHunk {
    /// Create a default hunk header string from the fields.
    pub fn make_header(&self) -> String {
        let old_range = if self.old_count == 1 {
            format!("{}", self.old_start)
        } else {
            format!("{},{}", self.old_start, self.old_count)
        };
        let new_range = if self.new_count == 1 {
            format!("{}", self.new_start)
        } else {
            format!("{},{}", self.new_start, self.new_count)
        };
        format!("@@ -{old_range} +{new_range} @@")
    }

    /// Recalculate stats for this hunk.
    pub fn stats(&self) -> DiffStats {
        let mut additions = 0usize;
        let mut deletions = 0usize;
        for line in &self.lines {
            match line.line_type {
                DiffLineType::Add => additions += 1,
                DiffLineType::Delete => deletions += 1,
                _ => {}
            }
        }
        DiffStats {
            additions,
            deletions,
            changes: additions + deletions,
        }
    }
}

// ---------------------------------------------------------------------------
// DiffStats
// ---------------------------------------------------------------------------

/// Aggregate statistics for a diff.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct DiffStats {
    /// Number of lines added.
    pub additions: usize,
    /// Number of lines deleted.
    pub deletions: usize,
    /// Total number of changed lines.
    pub changes: usize,
}

impl fmt::Display for DiffStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "+{} -{} ({} changed)",
            self.additions, self.deletions, self.changes
        )
    }
}

impl DiffStats {
    /// Create stats from a list of hunks.
    pub fn from_hunks(hunks: &[DiffHunk]) -> Self {
        let mut total = DiffStats::default();
        for hunk in hunks {
            let s = hunk.stats();
            total.additions += s.additions;
            total.deletions += s.deletions;
            total.changes += s.changes;
        }
        total
    }

    /// Merge another stats into this one.
    pub fn merge(&mut self, other: &DiffStats) {
        self.additions += other.additions;
        self.deletions += other.deletions;
        self.changes += other.changes;
    }
}

// ---------------------------------------------------------------------------
// Color helpers
// ---------------------------------------------------------------------------

/// ANSI color codes for terminal output.
#[derive(Debug, Clone, Copy)]
pub struct ColorScheme {
    pub add: &'static str,
    pub delete: &'static str,
    pub context: &'static str,
    pub header: &'static str,
    pub reset: &'static str,
    pub bold: &'static str,
    pub dim: &'static str,
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self {
            add: "\x1b[32m",     // green
            delete: "\x1b[31m",  // red
            context: "\x1b[36m", // cyan
            header: "\x1b[1m",   // bold
            reset: "\x1b[0m",
            bold: "\x1b[1m",
            dim: "\x1b[2m",
        }
    }
}

// ---------------------------------------------------------------------------
// DiffRenderer
// ---------------------------------------------------------------------------

/// Renders diffs with terminal colors.
#[derive(Debug, Clone)]
pub struct DiffRenderer {
    colors: ColorScheme,
    /// Terminal width for side-by-side mode.
    pub terminal_width: usize,
    /// Show line numbers.
    pub show_line_numbers: bool,
}

impl Default for DiffRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl DiffRenderer {
    /// Create a new renderer with default settings.
    pub fn new() -> Self {
        Self {
            colors: ColorScheme::default(),
            terminal_width: 80,
            show_line_numbers: true,
        }
    }

    /// Create with a custom color scheme.
    pub fn with_colors(colors: ColorScheme) -> Self {
        Self {
            colors,
            terminal_width: 80,
            show_line_numbers: true,
        }
    }

    /// Set terminal width (used by side-by-side mode).
    pub fn with_terminal_width(mut self, width: usize) -> Self {
        self.terminal_width = width;
        self
    }

    /// Enable or disable line numbers.
    pub fn with_line_numbers(mut self, show: bool) -> Self {
        self.show_line_numbers = show;
        self
    }

    /// Render a complete diff with file header and stats.
    pub fn render_diff(&self, hunks: &[DiffHunk], file_path: &str) -> String {
        let mut output = String::new();

        // File header
        output.push_str(&format!(
            "{}--- diff for: {}{}",
            self.colors.bold, file_path, self.colors.reset
        ));
        output.push('\n');

        // Stats
        let stats = DiffStats::from_hunks(hunks);
        output.push_str(&format!(
            "{}{}{}",
            self.colors.dim, stats, self.colors.reset
        ));
        output.push('\n');
        output.push('\n');

        // Render each hunk
        for hunk in hunks {
            output.push_str(&self.render_hunk(hunk));
            output.push('\n');
        }

        output
    }

    /// Render a single hunk in unified format.
    pub fn render_hunk(&self, hunk: &DiffHunk) -> String {
        let mut output = String::new();

        // Header line
        output.push_str(&format!(
            "{}{}{}",
            self.colors.header, hunk.header, self.colors.reset
        ));
        output.push('\n');

        // Lines
        for line in &hunk.lines {
            output.push_str(&self.render_line(line));
            output.push('\n');
        }

        output
    }

    /// Render a complete unified diff across multiple hunks.
    pub fn render_unified_diff(&self, hunks: &[DiffHunk], file_path: &str) -> String {
        let mut output = String::new();

        // Unified diff header
        output.push_str(&format!(
            "{}diff --git a/{} b/{}{}",
            self.colors.bold, file_path, file_path, self.colors.reset
        ));
        output.push('\n');

        for hunk in hunks {
            output.push_str(&self.render_hunk(hunk));
        }

        // Stats summary at bottom
        let stats = DiffStats::from_hunks(hunks);
        output.push_str(&format!(
            "\n{}Summary: {}{}",
            self.colors.dim, stats, self.colors.reset
        ));
        output.push('\n');

        output
    }

    /// Render a side-by-side diff view.
    pub fn render_side_by_side(&self, hunks: &[DiffHunk], file_path: &str) -> String {
        let mut output = String::new();

        // Header
        let header = format!("Side-by-side diff: {file_path}");
        output.push_str(&format!(
            "{}{}{}",
            self.colors.bold, header, self.colors.reset
        ));
        output.push('\n');
        output.push('\n');

        let half_width = (self.terminal_width.saturating_sub(3)) / 2;

        for hunk in hunks {
            output.push_str(&self.render_side_by_side_hunk(hunk, half_width));
        }

        // Stats
        let stats = DiffStats::from_hunks(hunks);
        output.push_str(&format!(
            "\n{}{}{}",
            self.colors.dim, stats, self.colors.reset
        ));
        output.push('\n');

        output
    }

    /// Render a single hunk in side-by-side format.
    fn render_side_by_side_hunk(&self, hunk: &DiffHunk, half_width: usize) -> String {
        let mut output = String::new();

        output.push_str(&format!(
            "{}{}{}",
            self.colors.header, hunk.header, self.colors.reset
        ));
        output.push('\n');

        // Separator line
        let sep = format!("{:<width$} | {:<width$}", "", "", width = half_width);
        output.push_str(&format!("{}{}{}", self.colors.dim, sep, self.colors.reset));
        output.push('\n');

        for line in &hunk.lines {
            let (left, right) = match line.line_type {
                DiffLineType::Context => {
                    let text = self.pad_content(&line.content, half_width);
                    (
                        format!("{}{}{}", self.colors.context, text, self.colors.reset),
                        format!("{}{}{}", self.colors.context, text, self.colors.reset),
                    )
                }
                DiffLineType::Delete => {
                    let text = self.pad_content(&line.content, half_width);
                    (
                        format!("{}{}{}", self.colors.delete, text, self.colors.reset),
                        " ".repeat(half_width),
                    )
                }
                DiffLineType::Add => {
                    let text = self.pad_content(&line.content, half_width);
                    (
                        " ".repeat(half_width),
                        format!("{}{}{}", self.colors.add, text, self.colors.reset),
                    )
                }
                DiffLineType::Header => continue,
            };

            output.push_str(&format!("{left} | {right}\n"));
        }

        output
    }

    /// Render a single line with appropriate coloring.
    fn render_line(&self, line: &DiffLine) -> String {
        let prefix = match line.line_type {
            DiffLineType::Context => format!("{} ", self.colors.context),
            DiffLineType::Add => format!("{}+", self.colors.add),
            DiffLineType::Delete => format!("{}-", self.colors.delete),
            DiffLineType::Header => format!("{} ", self.colors.header),
        };

        let mut result = String::new();

        if self.show_line_numbers {
            let old_num = line
                .line_number_old
                .map(|n| format!("{n:>4}"))
                .unwrap_or_else(|| "    ".to_string());
            let new_num = line
                .line_number_new
                .map(|n| format!("{n:>4}"))
                .unwrap_or_else(|| "    ".to_string());
            result.push_str(&format!(
                "{}{} | {}{}{}",
                self.colors.dim, old_num, new_num, self.colors.reset, prefix,
            ));
        } else {
            result.push_str(&prefix);
        }

        result.push_str(&line.content);
        result.push_str(self.colors.reset);

        result
    }

    /// Pad/truncate content to fit within a given width.
    fn pad_content(&self, content: &str, width: usize) -> String {
        if content.len() >= width {
            format!("{}...", &content[..width.saturating_sub(3)])
        } else {
            format!("{content:width$}")
        }
    }

    /// Render stats as a summary line.
    pub fn render_stats(&self, stats: &DiffStats) -> String {
        format!("{}{}{}", self.colors.bold, stats, self.colors.reset)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_hunk() -> DiffHunk {
        DiffHunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            header: "@@ -1,3 +1,3 @@ fn main()".to_string(),
            lines: vec![
                DiffLine {
                    line_type: DiffLineType::Context,
                    content: "fn main() {".to_string(),
                    line_number_old: Some(1),
                    line_number_new: Some(1),
                },
                DiffLine {
                    line_type: DiffLineType::Delete,
                    content: "    println!(\"old\");".to_string(),
                    line_number_old: Some(2),
                    line_number_new: None,
                },
                DiffLine {
                    line_type: DiffLineType::Add,
                    content: "    println!(\"new\");".to_string(),
                    line_number_old: None,
                    line_number_new: Some(2),
                },
                DiffLine {
                    line_type: DiffLineType::Context,
                    content: "}".to_string(),
                    line_number_old: Some(3),
                    line_number_new: Some(3),
                },
            ],
        }
    }

    fn multi_hunk_hunks() -> Vec<DiffHunk> {
        vec![
            DiffHunk {
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 1,
                header: "@@ -1,1 +1,1 @@".to_string(),
                lines: vec![DiffLine {
                    line_type: DiffLineType::Add,
                    content: "use std::io;".to_string(),
                    line_number_old: None,
                    line_number_new: Some(1),
                }],
            },
            DiffHunk {
                old_start: 5,
                old_count: 2,
                new_start: 6,
                new_count: 2,
                header: "@@ -5,2 +6,2 @@".to_string(),
                lines: vec![
                    DiffLine {
                        line_type: DiffLineType::Delete,
                        content: "old line".to_string(),
                        line_number_old: Some(5),
                        line_number_new: None,
                    },
                    DiffLine {
                        line_type: DiffLineType::Add,
                        content: "new line".to_string(),
                        line_number_old: None,
                        line_number_new: Some(6),
                    },
                ],
            },
        ]
    }

    // -- DiffLineType Display ----------------------------------------------

    #[test]
    fn line_type_display() {
        assert_eq!(DiffLineType::Context.to_string(), " ");
        assert_eq!(DiffLineType::Add.to_string(), "+");
        assert_eq!(DiffLineType::Delete.to_string(), "-");
        assert_eq!(DiffLineType::Header.to_string(), "@");
    }

    // -- DiffStats ---------------------------------------------------------

    #[test]
    fn stats_default_zero() {
        let s = DiffStats::default();
        assert_eq!(s.additions, 0);
        assert_eq!(s.deletions, 0);
        assert_eq!(s.changes, 0);
    }

    #[test]
    fn stats_display() {
        let s = DiffStats {
            additions: 3,
            deletions: 1,
            changes: 4,
        };
        let text = s.to_string();
        assert!(text.contains("+3"));
        assert!(text.contains("-1"));
        assert!(text.contains("4 changed"));
    }

    #[test]
    fn stats_from_hunks() {
        let hunks = multi_hunk_hunks();
        let stats = DiffStats::from_hunks(&hunks);
        assert_eq!(stats.additions, 2);
        assert_eq!(stats.deletions, 1);
        assert_eq!(stats.changes, 3);
    }

    #[test]
    fn stats_merge() {
        let mut a = DiffStats {
            additions: 1,
            deletions: 2,
            changes: 3,
        };
        let b = DiffStats {
            additions: 4,
            deletions: 5,
            changes: 9,
        };
        a.merge(&b);
        assert_eq!(a.additions, 5);
        assert_eq!(a.deletions, 7);
        assert_eq!(a.changes, 12);
    }

    // -- DiffHunk ----------------------------------------------------------

    #[test]
    fn hunk_make_header() {
        let hunk = DiffHunk {
            old_start: 10,
            old_count: 3,
            new_start: 10,
            new_count: 4,
            header: String::new(),
            lines: vec![],
        };
        assert_eq!(hunk.make_header(), "@@ -10,3 +10,4 @@");
    }

    #[test]
    fn hunk_make_header_single_line() {
        let hunk = DiffHunk {
            old_start: 5,
            old_count: 1,
            new_start: 5,
            new_count: 1,
            header: String::new(),
            lines: vec![],
        };
        assert_eq!(hunk.make_header(), "@@ -5 +5 @@");
    }

    #[test]
    fn hunk_stats() {
        let hunk = sample_hunk();
        let stats = hunk.stats();
        assert_eq!(stats.additions, 1);
        assert_eq!(stats.deletions, 1);
        assert_eq!(stats.changes, 2);
    }

    // -- DiffRenderer ------------------------------------------------------

    #[test]
    fn render_diff_contains_content() {
        let renderer = DiffRenderer::new();
        let hunks = vec![sample_hunk()];
        let output = renderer.render_diff(&hunks, "src/main.rs");
        assert!(output.contains("fn main"));
        assert!(output.contains("src/main.rs"));
    }

    #[test]
    fn render_diff_contains_stats() {
        let renderer = DiffRenderer::new();
        let hunks = vec![sample_hunk()];
        let output = renderer.render_diff(&hunks, "test.rs");
        assert!(output.contains("changed"));
    }

    #[test]
    fn render_hunk_contains_header() {
        let renderer = DiffRenderer::new();
        let output = renderer.render_hunk(&sample_hunk());
        assert!(output.contains("@@"));
    }

    #[test]
    fn render_unified_diff_format() {
        let renderer = DiffRenderer::new();
        let hunks = multi_hunk_hunks();
        let output = renderer.render_unified_diff(&hunks, "foo.rs");
        assert!(output.contains("diff --git"));
        assert!(output.contains("Summary:"));
    }

    #[test]
    fn render_side_by_side_contains_separator() {
        let renderer = DiffRenderer::new().with_terminal_width(80);
        let hunks = vec![sample_hunk()];
        let output = renderer.render_side_by_side(&hunks, "bar.rs");
        assert!(output.contains(" | "));
    }

    #[test]
    fn render_side_by_side_shows_file() {
        let renderer = DiffRenderer::new().with_terminal_width(80);
        let hunks = vec![sample_hunk()];
        let output = renderer.render_side_by_side(&hunks, "test.rs");
        assert!(output.contains("test.rs"));
    }

    #[test]
    fn renderer_with_line_numbers_disabled() {
        let renderer = DiffRenderer::new().with_line_numbers(false);
        let output = renderer.render_hunk(&sample_hunk());
        // Should not have the "    1 |" format
        assert!(!output.contains("|"));
    }

    #[test]
    fn renderer_custom_terminal_width() {
        let renderer = DiffRenderer::new().with_terminal_width(120);
        assert_eq!(renderer.terminal_width, 120);
    }

    // -- Serialization round-trip ------------------------------------------

    #[test]
    fn diff_line_serialization() {
        let line = DiffLine {
            line_type: DiffLineType::Add,
            content: "new line".to_string(),
            line_number_old: None,
            line_number_new: Some(5),
        };
        let json = serde_json::to_string(&line).unwrap();
        let back: DiffLine = serde_json::from_str(&json).unwrap();
        assert_eq!(line, back);
    }

    #[test]
    fn diff_hunk_serialization() {
        let hunk = sample_hunk();
        let json = serde_json::to_string(&hunk).unwrap();
        let back: DiffHunk = serde_json::from_str(&json).unwrap();
        assert_eq!(hunk, back);
    }

    #[test]
    fn diff_stats_serialization() {
        let stats = DiffStats {
            additions: 10,
            deletions: 3,
            changes: 13,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let back: DiffStats = serde_json::from_str(&json).unwrap();
        assert_eq!(stats, back);
    }

    #[test]
    fn render_stats_string() {
        let renderer = DiffRenderer::new();
        let stats = DiffStats {
            additions: 5,
            deletions: 2,
            changes: 7,
        };
        let output = renderer.render_stats(&stats);
        assert!(output.contains("changed"));
    }
}
