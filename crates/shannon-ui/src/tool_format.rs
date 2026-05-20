//! Tool output formatting for the REPL
//!
//! Provides smart content-type detection and formatted display for tool results,
//! including diffs, JSON, file trees, tables, code blocks, and errors.

use syntect::easy::HighlightLines;
use syntect::parsing::SyntaxSet;
use syntect::highlighting::ThemeSet;
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

/// Diff-specific ANSI escape codes for background fills and word highlights.
#[allow(dead_code)]
mod diff_colors {
    // Background fills for dark terminals (subtle tints)
    pub const BG_ADDED: &str = "\x1b[48;5;22m";       // Dark green background
    pub const BG_REMOVED: &str = "\x1b[48;5;52m";     // Dark red background
    pub const BG_CONTEXT: &str = "\x1b[48;5;236m";    // Very dark gray

    // Background fills for light terminals
    pub const BG_ADDED_LIGHT: &str = "\x1b[48;5;194m";    // Light green background
    pub const BG_REMOVED_LIGHT: &str = "\x1b[48;5;224m";  // Light red background
    pub const BG_CONTEXT_LIGHT: &str = "\x1b[48;5;254m";  // Very light gray

    // Word-level highlights (brighter foreground on the background)
    pub const FG_ADDED_WORD: &str = "\x1b[38;5;82m";      // Bright green
    pub const FG_REMOVED_WORD: &str = "\x1b[38;5;203m";   // Bright red

    // Foreground for diff gutter / line numbers
    pub const FG_GUTTER: &str = "\x1b[38;5;243m";         // Dim gray
    pub const FG_GUTTER_SEP: &str = "\x1b[38;5;240m";     // Slightly brighter for separator

    // Error background
    pub const BG_ERROR: &str = "\x1b[48;5;52m";
    pub const BG_ERROR_LIGHT: &str = "\x1b[48;5;224m";

    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const CYAN: &str = "\x1b[36m";
}

/// Check if the terminal has a light background by inspecting the `COLORFGBG` env var.
///
/// Many terminal emulators set `COLORFGBG` to something like `15;0` (light fg, dark bg)
/// or `0;15` (dark fg, light bg). We parse the background component and check if it
/// indicates a light terminal.
fn is_light_terminal() -> bool {
    if let Ok(val) = std::env::var("COLORFGBG") {
        // Format is typically "fg;bg" — split on semicolons, last value is bg
        let parts: Vec<&str> = val.split(';').collect();
        if let Some(bg_str) = parts.last() {
            if let Ok(bg) = bg_str.trim().parse::<u8>() {
                // ANSI colors 7, 10-15, 230-255 are typically light
                return (7..=15).contains(&bg) || bg >= 230;
            }
        }
    }
    // Default to dark terminal
    false
}

/// Lazy-initialized syntax highlighting state
struct SyntaxHighlight {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
    theme_name: String,
}

impl SyntaxHighlight {
    fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        let theme_name = "base16-eighties.dark".to_string();
        Self { syntax_set, theme_set, theme_name }
    }

    fn highlight(&self, code: &str, lang: &str) -> String {
        let syntax = self.syntax_set
            .find_syntax_by_token(lang)
            .or_else(|| self.syntax_set.find_syntax_by_extension(lang))
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let theme = self.theme_set.themes.get(&self.theme_name)
            .unwrap_or_else(|| &self.theme_set.themes["base16-eighties.dark"]);
        let mut highlighter = HighlightLines::new(syntax, theme);
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
/// Line count threshold above which results are line-truncated
const MAX_RESULT_LINES: usize = 100;
/// Number of lines to show when line-truncating
const VISIBLE_RESULT_LINES: usize = 20;

/// Clean up a tool name for display. MCP tools have the format
/// `mcp__plugin_{server}__{method}` — this extracts `{server}: {method}`.
/// Skill tools like `skill_commit` show as `commit`.
/// Non-MCP names are returned as-is.
pub fn display_tool_name(tool_name: &str) -> String {
    if let Some(rest) = tool_name.strip_prefix("mcp__plugin_") {
        if let Some((server, method)) = rest.rsplit_once("__") {
            // Deduplicate: "serena_serena" → "serena", "playwright_playwright" → "playwright"
            let clean_server = if let Some((a, b)) = server.split_once('_') {
                if a.eq_ignore_ascii_case(b) { a } else { server }
            } else {
                server
            };
            return format!("{clean_server}: {method}");
        }
    }
    if let Some(skill_name) = tool_name.strip_prefix("skill_") {
        return skill_name.to_string();
    }
    tool_name.to_string()
}

/// Classify a tool by its operation category for display purposes.
pub fn tool_category(tool_name: &str) -> ToolCategory {
    match tool_name {
        // Bash/shell tools
        "bash" | "sh" | "shell" | "run" | "execute" => ToolCategory::Bash,
        // Search tools
        "grep" | "search" | "ripgrep" | "rg" | "ast-grep" | "ast_grep"
        | "glob" | "find" | "list" | "ls" => ToolCategory::Search,
        // Read-only tools
        "read" | "cat" | "head" | "tail" | "view" => ToolCategory::Read,
        // Write tools
        "write" | "edit" | "create" | "delete" | "mkdir" | "mv" | "cp" | "patch" => ToolCategory::Write,
        // Agent tools
        "agent" | "subagent" | "delegate" | "task" => ToolCategory::Agent,
        // Skill tools (registered as skill_{id})
        _ if tool_name.starts_with("skill_") => ToolCategory::Skill,
        _ => {
            // Heuristic: MCP tools typically contain double underscores or dots
            if tool_name.contains("__") || tool_name.contains('.') {
                ToolCategory::Agent
            } else {
                ToolCategory::Read
            }
        }
    }
}

/// Tool operation category for display purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    Read,
    Write,
    Search,
    Bash,
    Agent,
    Skill,
}

/// Maximum lines to show in a read-mode summary.
const READ_SUMMARY_MAX_LINES: usize = 3;

/// Format a read tool result as a compact summary.
pub fn format_read_summary(tool_name: &str, result: &str) -> String {
    let lines: Vec<&str> = result.lines().filter(|l| !l.trim().is_empty()).collect();
    let count = lines.len();
    let summary = match tool_name {
        "read" | "cat" | "head" | "tail" => format!("Read {count} lines"),
        "grep" | "search" => {
            if count == 0 { "No matches found".to_string() }
            else { format!("Found {count} results") }
        }
        "glob" | "find" | "list" | "ls" => {
            if count == 0 { "No files found".to_string() }
            else { format!("Found {count} files") }
        }
        _ => format!("{count} results"),
    };
    let mut output = format!("{} {} {}", colors::DIM, summary, colors::RESET);
    if count > 0 {
        output.push('\n');
        let preview_lines: Vec<&str> = lines.iter().take(READ_SUMMARY_MAX_LINES).copied().collect();
        for line in &preview_lines {
            output.push_str(&format!("  {}{}{}\n", colors::DIM, &line[..line.len().min(120)], colors::RESET));
        }
        if count > READ_SUMMARY_MAX_LINES {
            output.push_str(&format!("  {}... {} more lines{}\n", colors::DIM, count - READ_SUMMARY_MAX_LINES, colors::RESET));
        }
    }
    output.trim_end().to_string()
}

/// Format a write tool result as a command+output block with category icon.
pub fn format_write_result(tool_name: &str, result: &str, is_error: bool, duration_label: &str) -> String {
    if is_error {
        return format_error(tool_name, result, duration_label);
    }
    let (icon, color) = match tool_category(tool_name) {
        ToolCategory::Read => ("\u{25B8}", colors::CYAN),       // ▸ read
        ToolCategory::Write => ("\u{270E}", colors::YELLOW),    // ✎ write
        ToolCategory::Search => ("\u{229B}", colors::MAGENTA),  // ⊛ search
        ToolCategory::Bash => ("$", colors::GREEN),              // $ bash
        ToolCategory::Agent => ("\u{25C6}", colors::CYAN),      // ◆ agent
        ToolCategory::Skill => ("\u{2726}", colors::MAGENTA),   // ✦ skill
    };
    let mut output = String::new();
    output.push_str(&format!("{color}{icon} {tool_name}{duration_label}{}\n", colors::RESET));
    let truncated = truncate_result(result);
    if !truncated.is_empty() {
        output.push_str(&truncated);
    }
    output
}

/// Format a tool result for display in the REPL.
///
/// Detects the content type of the result string and formats it accordingly:
/// - Errors get red coloring with a cross prefix
/// - Diffs get green/red/cyan coloring for added/removed/hunk lines
/// - JSON gets pretty-printed with indentation (capped at 50 lines)
/// - File lists get tree-drawing characters
/// - Tables get aligned columns
/// - Everything else passes through as-is
pub fn format_tool_result(tool_name: &str, result: &str, is_error: bool, duration_secs: Option<f64>) -> String {
    let duration_label = duration_secs.map(|d| {
        if d >= 60.0 {
            format!(" ({}m{:.0}s)", d as u64 / 60, d % 60.0)
        } else {
            format!(" ({:.1}s)", d)
        }
    }).unwrap_or_default();
    if is_error {
        return format_error(tool_name, &truncate_result(result), &duration_label);
    }
    let truncated = truncate_result(result);
    // Content-aware formatting for all tool types
    if looks_like_diff(&truncated) {
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
        match tool_category(tool_name) {
            ToolCategory::Read | ToolCategory::Search => {
                let mut s = format_read_summary(tool_name, result);
                if !duration_label.is_empty() {
                    s = format!("{s}{duration_label}");
                }
                s
            }
            ToolCategory::Write | ToolCategory::Bash | ToolCategory::Agent | ToolCategory::Skill => {
                format_write_result(tool_name, result, false, &duration_label)
            }
        }
    }
}

/// Format a tool result with full content (no dual-mode summarization).
pub fn format_tool_result_full(tool_name: &str, result: &str, is_error: bool) -> String {
    let truncated = truncate_result(result);

    if is_error {
        format_error(tool_name, &truncated, "")
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
    // Line-based truncation: for very long results, show only first N lines
    let lines: Vec<&str> = result.lines().collect();
    let line_truncated = if lines.len() > MAX_RESULT_LINES {
        let visible: String = lines[..VISIBLE_RESULT_LINES].join("\n");
        format!(
            "{}\n{}  [... {} more lines ({} total)]{}",
            visible,
            colors::DIM,
            lines.len() - VISIBLE_RESULT_LINES,
            lines.len(),
            colors::RESET,
        )
    } else {
        result.to_string()
    };

    // Character-based truncation on top of line truncation
    if line_truncated.len() <= MAX_RESULT_CHARS {
        line_truncated
    } else {
        let truncated: String = line_truncated.chars().take(MAX_RESULT_CHARS).collect();
        format!(
            "{}{}  [... {} more characters]{}",
            truncated,
            colors::DIM,
            line_truncated.len() - MAX_RESULT_CHARS,
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

// ── Diff formatting infrastructure ─────────────────────────────────────

/// Configuration for diff rendering behavior.
pub struct DiffConfig {
    /// Max context lines to show around each hunk boundary when collapsed.
    pub max_context_lines: usize,
    /// Max lines before collapsing a hunk (0 = no collapse).
    pub max_expanded_lines: usize,
    /// Show line numbers in the gutter.
    pub show_line_numbers: bool,
    /// Enable word-level diff highlighting.
    pub word_highlight: bool,
    /// Enable background color fills.
    pub bg_fills: bool,
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            max_context_lines: 3,
            max_expanded_lines: 100,
            show_line_numbers: true,
            word_highlight: true,
            bg_fills: true,
        }
    }
}

/// Diff statistics parsed from unified diff content.
pub struct DiffStats {
    /// Number of files changed.
    pub files_changed: usize,
    /// Total number of added lines.
    pub additions: usize,
    /// Total number of removed lines.
    pub deletions: usize,
}

/// Parse statistics from a unified diff string.
pub fn parse_diff_stats(diff: &str) -> DiffStats {
    let mut files_changed = 0usize;
    let mut additions = 0usize;
    let mut deletions = 0usize;

    for line in diff.lines() {
        if line.starts_with("--- ") && !line.starts_with("--- /dev/null") {
            files_changed += 1;
        } else if line.starts_with('+') && !line.starts_with("+++") {
            additions += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            deletions += 1;
        }
    }
    // If we never saw a --- header but saw +/- lines, count as 1 file
    if files_changed == 0 && (additions > 0 || deletions > 0) {
        files_changed = 1;
    }

    DiffStats { files_changed, additions, deletions }
}

/// Format a diff statistics summary line.
pub fn format_diff_summary(stats: &DiffStats) -> String {
    format!(
        "{}  {} file{} changed, {} addition{}, {} deletion{}{}",
        colors::DIM,
        stats.files_changed,
        if stats.files_changed == 1 { "" } else { "s" },
        stats.additions,
        if stats.additions == 1 { "" } else { "s" },
        stats.deletions,
        if stats.deletions == 1 { "" } else { "s" },
        colors::RESET,
    )
}

/// A parsed hunk header with line number information.
#[allow(dead_code)]
struct HunkHeader {
    old_start: usize,
    old_count: usize,
    new_start: usize,
    new_count: usize,
}

/// Parse a `@@ -old_start,old_count +new_start,new_count @@` header.
fn parse_hunk_header(line: &str) -> Option<HunkHeader> {
    // Extract content between @@ markers
    let start = line.find("@@ ")?;
    let rest = &line[start + 3..];
    let end = rest.find(" @@")?;
    let header_content = &rest[..end];

    // Parse: -old_start,old_count +new_start,new_count
    let parts: Vec<&str> = header_content.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let old_part = parts[0].strip_prefix('-')?;
    let new_part = parts[1].strip_prefix('+')?;

    let (old_start, old_count) = parse_range(old_part)?;
    let (new_start, new_count) = parse_range(new_part)?;

    Some(HunkHeader { old_start, old_count, new_start, new_count })
}

/// Parse a range like "1,3" into (start, count). Returns (start, 1) if no comma.
fn parse_range(s: &str) -> Option<(usize, usize)> {
    if let Some(comma) = s.find(',') {
        let start: usize = s[..comma].parse().ok()?;
        let count: usize = s[comma + 1..].parse().ok()?;
        Some((start, count))
    } else {
        let start: usize = s.parse().ok()?;
        Some((start, 1))
    }
}

/// Compute word-level diff between removed and added lines.
///
/// Uses a simple LCS-based approach to identify which words changed.
/// Returns (removed_formatted, added_formatted) with word-level highlights.
fn word_diff(removed: &str, added: &str) -> (String, String) {
    let removed_words = split_words(removed);
    let added_words = split_words(added);

    // Compute LCS table
    let rl = removed_words.len();
    let al = added_words.len();

    // Cap to prevent excessive computation on very long lines
    if rl > 200 || al > 200 {
        return (removed.to_string(), added.to_string());
    }

    let mut dp = vec![vec![0usize; al + 1]; rl + 1];
    for i in 1..=rl {
        for j in 1..=al {
            if removed_words[i - 1] == added_words[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    // Backtrack to find which words are common vs changed
    let mut removed_changed = vec![true; rl];
    let mut added_changed = vec![true; al];

    let mut i = rl;
    let mut j = al;
    while i > 0 && j > 0 {
        if removed_words[i - 1] == added_words[j - 1] {
            removed_changed[i - 1] = false;
            added_changed[j - 1] = false;
            i -= 1;
            j -= 1;
        } else if dp[i - 1][j] >= dp[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }

    // Format: highlight changed words with brighter colors
    let mut removed_fmt = String::new();
    for (idx, word) in removed_words.iter().enumerate() {
        if removed_changed[idx] {
            removed_fmt.push_str(diff_colors::FG_REMOVED_WORD);
            removed_fmt.push_str(word);
            removed_fmt.push_str(diff_colors::RESET);
        } else {
            removed_fmt.push_str(word);
        }
    }

    let mut added_fmt = String::new();
    for (idx, word) in added_words.iter().enumerate() {
        if added_changed[idx] {
            added_fmt.push_str(diff_colors::FG_ADDED_WORD);
            added_fmt.push_str(word);
            added_fmt.push_str(diff_colors::RESET);
        } else {
            added_fmt.push_str(word);
        }
    }

    (removed_fmt, added_fmt)
}

/// Split a string into words, preserving whitespace as separate tokens.
fn split_words(s: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut start = 0;
    let mut chars = s.char_indices().peekable();

    while let Some((_idx, ch)) = chars.next() {
        let is_space = ch.is_whitespace();
        // Consume all same-category chars
        while let Some(&(_next_idx, next_ch)) = chars.peek() {
            if next_ch.is_whitespace() == is_space {
                chars.next();
            } else {
                break;
            }
        }
        // Find the end index of this run
        let end = chars.peek().map(|(i, _)| *i).unwrap_or_else(|| {
            s.len()
        });
        if start < end {
            words.push(&s[start..end]);
        }
        start = end;
    }
    words
}

/// Classification of a diff line for rendering purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffLineKind {
    FileHeader,
    HunkHeader,
    Added,
    Removed,
    Context,
    NoNewline,
}

/// Classify a single diff line.
fn classify_diff_line(line: &str) -> DiffLineKind {
    if line.starts_with("+++") || (line.starts_with("---") && !line.starts_with("--- /dev/null")) {
        DiffLineKind::FileHeader
    } else if line.starts_with("@@") {
        DiffLineKind::HunkHeader
    } else if line.starts_with('+') {
        DiffLineKind::Added
    } else if line.starts_with('-') {
        DiffLineKind::Removed
    } else if line.starts_with("\\ No newline") {
        DiffLineKind::NoNewline
    } else {
        DiffLineKind::Context
    }
}

/// A single diff line with its classification and optional line number.
struct DiffLine<'a> {
    kind: DiffLineKind,
    content: &'a str,
    old_line: Option<usize>,
    new_line: Option<usize>,
}

/// Format a single diff line with background fills, line numbers, and optional word highlights.
fn format_diff_line_enhanced(
    line: &DiffLine,
    config: &DiffConfig,
    light: bool,
) -> String {
    let reset = diff_colors::RESET;
    let mut out = String::new();

    match line.kind {
        DiffLineKind::FileHeader => {
            out.push_str(colors::BOLD);
            out.push_str(line.content);
            out.push_str(reset);
        }
        DiffLineKind::HunkHeader => {
            out.push_str(colors::CYAN);
            out.push_str(line.content);
            out.push_str(reset);
        }
        DiffLineKind::Added => {
            let bg = if config.bg_fills {
                if light { diff_colors::BG_ADDED_LIGHT } else { diff_colors::BG_ADDED }
            } else {
                ""
            };
            let content = &line.content[1..]; // strip leading '+'
            let gutter = format_line_gutter(line.new_line, line.kind, config, light);
            out.push_str(&gutter);
            out.push_str(bg);
            out.push('+');
            out.push_str(content);
            out.push_str(reset);
        }
        DiffLineKind::Removed => {
            let bg = if config.bg_fills {
                if light { diff_colors::BG_REMOVED_LIGHT } else { diff_colors::BG_REMOVED }
            } else {
                ""
            };
            let content = &line.content[1..]; // strip leading '-'
            let gutter = format_line_gutter(line.old_line, line.kind, config, light);
            out.push_str(&gutter);
            out.push_str(bg);
            out.push('-');
            out.push_str(content);
            out.push_str(reset);
        }
        DiffLineKind::Context => {
            let bg = if config.bg_fills {
                if light { diff_colors::BG_CONTEXT_LIGHT } else { diff_colors::BG_CONTEXT }
            } else {
                ""
            };
            let gutter = format_line_gutter(line.new_line, line.kind, config, light);
            out.push_str(&gutter);
            out.push_str(bg);
            out.push(' ');
            out.push_str(line.content);
            out.push_str(reset);
        }
        DiffLineKind::NoNewline => {
            out.push_str(colors::DIM);
            out.push_str(line.content);
            out.push_str(reset);
        }
    }
    out
}

/// Format the line number gutter for a diff line.
fn format_line_gutter(
    line_num: Option<usize>,
    _kind: DiffLineKind,
    config: &DiffConfig,
    _light: bool,
) -> String {
    if !config.show_line_numbers {
        return String::new();
    }
    let fg_gutter = diff_colors::FG_GUTTER;
    let fg_sep = diff_colors::FG_GUTTER_SEP;
    let reset = diff_colors::RESET;

    let num_str = match line_num {
        Some(n) => format!("{n:>4}"),
        None => "    ".to_string(),
    };
    format!("{fg_gutter}{num_str} {fg_sep}\u{2502}{reset} ")
}

// ── Formatting functions ────────────────────────────────────────────────

/// Format a diff with enhanced color coding, background fills, line numbers,
/// word-level highlighting, and collapse for large diffs.
///
/// Uses default `DiffConfig` settings. For custom configuration, use `format_diff_with_config`.
fn format_diff(s: &str) -> String {
    format_diff_with_config(s, &DiffConfig::default())
}

/// Format a diff with a specific configuration.
fn format_diff_with_config(s: &str, config: &DiffConfig) -> String {
    let light = is_light_terminal();
    let raw_lines: Vec<&str> = s.lines().collect();
    let total_lines = raw_lines.len();

    // Track line numbers across hunks
    let mut old_line: Option<usize> = None;
    let mut new_line: Option<usize> = None;

    // Build classified lines with line numbers
    let mut diff_lines: Vec<DiffLine<'_>> = Vec::new();

    for raw in &raw_lines {
        let kind = classify_diff_line(raw);

        match kind {
            DiffLineKind::HunkHeader => {
                if let Some(h) = parse_hunk_header(raw) {
                    old_line = Some(h.old_start);
                    new_line = Some(h.new_start);
                }
                diff_lines.push(DiffLine {
                    kind,
                    content: raw,
                    old_line: None,
                    new_line: None,
                });
            }
            DiffLineKind::FileHeader | DiffLineKind::NoNewline => {
                diff_lines.push(DiffLine {
                    kind,
                    content: raw,
                    old_line: None,
                    new_line: None,
                });
            }
            DiffLineKind::Added => {
                diff_lines.push(DiffLine {
                    kind,
                    content: raw,
                    old_line: None,
                    new_line,
                });
                if let Some(ref mut n) = new_line { *n += 1; }
            }
            DiffLineKind::Removed => {
                diff_lines.push(DiffLine {
                    kind,
                    content: raw,
                    old_line,
                    new_line: None,
                });
                if let Some(ref mut o) = old_line { *o += 1; }
            }
            DiffLineKind::Context => {
                diff_lines.push(DiffLine {
                    kind,
                    content: raw,
                    old_line,
                    new_line,
                });
                if let Some(ref mut o) = old_line { *o += 1; }
                if let Some(ref mut n) = new_line { *n += 1; }
            }
        }
    }

    // Determine if we need to collapse — find hunk bodies (everything except file/hunk headers)
    let should_collapse = config.max_expanded_lines > 0 && total_lines > config.max_expanded_lines;

    // If word highlighting is enabled, pair adjacent removed/added lines for word diff
    let word_pairs = if config.word_highlight {
        compute_word_pairs(&diff_lines)
    } else {
        Vec::new()
    };

    let mut output = String::new();

    if should_collapse {
        // Collapse: show all headers + first/last N context lines around each hunk
        output.push_str(&format_diff_collapsed(&diff_lines, &word_pairs, config, light));
    } else {
        // Full rendering
        for (idx, dl) in diff_lines.iter().enumerate() {
            if let Some(&(rem_idx, add_idx)) = word_pairs.iter().find(|(r, a)| *r == idx || *a == idx) {
                // This line is part of a word-diff pair; render with word highlights
                if idx == rem_idx {
                    let rem_dl = &diff_lines[rem_idx];
                    let add_dl = &diff_lines[add_idx];
                    let rem_content = rem_dl.content.strip_prefix('-').unwrap_or(rem_dl.content);
                    let add_content = add_dl.content.strip_prefix('+').unwrap_or(add_dl.content);
                    let (rem_fmt, add_fmt) = word_diff(rem_content, add_content);

                    // Render removed with word highlights
                    let bg = if config.bg_fills {
                        if light { diff_colors::BG_REMOVED_LIGHT } else { diff_colors::BG_REMOVED }
                    } else { "" };
                    let gutter = format_line_gutter(rem_dl.old_line, rem_dl.kind, config, light);
                    output.push_str(&gutter);
                    output.push_str(bg);
                    output.push('-');
                    output.push_str(&rem_fmt);
                    output.push_str(diff_colors::RESET);
                    output.push('\n');

                    // Render added with word highlights
                    let bg = if config.bg_fills {
                        if light { diff_colors::BG_ADDED_LIGHT } else { diff_colors::BG_ADDED }
                    } else { "" };
                    let gutter = format_line_gutter(add_dl.new_line, add_dl.kind, config, light);
                    output.push_str(&gutter);
                    output.push_str(bg);
                    output.push('+');
                    output.push_str(&add_fmt);
                    output.push_str(diff_colors::RESET);
                    output.push('\n');
                }
                // Skip the add line since we already rendered it above
                continue;
            }
            output.push_str(&format_diff_line_enhanced(dl, config, light));
            output.push('\n');
        }
    }

    // Append summary stats
    let stats = parse_diff_stats(s);
    output.push_str(&format_diff_summary(&stats));
    output.push('\n');

    output.trim_end().to_string()
}

/// Identify pairs of adjacent removed/added lines for word-level diff.
fn compute_word_pairs<'a>(lines: &[DiffLine<'a>]) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        if lines[i].kind == DiffLineKind::Removed && lines[i + 1].kind == DiffLineKind::Added {
            pairs.push((i, i + 1));
            i += 2;
        } else {
            i += 1;
        }
    }
    pairs
}

/// Format a diff with collapsed hunks for large diffs.
fn format_diff_collapsed(
    lines: &[DiffLine<'_>],
    word_pairs: &[(usize, usize)],
    config: &DiffConfig,
    light: bool,
) -> String {
    let mut output = String::new();

    // Split into hunks: collect ranges of lines between hunk headers
    let mut hunk_ranges: Vec<(usize, usize)> = Vec::new(); // (start_idx, end_idx exclusive)
    let mut hunk_start = 0;

    for (i, dl) in lines.iter().enumerate() {
        if dl.kind == DiffLineKind::HunkHeader && i > 0 {
            hunk_ranges.push((hunk_start, i));
            hunk_start = i;
        }
    }
    if hunk_start < lines.len() {
        hunk_ranges.push((hunk_start, lines.len()));
    }

    let ctx = config.max_context_lines;
    let mut rendered_word_add = std::collections::HashSet::new();

    for (start, end) in &hunk_ranges {
        let hunk_body: Vec<(usize, &DiffLine<'_>)> = lines[*start..*end]
            .iter()
            .enumerate()
            .map(|(i, dl)| (i + *start, dl))
            .collect();

        // Separate headers from body lines
        let header_end = hunk_body.iter()
            .position(|(i, dl)| *i > *start && dl.kind != DiffLineKind::FileHeader && dl.kind != DiffLineKind::HunkHeader)
            .unwrap_or(0);

        // Render file/hunk headers
        for (_, dl) in hunk_body.iter().take(header_end.max(1)) {
            output.push_str(&format_diff_line_enhanced(dl, config, light));
            output.push('\n');
        }

        let body_lines: &[(usize, &DiffLine<'_>)] = &hunk_body[header_end.max(1)..];
        if body_lines.is_empty() {
            continue;
        }

        // If body fits in context, render all
        if body_lines.len() <= ctx * 2 {
            for &(idx, dl) in body_lines {
                if let Some(&(rem_idx, add_idx)) = word_pairs.iter().find(|(r, a)| *r == idx || *a == idx) {
                    if idx == rem_idx && !rendered_word_add.contains(&add_idx) {
                        rendered_word_add.insert(add_idx);
                        let rem_dl = &lines[rem_idx];
                        let add_dl = &lines[add_idx];
                        let rem_content = rem_dl.content.strip_prefix('-').unwrap_or(rem_dl.content);
                        let add_content = add_dl.content.strip_prefix('+').unwrap_or(add_dl.content);
                        let (rem_fmt, add_fmt) = word_diff(rem_content, add_content);

                        let bg = if config.bg_fills {
                            if light { diff_colors::BG_REMOVED_LIGHT } else { diff_colors::BG_REMOVED }
                        } else { "" };
                        let gutter = format_line_gutter(rem_dl.old_line, rem_dl.kind, config, light);
                        output.push_str(&gutter);
                        output.push_str(bg);
                        output.push('-');
                        output.push_str(&rem_fmt);
                        output.push_str(diff_colors::RESET);
                        output.push('\n');

                        let bg = if config.bg_fills {
                            if light { diff_colors::BG_ADDED_LIGHT } else { diff_colors::BG_ADDED }
                        } else { "" };
                        let gutter = format_line_gutter(add_dl.new_line, add_dl.kind, config, light);
                        output.push_str(&gutter);
                        output.push_str(bg);
                        output.push('+');
                        output.push_str(&add_fmt);
                        output.push_str(diff_colors::RESET);
                        output.push('\n');
                    }
                    continue;
                }
                output.push_str(&format_diff_line_enhanced(dl, config, light));
                output.push('\n');
            }
        } else {
            // Render first ctx lines, skip indicator, last ctx lines
            for &(idx, dl) in body_lines.iter().take(ctx) {
                if let Some(&(rem_idx, add_idx)) = word_pairs.iter().find(|(r, a)| *r == idx || *a == idx) {
                    if idx == rem_idx && !rendered_word_add.contains(&add_idx) {
                        rendered_word_add.insert(add_idx);
                        let rem_dl = &lines[rem_idx];
                        let add_dl = &lines[add_idx];
                        let rem_content = rem_dl.content.strip_prefix('-').unwrap_or(rem_dl.content);
                        let add_content = add_dl.content.strip_prefix('+').unwrap_or(add_dl.content);
                        let (rem_fmt, add_fmt) = word_diff(rem_content, add_content);

                        let bg = if config.bg_fills {
                            if light { diff_colors::BG_REMOVED_LIGHT } else { diff_colors::BG_REMOVED }
                        } else { "" };
                        let gutter = format_line_gutter(rem_dl.old_line, rem_dl.kind, config, light);
                        output.push_str(&gutter);
                        output.push_str(bg);
                        output.push('-');
                        output.push_str(&rem_fmt);
                        output.push_str(diff_colors::RESET);
                        output.push('\n');

                        let bg = if config.bg_fills {
                            if light { diff_colors::BG_ADDED_LIGHT } else { diff_colors::BG_ADDED }
                        } else { "" };
                        let gutter = format_line_gutter(add_dl.new_line, add_dl.kind, config, light);
                        output.push_str(&gutter);
                        output.push_str(bg);
                        output.push('+');
                        output.push_str(&add_fmt);
                        output.push_str(diff_colors::RESET);
                        output.push('\n');
                    }
                    continue;
                }
                output.push_str(&format_diff_line_enhanced(dl, config, light));
                output.push('\n');
            }

            let skipped = body_lines.len() - ctx * 2;
            output.push_str(&format!(
                "{}  ... skipped {} lines (Ctrl+F to expand) ...{}\n",
                colors::DIM,
                skipped,
                colors::RESET,
            ));

            for &(idx, dl) in body_lines.iter().rev().take(ctx).rev() {
                if let Some(&(rem_idx, add_idx)) = word_pairs.iter().find(|(r, a)| *r == idx || *a == idx) {
                    if idx == rem_idx && !rendered_word_add.contains(&add_idx) {
                        rendered_word_add.insert(add_idx);
                        let rem_dl = &lines[rem_idx];
                        let add_dl = &lines[add_idx];
                        let rem_content = rem_dl.content.strip_prefix('-').unwrap_or(rem_dl.content);
                        let add_content = add_dl.content.strip_prefix('+').unwrap_or(add_dl.content);
                        let (rem_fmt, add_fmt) = word_diff(rem_content, add_content);

                        let bg = if config.bg_fills {
                            if light { diff_colors::BG_REMOVED_LIGHT } else { diff_colors::BG_REMOVED }
                        } else { "" };
                        let gutter = format_line_gutter(rem_dl.old_line, rem_dl.kind, config, light);
                        output.push_str(&gutter);
                        output.push_str(bg);
                        output.push('-');
                        output.push_str(&rem_fmt);
                        output.push_str(diff_colors::RESET);
                        output.push('\n');

                        let bg = if config.bg_fills {
                            if light { diff_colors::BG_ADDED_LIGHT } else { diff_colors::BG_ADDED }
                        } else { "" };
                        let gutter = format_line_gutter(add_dl.new_line, add_dl.kind, config, light);
                        output.push_str(&gutter);
                        output.push_str(bg);
                        output.push('+');
                        output.push_str(&add_fmt);
                        output.push_str(diff_colors::RESET);
                        output.push('\n');
                    }
                    continue;
                }
                output.push_str(&format_diff_line_enhanced(dl, config, light));
                output.push('\n');
            }
        }
    }

    output
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

/// Format an error result with red coloring, a cross prefix, and subtle background.
fn format_error(tool_name: &str, s: &str, duration_label: &str) -> String {
    let reset = colors::RESET;
    let red = colors::RED;
    let dim = colors::DIM;
    let bold = colors::BOLD;
    let lines: Vec<&str> = s.trim().lines().collect();
    let body = lines.iter().map(|l| format!("  {dim}{l}{reset}")).collect::<Vec<_>>().join("\n");
    format!(
        "{red}{bold}╔═ \u{2717} {tool_name}{duration_label} failed ═{reset}\n{body}\n{red}{bold}╚═══{reset}",
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
        // Should contain ANSI codes for backgrounds and/or foregrounds
        let plain = strip_ansi(&formatted);
        // Content should still be present after stripping ANSI
        assert!(plain.contains("old"));
        assert!(plain.contains("new"));
        assert!(plain.contains("context"));
    }

    #[test]
    fn test_format_diff_background_fills() {
        let diff = "--- a/old.rs\n+++ b/new.rs\n@@ -1,3 +1,3 @@\n-old\n+new\n context\n";
        let formatted = format_diff(diff);
        // Should contain background fill escape codes
        assert!(formatted.contains("\x1b[48;5;")); // background fill sequence
    }

    #[test]
    fn test_format_diff_line_numbers() {
        let diff = "--- a/old.rs\n+++ b/new.rs\n@@ -42,3 +42,3 @@\n-old\n+new\n context\n";
        let formatted = format_diff(diff);
        let plain = strip_ansi(&formatted);
        // Should contain line number 42 in the gutter
        assert!(plain.contains("42"));
        // Should contain the box-drawing gutter separator
        assert!(plain.contains('\u{2502}')); // │
    }

    #[test]
    fn test_format_diff_no_line_numbers() {
        let diff = "--- a/old.rs\n+++ b/new.rs\n@@ -42,3 +42,3 @@\n-old\n+new\n context\n";
        let mut config = DiffConfig::default();
        config.show_line_numbers = false;
        let formatted = format_diff_with_config(diff, &config);
        let plain = strip_ansi(&formatted);
        // Should NOT contain the box-drawing gutter separator
        assert!(!plain.contains('\u{2502}'));
    }

    #[test]
    fn test_format_diff_summary_stats() {
        let diff = "--- a/old.rs\n+++ b/new.rs\n@@ -1,3 +1,4 @@\n-old\n-old2\n+new\n+new2\n+new3\n context\n";
        let formatted = format_diff(diff);
        let plain = strip_ansi(&formatted);
        // Should contain summary: 1 file, 3 additions, 2 deletions
        assert!(plain.contains("1 file changed"));
        assert!(plain.contains("3 additions"));
        assert!(plain.contains("2 deletions"));
    }

    #[test]
    fn test_format_diff_collapsed() {
        // Create a diff with many lines to trigger collapse
        let mut diff = String::from("--- a/big.rs\n+++ b/big.rs\n@@ -1,101 +1,101 @@\n");
        for i in 0..100 {
            diff.push_str(&format!(" context line {i}\n"));
        }
        let mut config = DiffConfig::default();
        config.max_expanded_lines = 50;
        let formatted = format_diff_with_config(&diff, &config);
        let plain = strip_ansi(&formatted);
        // Should contain skip indicator
        assert!(plain.contains("skipped"));
        assert!(plain.contains("Ctrl+F to expand"));
    }

    #[test]
    fn test_format_diff_no_collapse_under_threshold() {
        let diff = "--- a/small.rs\n+++ b/small.rs\n@@ -1,3 +1,3 @@\n-old\n+new\n context\n";
        let mut config = DiffConfig::default();
        config.max_expanded_lines = 100;
        let formatted = format_diff_with_config(diff, &config);
        let plain = strip_ansi(&formatted);
        // Should NOT contain skip indicator
        assert!(!plain.contains("skipped"));
    }

    // ── Word diff tests ───────────────────────────────────────────────

    #[test]
    fn test_word_diff_changed_word() {
        let (removed, added) = word_diff("old_function_name()", "new_function_name()");
        // The changed portion should have bright highlight
        assert!(removed.contains(diff_colors::FG_REMOVED_WORD));
        assert!(added.contains(diff_colors::FG_ADDED_WORD));
        // The common portion should NOT have highlight
        let removed_plain = strip_ansi(&removed);
        let added_plain = strip_ansi(&added);
        assert!(removed_plain.contains("_function_name()"));
        assert!(added_plain.contains("_function_name()"));
    }

    #[test]
    fn test_word_diff_identical() {
        let (removed, added) = word_diff("same_line()", "same_line()");
        // Identical lines should have no highlights
        assert!(!removed.contains(diff_colors::FG_REMOVED_WORD));
        assert!(!added.contains(diff_colors::FG_ADDED_WORD));
    }

    #[test]
    fn test_word_diff_completely_different() {
        let (removed, added) = word_diff("foo", "bar");
        // Completely different — all words should be highlighted
        assert!(removed.contains(diff_colors::FG_REMOVED_WORD));
        assert!(added.contains(diff_colors::FG_ADDED_WORD));
    }

    #[test]
    fn test_word_diff_long_line_fallback() {
        // Very long line should fall back to no word diff (cap at 200 words)
        let long_removed: String = "word ".repeat(201);
        let long_added: String = "other ".repeat(201);
        let (removed, added) = word_diff(&long_removed, &long_added);
        // Should just return the plain text without highlights
        assert!(!removed.contains(diff_colors::FG_REMOVED_WORD));
        assert!(!added.contains(diff_colors::FG_ADDED_WORD));
    }

    // ── Diff stats tests ───────────────────────────────────────────────

    #[test]
    fn test_parse_diff_stats_basic() {
        let diff = "--- a/old.rs\n+++ b/new.rs\n@@ -1,3 +1,4 @@\n-old\n-old2\n+new\n+new2\n+new3\n context\n";
        let stats = parse_diff_stats(diff);
        assert_eq!(stats.files_changed, 1);
        assert_eq!(stats.additions, 3);
        assert_eq!(stats.deletions, 2);
    }

    #[test]
    fn test_parse_diff_stats_no_file_header() {
        let diff = "+added line\n-removed line\n context\n";
        let stats = parse_diff_stats(diff);
        assert_eq!(stats.files_changed, 1); // counts as 1 file when no header
        assert_eq!(stats.additions, 1);
        assert_eq!(stats.deletions, 1);
    }

    #[test]
    fn test_parse_diff_stats_empty() {
        let stats = parse_diff_stats("");
        assert_eq!(stats.files_changed, 0);
        assert_eq!(stats.additions, 0);
        assert_eq!(stats.deletions, 0);
    }

    #[test]
    fn test_format_diff_summary_singular() {
        let stats = DiffStats { files_changed: 1, additions: 1, deletions: 1 };
        let summary = format_diff_summary(&stats);
        let plain = strip_ansi(&summary);
        assert!(plain.contains("1 file changed"));
        assert!(plain.contains("1 addition"));
        assert!(plain.contains("1 deletion"));
    }

    #[test]
    fn test_format_diff_summary_plural() {
        let stats = DiffStats { files_changed: 3, additions: 5, deletions: 2 };
        let summary = format_diff_summary(&stats);
        let plain = strip_ansi(&summary);
        assert!(plain.contains("3 files changed"));
        assert!(plain.contains("5 additions"));
        assert!(plain.contains("2 deletions"));
    }

    // ── Hunk header parsing tests ──────────────────────────────────────

    #[test]
    fn test_parse_hunk_header_full() {
        let h = parse_hunk_header("@@ -10,5 +20,3 @@ fn foo()").unwrap();
        assert_eq!(h.old_start, 10);
        assert_eq!(h.old_count, 5);
        assert_eq!(h.new_start, 20);
        assert_eq!(h.new_count, 3);
    }

    #[test]
    fn test_parse_hunk_header_no_count() {
        let h = parse_hunk_header("@@ -1 +1 @@").unwrap();
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_count, 1);
        assert_eq!(h.new_start, 1);
        assert_eq!(h.new_count, 1);
    }

    #[test]
    fn test_parse_hunk_header_invalid() {
        assert!(parse_hunk_header("not a hunk header").is_none());
    }

    // ── Background fill / light terminal tests ─────────────────────────

    #[test]
    fn test_diff_config_default() {
        let config = DiffConfig::default();
        assert_eq!(config.max_context_lines, 3);
        assert_eq!(config.max_expanded_lines, 100);
        assert!(config.show_line_numbers);
        assert!(config.word_highlight);
        assert!(config.bg_fills);
    }

    #[test]
    fn test_diff_config_no_bg_fills() {
        let diff = "--- a/old.rs\n+++ b/new.rs\n@@ -1,3 +1,3 @@\n-old\n+new\n ctx\n";
        let mut config = DiffConfig::default();
        config.bg_fills = false;
        let formatted = format_diff_with_config(diff, &config);
        // Should not contain background fill codes
        assert!(!formatted.contains(diff_colors::BG_ADDED));
        assert!(!formatted.contains(diff_colors::BG_REMOVED));
        assert!(!formatted.contains(diff_colors::BG_ADDED_LIGHT));
        assert!(!formatted.contains(diff_colors::BG_REMOVED_LIGHT));
    }

    #[test]
    fn test_diff_config_no_word_highlight() {
        let diff = "--- a/old.rs\n+++ b/new.rs\n@@ -1 +1 @@\n-old_function_name()\n+new_function_name()\n";
        let mut config = DiffConfig::default();
        config.word_highlight = false;
        let formatted = format_diff_with_config(diff, &config);
        // Should not contain word-level highlight codes
        assert!(!formatted.contains(diff_colors::FG_ADDED_WORD));
        assert!(!formatted.contains(diff_colors::FG_REMOVED_WORD));
    }

    // ── Split words tests ──────────────────────────────────────────────

    #[test]
    fn test_split_words_basic() {
        let words = split_words("hello world");
        assert_eq!(words, vec!["hello", " ", "world"]);
    }

    #[test]
    fn test_split_words_punctuation() {
        let words = split_words("foo.bar");
        assert_eq!(words, vec!["foo.bar"]);
    }

    #[test]
    fn test_split_words_empty() {
        let words = split_words("");
        assert!(words.is_empty());
    }

    // ── Existing formatting tests ──────────────────────────────────────

    #[test]
    fn test_format_json_pretty_print() {
        let json = r#"{"name":"test","value":42}"#;
        let formatted = format_json(json);
        let plain = strip_ansi(&formatted);
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
        assert!(plain.contains(invalid));
    }

    #[test]
    fn test_format_json_truncation() {
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
        assert!(formatted.contains("\u{251C}\u{2500}\u{2500} ")); // ├──
        assert!(formatted.contains("\u{2514}\u{2500}\u{2500} ")); // └──
        assert!(formatted.contains("src/main.rs"));
        assert!(formatted.contains("Cargo.toml"));
    }

    #[test]
    fn test_format_file_tree_single() {
        let list = "only_file.rs\n";
        let formatted = format_file_tree(list);
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
        assert!(formatted.contains('-'));
    }

    #[test]
    fn test_format_error_basic() {
        let formatted = format_error("bash", "command not found", "");
        assert!(formatted.contains("\u{2717}")); // ✗
        assert!(formatted.contains("bash failed"));
        assert!(formatted.contains("command not found"));
        assert!(formatted.contains(colors::RED));
    }

    #[test]
    fn test_format_error_has_border() {
        let formatted = format_error("bash", "command not found", "");
        // Should contain box-drawing border
        assert!(formatted.contains("╔") || formatted.contains("╚"), "Error formatting should include box border");
    }

    // ── Main entry point tests ────────────────────────────────────────

    #[test]
    fn test_format_tool_result_error() {
        let result = format_tool_result("bash", "command failed", true, None);
        assert!(result.contains("\u{2717}"));
        assert!(result.contains("bash failed"));
    }

    #[test]
    fn test_format_tool_result_diff() {
        let diff = "--- a/old.rs\n+++ b/new.rs\n@@ -1 +1 @@\n-old\n+new\n";
        let result = format_tool_result_full("edit", diff, false);
        let plain = strip_ansi(&result);
        assert!(plain.contains("old"));
        assert!(plain.contains("new"));
    }

    #[test]
    fn test_format_tool_result_json() {
        let json = r#"{"status":"ok","count":5}"#;
        let result = format_tool_result("query", json, false, None);
        assert!(result.contains('\n'));
    }

    #[test]
    fn test_format_tool_result_file_list() {
        let list = "src/main.rs\nsrc/lib.rs\nCargo.toml\n";
        let result = format_tool_result("glob", list, false, None);
        // File lists are rendered as a tree by format_file_tree
        let plain = strip_ansi(&result);
        assert!(plain.contains("src/main.rs"));
        assert!(plain.contains("src/lib.rs"));
        assert!(plain.contains("Cargo.toml"));
    }

    #[test]
    fn test_format_tool_result_table() {
        let table = "| Name | Value |\n| --- | --- |\n| foo | bar |\n";
        let result = format_tool_result("query", table, false, None);
        let plain = strip_ansi(&result);
        assert!(plain.contains("foo"));
        assert!(plain.contains("bar"));
    }

    #[test]
    fn test_format_tool_result_plain_text() {
        let text = "Just some plain text output from a tool.";
        let result = format_tool_result("echo", text, false, None);
        // echo is Read category → format_read_summary
        let plain = strip_ansi(&result);
        assert!(plain.contains("Just some plain text"));
    }

    #[test]
    fn test_format_tool_result_truncation() {
        let long_text = "x".repeat(5000);
        // Use format_write_result for truncation since Read category summarizes
        let result = format_tool_result("bash", &long_text, false, None);
        let plain = strip_ansi(&result);
        assert!(plain.contains("bash"));
    }

    #[test]
    fn test_format_tool_result_empty_string() {
        let result = format_tool_result("read", "", false, None);
        let plain = strip_ansi(&result);
        assert!(plain.contains("0 lines") || plain.is_empty());
    }

    #[test]
    fn test_format_tool_result_whitespace_only() {
        let result = format_tool_result("read", "   \n  \n  ", false, None);
        let plain = strip_ansi(&result);
        assert!(plain.contains("0 lines") || plain.is_empty());
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn test_looks_like_diff_mostly_plus_minus() {
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
        assert!(formatted.contains("src/main.rs"));
        assert!(formatted.contains("src/lib.rs"));
        assert!(formatted.contains("Cargo.toml"));
        assert_eq!(formatted.lines().count(), 3);
    }

    #[test]
    fn test_format_write_result_has_icon() {
        let result = format_write_result("edit", "done", false, "");
        let plain = strip_ansi(&result);
        assert!(plain.contains("edit"));
    }

    #[test]
    fn test_format_write_result_error_delegates() {
        let result = format_write_result("bash", "failed", true, "");
        assert!(result.contains("\u{2717}")); // ✗
        assert!(result.contains("bash failed"));
    }

    #[test]
    fn test_classify_diff_line() {
        assert_eq!(classify_diff_line("+++ b/foo.rs"), DiffLineKind::FileHeader);
        assert_eq!(classify_diff_line("--- a/foo.rs"), DiffLineKind::FileHeader);
        assert_eq!(classify_diff_line("--- /dev/null"), DiffLineKind::Removed);
        assert_eq!(classify_diff_line("@@ -1,3 +1,4 @@"), DiffLineKind::HunkHeader);
        assert_eq!(classify_diff_line("+added"), DiffLineKind::Added);
        assert_eq!(classify_diff_line("-removed"), DiffLineKind::Removed);
        assert_eq!(classify_diff_line(" context"), DiffLineKind::Context);
        assert_eq!(classify_diff_line("\\ No newline at end of file"), DiffLineKind::NoNewline);
    }

    #[test]
    fn test_tool_category_skill() {
        assert_eq!(tool_category("skill_commit"), ToolCategory::Skill);
        assert_eq!(tool_category("skill_my-custom-skill"), ToolCategory::Skill);
        assert_eq!(tool_category("skill_pdf"), ToolCategory::Skill);
        // Non-skill tools should still match their categories
        assert_eq!(tool_category("bash"), ToolCategory::Bash);
        assert_eq!(tool_category("read"), ToolCategory::Read);
        assert_eq!(tool_category("agent"), ToolCategory::Agent);
    }

    #[test]
    fn test_display_tool_name_mcp() {
        assert_eq!(display_tool_name("mcp__plugin_serena_serena__find_symbol"), "serena: find_symbol");
        assert_eq!(display_tool_name("mcp__plugin_playwright_playwright__browser_click"), "playwright: browser_click");
        assert_eq!(display_tool_name("mcp__plugin_context7_context7__resolve-library-id"), "context7: resolve-library-id");
    }

    #[test]
    fn test_display_tool_name_non_mcp() {
        assert_eq!(display_tool_name("bash"), "bash");
        assert_eq!(display_tool_name("read"), "read");
        assert_eq!(display_tool_name("skill_commit"), "commit");
        assert_eq!(display_tool_name("skill_pdf"), "pdf");
    }
}
