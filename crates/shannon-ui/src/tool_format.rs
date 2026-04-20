//! Tool output formatting for the REPL
//!
//! Provides smart content-type detection and formatted display for tool results,
//! including diffs, JSON, file trees, tables, code blocks, and errors.

use syntect::easy::HighlightLines;
use syntect::parsing::SyntaxSet;
use syntect::highlighting::{ThemeSet, Theme};
use syntect::highlighting::FontStyle;
use syntect::util::LinesWithEndings;

/// ANSI color codes for terminal output
#[allow(dead_code)]
mod colors {
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const CYAN: &str = "\x1b[36m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const BLUE: &str = "\x1b[34m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const RESET: &str = "\x1b[0m";
}

/// Lazy-initialized syntax highlighting state
struct SyntaxHighlight {
    syntax_set: SyntaxSet,
    theme: Theme,
}

impl SyntaxHighlight {
    fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        // Use a dark theme that works well in terminal
        let theme = theme_set.themes["base16-eighties.dark"].clone();
        Self { syntax_set, theme }
    }

    fn highlight(&self, code: &str, lang: &str) -> String {
        let syntax = self.syntax_set
            .find_syntax_by_token(lang)
            .or_else(|| self.syntax_set.find_syntax_by_extension(lang))
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        let mut output = String::new();

        for line in LinesWithEndings::from(code) {
            let ranges = highlighter.highlight_line(line, &self.syntax_set).unwrap_or_default();
            for (style, text) in ranges {
                let ansi = style_to_ansi(&style);
                output.push_str(&ansi);
                output.push_str(text.trim_end_matches('\n'));
                output.push_str(colors::RESET);
            }
            output.push('\n');
        }
        output
    }
}

/// Convert a syntect style to ANSI escape sequence.
fn style_to_ansi(style: &syntect::highlighting::Style) -> String {
    let fg = style.foreground;
    let mut parts = Vec::new();
    // Map RGB to nearest ANSI 256-color
    let ansi_color = rgb_to_ansi256(fg.r, fg.g, fg.b);
    parts.push(format!("\x1b[38;5;{ansi_color}m"));
    if style.font_style.contains(FontStyle::BOLD) {
        parts.push(colors::BOLD.to_string());
    }
    parts.join("")
}

/// Approximate RGB to 256-color terminal index.
fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
    // Check if it's a greyscale
    if r == g && g == b {
        if r < 8 { return 16; }
        if r > 248 { return 231; }
        return ((r as u16 - 8) * 24 / 247) as u8 + 232;
    }
    // Map to 6x6x6 color cube
    let ri = (r as u16 * 5 / 255) as u8;
    let gi = (g as u16 * 5 / 255) as u8;
    let bi = (b as u16 * 5 / 255) as u8;
    16 + 36 * ri + 6 * gi + bi
}

use std::sync::LazyLock;
static SYNTAX_HIGHLIGHT: LazyLock<SyntaxHighlight> = LazyLock::new(SyntaxHighlight::new);

/// Maximum number of lines to show for pretty-printed JSON output
const JSON_MAX_LINES: usize = 50;

/// Maximum number of characters to show for a single tool result before truncation
const MAX_RESULT_CHARS: usize = 2000;

/// Format a tool result for display in the REPL.
///
/// Detects the content type of the result string and formats it accordingly:
/// - Errors get red coloring with a cross prefix
/// - Diffs get green/red/cyan coloring for added/removed/hunk lines
/// - JSON gets pretty-printed with indentation (capped at 50 lines)
/// - File lists get tree-drawing characters
/// - Tables get aligned columns
/// - Everything else passes through as-is
pub fn format_tool_result(tool_name: &str, result: &str, is_error: bool) -> String {
    let truncated = truncate_result(result);

    if is_error {
        format_error(tool_name, &truncated)
    } else if looks_like_diff(&truncated) {
        format_diff(&truncated)
    } else if looks_like_json(&truncated) {
        format_json(&truncated)
    } else if looks_like_file_list(&truncated) {
        format_file_tree(&truncated)
    } else if looks_like_table(&truncated) {
        format_table(&truncated)
    } else if looks_like_code(&truncated) {
        highlight_code_by_tool(&truncated, tool_name)
    } else {
        truncated
    }
}

/// Truncate result to a reasonable display length, appending an indicator if truncated.
fn truncate_result(result: &str) -> String {
    if result.len() <= MAX_RESULT_CHARS {
        result.to_string()
    } else {
        let truncated: String = result.chars().take(MAX_RESULT_CHARS).collect();
        format!(
            "{}{}  [... {} more characters]{}",
            truncated,
            colors::DIM,
            result.len() - MAX_RESULT_CHARS,
            colors::RESET,
        )
    }
}

// ── Detection functions ─────────────────────────────────────────────────

/// Check if the content looks like source code (multi-line with code-like patterns).
fn looks_like_code(s: &str) -> bool {
    let lines: Vec<&str> = s.lines().take(10).collect();
    if lines.len() < 2 {
        return false;
    }
    // Heuristic: look for common code patterns
    let code_indicators = [
        ("fn ", "rust"),
        ("let ", "rust"),
        ("pub ", "rust"),
        ("use ", "rust"),
        ("impl ", "rust"),
        ("function ", "javascript"),
        ("const ", "javascript"),
        ("import ", "javascript"),
        ("class ", "typescript"),
        ("def ", "python"),
        ("from ", "python"),
        ("if __name__", "python"),
        ("package ", "java"),
        ("#include", "cpp"),
        ("func ", "go"),
        ("    ", "indented"),
    ];
    let mut score = 0;
    for line in &lines {
        for (pattern, _) in &code_indicators {
            if line.contains(pattern) {
                score += 1;
                break;
            }
        }
    }
    score * 2 >= lines.len()
}

/// Map tool name to a language identifier for syntax highlighting.
fn highlight_code_by_tool(s: &str, tool_name: &str) -> String {
    let lang = match tool_name {
        "bash" | "sh" | "shell" | "run" | "execute" => "bash",
        "read" | "cat" | "file" => "text",
        "python" | "py" => "python",
        "node" | "javascript" | "js" => "javascript",
        "grep" | "search" => "text",
        _ => "text",
    };
    SYNTAX_HIGHLIGHT.highlight(s, lang)
}

/// Check if the content looks like a unified diff.
fn looks_like_diff(s: &str) -> bool {
    let trimmed = s.trim_start();
    // Unified diff headers start with +++ or ---, or hunk markers @@
    // Also match if multiple lines start with + or - in a diff-like pattern
    if trimmed.starts_with("+++") || trimmed.starts_with("---") || trimmed.starts_with("@@") {
        return true;
    }
    // Check for diff --- a/... +++ b/... pattern on first two lines
    let mut lines = trimmed.lines().take(4);
    let first = lines.next().unwrap_or("");
    let second = lines.next().unwrap_or("");
    if first.starts_with("---") && second.starts_with("+++") {
        return true;
    }
    // Check if enough lines look like diff content (+/- at start)
    let total = trimmed.lines().count().min(20);
    if total < 3 {
        return false;
    }
    let diff_lines = trimmed
        .lines()
        .take(20)
        .filter(|l| l.starts_with('+') || l.starts_with('-') || l.starts_with('@'))
        .count();
    diff_lines * 2 >= total
}

/// Check if the content looks like JSON (starts with `{` or `[`).
fn looks_like_json(s: &str) -> bool {
    let trimmed = s.trim_start();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

/// Check if the content looks like a file listing (one path per line).
fn looks_like_file_list(s: &str) -> bool {
    let lines: Vec<&str> = s.lines().take(20).collect();
    if lines.len() < 2 {
        return false;
    }
    // Most lines should look like file paths (contain / or .ext, no spaces)
    let path_like = lines
        .iter()
        .filter(|l| {
            let l = l.trim();
            !l.is_empty()
                && !l.contains(' ')
                && (l.contains('/') || l.contains('.') || l.contains('\\'))
        })
        .count();
    path_like * 2 >= lines.len() && lines.len() >= 2
}

/// Check if the content looks like a markdown table (contains `|` and `---`).
fn looks_like_table(s: &str) -> bool {
    let lines: Vec<&str> = s.lines().take(5).collect();
    let has_pipe = lines.iter().any(|l| l.contains('|'));
    let has_separator = lines.iter().any(|l| {
        let trimmed = l.trim();
        trimmed.contains("---") && trimmed.contains('|')
    });
    has_pipe && has_separator
}

// ── Formatting functions ────────────────────────────────────────────────

/// Format a diff with color coding.
///
/// - Green for added lines (`+`)
/// - Red for removed lines (`-`)
/// - Cyan for hunk headers (`@@`)
/// - Bold for file headers (`+++`, `---`)
fn format_diff(s: &str) -> String {
    s.lines()
        .map(|line| {
            if line.starts_with("+++") || line.starts_with("---") && !line.starts_with("--- /dev/null") {
                format!("{}{}{}", colors::BOLD, line, colors::RESET)
            } else if line.starts_with('+') {
                format!("{}{}{}", colors::GREEN, line, colors::RESET)
            } else if line.starts_with('-') {
                format!("{}{}{}", colors::RED, line, colors::RESET)
            } else if line.starts_with("@@") {
                format!("{}{}{}", colors::CYAN, line, colors::RESET)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Pretty-print JSON with indentation and syntax highlighting, capped at 50 lines.
fn format_json(s: &str) -> String {
    let pretty = match serde_json::from_str::<serde_json::Value>(s) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| s.to_string()),
        Err(_) => s.to_string(),
    };

    let lines: Vec<&str> = pretty.lines().collect();
    let capped = if lines.len() <= JSON_MAX_LINES {
        pretty
    } else {
        let mut output = lines[..JSON_MAX_LINES].join("\n");
        output.push_str(&format!(
            "\n{}  ... {} more lines{}",
            colors::DIM,
            lines.len() - JSON_MAX_LINES,
            colors::RESET,
        ));
        output
    };

    // Apply JSON syntax highlighting
    SYNTAX_HIGHLIGHT.highlight(&capped, "json")
}

/// Format a file list as a tree with drawing characters.
///
/// Uses unicode box-drawing: `|--` for intermediate items, ``--` for the last.
fn format_file_tree(s: &str) -> String {
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return s.to_string();
    }

    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let is_last = i == lines.len() - 1;
            let branch = if is_last { "\u{2514}\u{2500}\u{2500} " } else { "\u{251C}\u{2500}\u{2500} " };
            format!("{}{}", branch, line.trim())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format a markdown-style table with aligned columns.
///
/// Detects column boundaries from `|` characters and aligns them.
fn format_table(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    if lines.is_empty() {
        return s.to_string();
    }

    // Parse all rows into cells
    let rows: Vec<Vec<&str>> = lines
        .iter()
        .map(|line| {
            line.split('|')
                .map(|cell| cell.trim())
                .filter(|cell| !cell.is_empty())
                .collect()
        })
        .filter(|row: &Vec<&str>| !row.is_empty())
        .collect();

    if rows.is_empty() {
        return s.to_string();
    }

    // Determine max columns
    let max_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if max_cols == 0 {
        return s.to_string();
    }

    // Calculate column widths
    let mut col_widths = vec![0usize; max_cols];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            if i < max_cols {
                col_widths[i] = col_widths[i].max(cell.len());
            }
        }
    }

    // Cap column widths to 40 characters
    for w in &mut col_widths {
        *w = (*w).min(40);
    }

    // Format rows
    let mut output = String::new();
    for (row_idx, row) in rows.iter().enumerate() {
        let mut line = String::from("| ");
        for (i, cell) in row.iter().enumerate() {
            if i < max_cols {
                let width = col_widths[i];
                // Truncate cell content if wider than column width
                let display_cell: String = cell.chars().take(width).collect();
                line.push_str(&format!("{display_cell:width$}"));
                line.push_str(" | ");
            }
        }
        // Trim trailing " | " to just "|"
        if line.ends_with(" | ") {
            line.truncate(line.len() - 1);
            line.push('|');
        }
        output.push_str(&line);
        output.push('\n');

        // Add separator after header row if the next line is a --- separator
        if row_idx == 0 {
            let sep: String = col_widths
                .iter()
                .map(|&w| format!("{}{}", "-".repeat(w), "-".repeat(0)))
                .collect::<Vec<_>>()
                .join("-+-");
            output.push_str(&format!("|-{sep}-|\n"));
        }
    }

    output.trim_end().to_string()
}

/// Format an error result with red coloring and a cross prefix.
fn format_error(tool_name: &str, s: &str) -> String {
    format!(
        "{}\u{2717} {} failed:{} {}",
        colors::RED,
        tool_name,
        colors::RESET,
        s.trim(),
    )
}

// ── Tests ───────────────────────────────────────────────────────────────

/// Strip ANSI escape sequences from a string.
pub fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['
            // Skip all digits and semicolons until 'm'
            while let Some(&next) = chars.peek() {
                chars.next();
                if next == 'm' { break; }
            }
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Detection tests ──────────────────────────────────────────────

    #[test]
    fn test_looks_like_diff_unified() {
        let diff = "--- a/foo.rs\n+++ b/foo.rs\n@@ -1,3 +1,4 @@\n-old line\n+new line\n context\n";
        assert!(looks_like_diff(diff));
    }

    #[test]
    fn test_looks_like_diff_hunk_only() {
        let diff = "@@ -10,5 +10,6 @@\n context\n+added\n-removed\n";
        assert!(looks_like_diff(diff));
    }

    #[test]
    fn test_looks_like_diff_header() {
        assert!(looks_like_diff("+++ b/new_file.rs\n+content\n"));
        assert!(looks_like_diff("--- a/old.rs\n+++ b/old.rs\n+line\n-line\n"));
    }

    #[test]
    fn test_not_diff() {
        assert!(!looks_like_diff("Hello world"));
        assert!(!looks_like_diff("Just some normal text\nwith lines\n"));
    }

    #[test]
    fn test_looks_like_json_object() {
        assert!(looks_like_json("{\"key\": \"value\"}"));
    }

    #[test]
    fn test_looks_like_json_array() {
        assert!(looks_like_json("[1, 2, 3]"));
    }

    #[test]
    fn test_not_json() {
        assert!(!looks_like_json("Hello world"));
        assert!(!looks_like_json("Not JSON at all"));
    }

    #[test]
    fn test_looks_like_file_list_unix() {
        let list = "src/main.rs\nsrc/lib.rs\nCargo.toml\nREADME.md\n";
        assert!(looks_like_file_list(list));
    }

    #[test]
    fn test_looks_like_file_list_with_dirs() {
        let list = "src/\ntests/\nCargo.toml\n";
        assert!(looks_like_file_list(list));
    }

    #[test]
    fn test_not_file_list() {
        assert!(!looks_like_file_list("Hello world"));
        assert!(!looks_like_file_list("single-file"));
    }

    #[test]
    fn test_looks_like_table() {
        let table = "| Name | Value |\n| --- | --- |\n| foo | bar |\n";
        assert!(looks_like_table(table));
    }

    #[test]
    fn test_not_table() {
        assert!(!looks_like_table("No pipes here"));
        assert!(!looks_like_table("Some | text but no separator"));
    }

    // ── Formatting tests ─────────────────────────────────────────────

    #[test]
    fn test_format_diff_colors() {
        let diff = "--- a/old.rs\n+++ b/new.rs\n@@ -1,3 +1,3 @@\n-old\n+new\n context\n";
        let formatted = format_diff(diff);
        // Should contain ANSI codes
        assert!(formatted.contains(colors::RED));
        assert!(formatted.contains(colors::GREEN));
        assert!(formatted.contains(colors::CYAN));
        assert!(formatted.contains(colors::BOLD));
        // Should still contain the actual content
        assert!(formatted.contains("-old"));
        assert!(formatted.contains("+new"));
        assert!(formatted.contains(" context"));
    }

    #[test]
    fn test_format_json_pretty_print() {
        let json = r#"{"name":"test","value":42}"#;
        let formatted = format_json(json);
        let plain = strip_ansi(&formatted);
        // Should be pretty-printed (contains newlines and indentation)
        assert!(plain.contains('\n'));
        assert!(plain.contains("  "));
        assert!(plain.contains("\"name\""));
        assert!(plain.contains("\"test\""));
    }

    #[test]
    fn test_format_json_invalid_falls_back() {
        let invalid = "{not valid json}";
        let formatted = format_json(invalid);
        let plain = strip_ansi(&formatted);
        // Should fall back to original text (minus ANSI codes)
        assert!(plain.contains(invalid));
    }

    #[test]
    fn test_format_json_truncation() {
        // Create a large JSON that exceeds 50 lines when pretty-printed
        let mut obj = serde_json::Map::new();
        for i in 0..100 {
            obj.insert(
                format!("key_{i}"),
                serde_json::Value::String(format!("value_{i}")),
            );
        }
        let large_json = serde_json::to_string(&serde_json::Value::Object(obj)).unwrap();
        let formatted = format_json(&large_json);
        let plain = strip_ansi(&formatted);
        assert!(plain.contains("... "));
        assert!(plain.contains("more lines"));
    }

    #[test]
    fn test_format_file_tree() {
        let list = "src/main.rs\nsrc/lib.rs\nCargo.toml\n";
        let formatted = format_file_tree(list);
        // Should contain tree-drawing characters
        assert!(formatted.contains("\u{251C}\u{2500}\u{2500} ")); // ├──
        assert!(formatted.contains("\u{2514}\u{2500}\u{2500} ")); // └──
        assert!(formatted.contains("src/main.rs"));
        assert!(formatted.contains("Cargo.toml"));
    }

    #[test]
    fn test_format_file_tree_single() {
        let list = "only_file.rs\n";
        let formatted = format_file_tree(list);
        // Single file should use └── (last item marker)
        assert!(formatted.contains("\u{2514}\u{2500}\u{2500} "));
        assert!(!formatted.contains("\u{251C}\u{2500}\u{2500} "));
    }

    #[test]
    fn test_format_file_tree_empty() {
        let formatted = format_file_tree("");
        assert!(formatted.is_empty());
    }

    #[test]
    fn test_format_table_basic() {
        let table = "| Name | Age |\n| --- | --- |\n| Alice | 30 |\n| Bob | 25 |\n";
        let formatted = format_table(table);
        assert!(formatted.contains("Alice"));
        assert!(formatted.contains("Bob"));
        // Should have separator line
        assert!(formatted.contains('-'));
    }

    #[test]
    fn test_format_error_basic() {
        let formatted = format_error("bash", "command not found");
        assert!(formatted.contains("\u{2717}")); // ✗
        assert!(formatted.contains("bash failed"));
        assert!(formatted.contains("command not found"));
        assert!(formatted.contains(colors::RED));
    }

    // ── Main entry point tests ────────────────────────────────────────

    #[test]
    fn test_format_tool_result_error() {
        let result = format_tool_result("bash", "command failed", true);
        assert!(result.contains("\u{2717}"));
        assert!(result.contains("bash failed"));
    }

    #[test]
    fn test_format_tool_result_diff() {
        let diff = "--- a/old.rs\n+++ b/new.rs\n@@ -1 +1 @@\n-old\n+new\n";
        let result = format_tool_result("edit", diff, false);
        assert!(result.contains(colors::GREEN));
        assert!(result.contains(colors::RED));
    }

    #[test]
    fn test_format_tool_result_json() {
        let json = r#"{"status":"ok","count":5}"#;
        let result = format_tool_result("query", json, false);
        assert!(result.contains('\n')); // pretty-printed
    }

    #[test]
    fn test_format_tool_result_file_list() {
        let list = "src/main.rs\nsrc/lib.rs\nCargo.toml\n";
        let result = format_tool_result("glob", list, false);
        assert!(result.contains("\u{251C}\u{2500}\u{2500} ")); // ├──
    }

    #[test]
    fn test_format_tool_result_table() {
        let table = "| Name | Value |\n| --- | --- |\n| foo | bar |\n";
        let result = format_tool_result("query", table, false);
        assert!(result.contains("foo"));
        assert!(result.contains("bar"));
    }

    #[test]
    fn test_format_tool_result_plain_text() {
        let text = "Just some plain text output from a tool.";
        let result = format_tool_result("echo", text, false);
        assert_eq!(result, text);
    }

    #[test]
    fn test_format_tool_result_truncation() {
        let long_text = "x".repeat(5000);
        let result = format_tool_result("tool", &long_text, false);
        assert!(result.contains("[... "));
        assert!(result.contains("more characters]"));
    }

    #[test]
    fn test_format_tool_result_empty_string() {
        let result = format_tool_result("tool", "", false);
        assert!(result.is_empty());
    }

    #[test]
    fn test_format_tool_result_whitespace_only() {
        let result = format_tool_result("tool", "   \n  \n  ", false);
        assert_eq!(result, "   \n  \n  ");
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn test_looks_like_diff_mostly_plus_minus() {
        // A file that has many +/- lines but no @@ header should still match
        let content = "+added line 1\n+added line 2\n-removed line\n+added line 3\n+added line 4\n+added line 5\n";
        assert!(looks_like_diff(content));
    }

    #[test]
    fn test_format_table_empty_input() {
        let result = format_table("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_format_json_array() {
        let json = "[1,2,3]";
        let formatted = format_json(json);
        assert!(formatted.contains('\n'));
        assert!(formatted.contains('1'));
    }

    #[test]
    fn test_format_file_tree_with_blank_lines() {
        let list = "src/main.rs\n\nsrc/lib.rs\n\nCargo.toml\n";
        let formatted = format_file_tree(list);
        // Should skip blank lines and format the three files
        assert!(formatted.contains("src/main.rs"));
        assert!(formatted.contains("src/lib.rs"));
        assert!(formatted.contains("Cargo.toml"));
        // Should have three entries with tree characters
        assert_eq!(formatted.lines().count(), 3);
    }
}
