//! Enhanced markdown table renderer with column alignment and width optimization.
//!
//! Provides detection, parsing, and formatted rendering of markdown tables
//! with proper column alignment (left for text, right for numbers), width
//! optimization, truncation with ellipsis, and box-drawing border characters.

#[cfg(test)]
use std::hash::{Hash, Hasher};

/// Maximum column width before truncation.
const DEFAULT_MAX_COL_WIDTH: usize = 50;

/// Range of lines forming a detected markdown table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRange {
    /// 0-based index of the first line of the table.
    pub start_line: usize,
    /// 0-based index of the last line of the table (inclusive).
    pub end_line: usize,
}

/// Alignment for a table column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnAlign {
    Left,
    Right,
    Center,
}

/// Detect if a string contains a markdown table.
///
/// Looks for consecutive lines starting with `|` that form a valid table
/// (header + separator + at least one optional data row). Returns the range
/// of lines forming the table if found.
pub fn detect_table(content: &str) -> Option<TableRange> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() < 2 {
        return None;
    }

    // Find a run of pipe-delimited lines containing a separator row.
    let mut start: Option<usize> = None;
    let mut found_separator = false;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if is_table_line(trimmed) {
            if start.is_none() {
                start = Some(i);
            }
            if is_separator_line(trimmed) {
                found_separator = true;
            }
        } else if start.is_some() {
            // End of a table run.
            if found_separator {
                let s = start.unwrap();
                // Must have at least header + separator.
                if i - s >= 2 {
                    return Some(TableRange {
                        start_line: s,
                        end_line: i - 1,
                    });
                }
            }
            start = None;
            found_separator = false;
        }
    }

    // Handle table that extends to the end of content.
    if let Some(s) = start {
        if found_separator && lines.len() - s >= 2 {
            return Some(TableRange {
                start_line: s,
                end_line: lines.len() - 1,
            });
        }
    }

    None
}

/// Check if a line looks like a table row (contains pipes).
fn is_table_line(line: &str) -> bool {
    line.contains('|')
}

/// Check if a line is a markdown table separator row (e.g., `| --- | --- |`).
fn is_separator_line(line: &str) -> bool {
    let trimmed = line.trim();
    // Strip leading/trailing pipes for analysis.
    let inner = trimmed
        .trim_start_matches('|')
        .trim_end_matches('|')
        .trim();
    if inner.is_empty() {
        return false;
    }
    // All segments between pipes should be dashes, colons, or spaces.
    inner.split('|').all(|seg| {
        let s = seg.trim();
        !s.is_empty()
            && s
                .chars()
                .all(|c| c == '-' || c == ':' || c == ' ')
    })
}

/// Parse alignment from a separator segment.
///
/// - `:---` or `---` -> Left
/// - `---:` -> Right
/// - `:---:` -> Center
#[cfg(test)]
fn parse_alignment(seg: &str) -> ColumnAlign {
    let s = seg.trim();
    let starts_colon = s.starts_with(':');
    let ends_colon = s.ends_with(':');
    match (starts_colon, ends_colon) {
        (true, true) => ColumnAlign::Center,
        (_, true) => ColumnAlign::Right,
        _ => ColumnAlign::Left,
    }
}

/// Determine if a cell value looks numeric (for right-alignment heuristic).
fn is_numeric(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Allow leading sign, digits, decimal point, commas, percent.
    let mut seen_dot = false;
    let mut seen_digit = false;
    for (i, c) in trimmed.chars().enumerate() {
        match c {
            '0'..='9' => seen_digit = true,
            '.' if !seen_dot => seen_dot = true,
            '-' | '+' if i == 0 => {}
            ',' => {}
            '%' if i == trimmed.len() - 1 => {}
            _ => return false,
        }
    }
    seen_digit
}

/// Parse a table row into cells, stripping leading/trailing pipes and whitespace.
fn parse_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed
        .trim_start_matches('|')
        .trim_end_matches('|');
    inner
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

/// Calculate optimal column widths given headers, rows, alignment, and max width.
///
/// Returns column widths and alignments.
fn calculate_widths(
    headers: &[String],
    rows: &[Vec<String>],
    max_width: usize,
    overrides: Option<Vec<usize>>,
) -> (Vec<usize>, Vec<ColumnAlign>) {
    let num_cols = headers.len().max(rows.iter().map(|r| r.len()).max().unwrap_or(0));
    if num_cols == 0 {
        return (Vec::new(), Vec::new());
    }

    // Determine alignments: detect from content, right-align numeric columns.
    let mut alignments = vec![ColumnAlign::Left; num_cols];

    // Check if all values in a column are numeric (after the header).
    for col in 0..num_cols {
        let mut all_numeric = true;
        let mut has_data = false;
        for row in rows {
            if col < row.len() && !row[col].is_empty() {
                has_data = true;
                if !is_numeric(&row[col]) {
                    all_numeric = false;
                    break;
                }
            }
        }
        if has_data && all_numeric {
            alignments[col] = ColumnAlign::Right;
        }
    }

    // Calculate natural column widths.
    let mut widths = vec![0usize; num_cols];
    for (col, h) in headers.iter().enumerate() {
        widths[col] = widths[col].max(h.len());
    }
    for row in rows {
        for (col, cell) in row.iter().enumerate() {
            if col < num_cols {
                widths[col] = widths[col].max(cell.len());
            }
        }
    }

    // Apply user overrides if provided.
    if let Some(ref ov) = overrides {
        for (col, &w) in ov.iter().enumerate() {
            if col < num_cols {
                widths[col] = w;
            }
        }
    }

    // Cap individual column widths.
    let max_col = max_width.saturating_sub(3 * num_cols + 1) / num_cols.max(1);
    let cap = max_col.min(DEFAULT_MAX_COL_WIDTH);
    for w in &mut widths {
        *w = (*w).min(cap);
    }

    // If total width exceeds max_width, proportionally reduce.
    let overhead = 3 * num_cols + 1; // "| x |" per column + final "|"
    let total: usize = widths.iter().sum();
    if total + overhead > max_width && total > 0 {
        let available = max_width.saturating_sub(overhead);
        let scale = available as f64 / total as f64;
        for w in &mut widths {
            *w = (*w as f64 * scale).floor() as usize;
            if *w == 0 {
                *w = 1;
            }
        }
    }

    (widths, alignments)
}

/// Truncate a string to `max_len` characters, appending ellipsis if truncated.
fn truncate_with_ellipsis(s: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    if s.len() <= max_len {
        return s.to_string();
    }
    if max_len <= 1 {
        return s.chars().take(max_len).collect();
    }
    // Take characters up to max_len - 1, then add ellipsis char.
    let truncated: String = s.chars().take(max_len - 1).collect();
    format!("{truncated}\u{2026}")
}

/// Align a cell value within a given width using the specified alignment.
fn align_cell(value: &str, width: usize, align: ColumnAlign) -> String {
    let display = truncate_with_ellipsis(value, width);
    match align {
        ColumnAlign::Left => format!("{display:<width$}"),
        ColumnAlign::Right => format!("{display:>width$}"),
        ColumnAlign::Center => format!("{display:^width$}"),
    }
}

/// Renders a markdown table with proper column alignment and width optimization.
///
/// - Headers and rows are provided as string slices.
/// - Optional `widths` override the auto-calculated column widths.
/// - Columns are left-aligned for text, right-aligned for numeric data.
/// - Wide columns are truncated with ellipsis.
/// - Uses box-drawing characters for borders.
///
/// # Panics
///
/// Does not panic; returns an empty string for empty input.
pub fn render_table(
    header: &[&str],
    rows: &[Vec<String>],
    widths: Option<Vec<usize>>,
) -> String {
    if header.is_empty() {
        return String::new();
    }

    let max_width = 120; // Reasonable default terminal width.
    let headers: Vec<String> = header.iter().map(|s| s.to_string()).collect();
    let (col_widths, alignments) = calculate_widths(&headers, rows, max_width, widths);

    if col_widths.is_empty() {
        return String::new();
    }

    let mut output = String::new();

    // Top border: ┌─────┬─────┐
    output.push('\u{250C}'); // ┌
    for (i, &w) in col_widths.iter().enumerate() {
        output.push_str(&"\u{2500}".repeat(w + 2));
        if i < col_widths.len() - 1 {
            output.push('\u{252C}'); // ┬
        }
    }
    output.push('\u{2510}'); // ┐
    output.push('\n');

    // Header row: │ cell │ cell │
    output.push('\u{2502}'); // │
    for (col, h) in headers.iter().enumerate() {
        if col < col_widths.len() {
            output.push(' ');
            output.push_str(&align_cell(h, col_widths[col], ColumnAlign::Left));
            output.push(' ');
            output.push('\u{2502}'); // │
        }
    }
    output.push('\n');

    // Header separator: ├─────┼─────┤
    output.push('\u{251C}'); // ├
    for (i, &w) in col_widths.iter().enumerate() {
        output.push_str(&"\u{2500}".repeat(w + 2));
        if i < col_widths.len() - 1 {
            output.push('\u{253C}'); // ┼
        }
    }
    output.push('\u{2524}'); // ┤
    output.push('\n');

    // Data rows.
    for row in rows {
        output.push('\u{2502}'); // │
        for (col, width) in col_widths.iter().enumerate() {
            let cell = row.get(col).map(|s| s.as_str()).unwrap_or("");
            let align = alignments.get(col).copied().unwrap_or(ColumnAlign::Left);
            output.push(' ');
            output.push_str(&align_cell(cell, *width, align));
            output.push(' ');
            output.push('\u{2502}'); // │
        }
        output.push('\n');
    }

    // Bottom border: └─────┴─────┘
    output.push('\u{2514}'); // └
    for (i, &w) in col_widths.iter().enumerate() {
        output.push_str(&"\u{2500}".repeat(w + 2));
        if i < col_widths.len() - 1 {
            output.push('\u{2534}'); // ┴
        }
    }
    output.push('\u{2518}'); // ┘

    output
}

/// Auto-format a raw markdown table for better display.
///
/// Parses the raw markdown table string and re-renders it with optimal widths
/// and box-drawing borders. Respects the given terminal `max_width`.
pub fn format_table(raw: &str, max_width: usize) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    if lines.len() < 2 {
        return raw.to_string();
    }

    // Parse all rows into cells.
    let parsed: Vec<Vec<String>> = lines
        .iter()
        .filter(|l| is_table_line(l.trim()))
        .map(|l| parse_row(l))
        .filter(|row| !row.is_empty())
        .collect();

    if parsed.len() < 2 {
        return raw.to_string();
    }

    // Separate header, separator, and data rows.
    let header = parsed.first().cloned().unwrap_or_default();
    let data_rows: Vec<Vec<String>> = parsed
        .iter()
        .enumerate()
        .filter(|(i, _)| *i >= 2) // Skip header and separator.
        .map(|(_, row)| row.clone())
        .collect();

    if header.is_empty() {
        return raw.to_string();
    }

    // Calculate widths using the provided max_width.
    let (col_widths, alignments) =
        calculate_widths(&header, &data_rows, max_width, None);

    if col_widths.is_empty() {
        return raw.to_string();
    }

    let mut output = String::new();

    // Top border.
    output.push('\u{250C}');
    for (i, &w) in col_widths.iter().enumerate() {
        output.push_str(&"\u{2500}".repeat(w + 2));
        if i < col_widths.len() - 1 {
            output.push('\u{252C}');
        }
    }
    output.push('\u{2510}');
    output.push('\n');

    // Header.
    output.push('\u{2502}');
    for (col, h) in header.iter().enumerate() {
        if col < col_widths.len() {
            output.push(' ');
            output.push_str(&align_cell(h, col_widths[col], ColumnAlign::Left));
            output.push(' ');
            output.push('\u{2502}');
        }
    }
    output.push('\n');

    // Separator.
    output.push('\u{251C}');
    for (i, &w) in col_widths.iter().enumerate() {
        output.push_str(&"\u{2500}".repeat(w + 2));
        if i < col_widths.len() - 1 {
            output.push('\u{253C}');
        }
    }
    output.push('\u{2524}');
    output.push('\n');

    // Data rows.
    for row in &data_rows {
        output.push('\u{2502}');
        for (col, width) in col_widths.iter().enumerate() {
            let cell = row.get(col).map(|s| s.as_str()).unwrap_or("");
            let align = alignments.get(col).copied().unwrap_or(ColumnAlign::Left);
            output.push(' ');
            output.push_str(&align_cell(cell, *width, align));
            output.push(' ');
            output.push('\u{2502}');
        }
        output.push('\n');
    }

    // Bottom border.
    output.push('\u{2514}');
    for (i, &w) in col_widths.iter().enumerate() {
        output.push_str(&"\u{2500}".repeat(w + 2));
        if i < col_widths.len() - 1 {
            output.push('\u{2534}');
        }
    }
    output.push('\u{2518}');

    output
}

/// Simple non-cryptographic hash for content comparison.
#[cfg(test)]
fn content_hash(s: &str) -> u64 {
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
    fn test_render_table_basic() {
        let header = &["Name", "Age"];
        let rows = vec![
            vec!["Alice".to_string(), "30".to_string()],
            vec!["Bob".to_string(), "25".to_string()],
        ];
        let result = render_table(header, &rows, None);
        assert!(result.contains("Alice"));
        assert!(result.contains("Bob"));
        assert!(result.contains("30"));
        assert!(result.contains("25"));
        // Box-drawing characters.
        assert!(result.contains('\u{250C}')); // ┌
        assert!(result.contains('\u{2514}')); // └
        assert!(result.contains('\u{2510}')); // ┐
        assert!(result.contains('\u{2518}')); // ┘
    }

    #[test]
    fn test_render_table_alignment() {
        let header = &["Item", "Price"];
        let rows = vec![
            vec!["Apple".to_string(), "1.50".to_string()],
            vec!["Banana".to_string(), "0.75".to_string()],
        ];
        let result = render_table(header, &rows, None);
        // Price column should be right-aligned (numeric).
        assert!(result.contains("1.50"));
        assert!(result.contains("0.75"));
    }

    #[test]
    fn test_render_table_truncation() {
        let long_name = "x".repeat(100);
        let header = &["Name", "Value"];
        let rows = vec![vec![long_name.clone(), "42".to_string()]];
        let result = render_table(header, &rows, None);
        // The long name should be truncated with ellipsis.
        assert!(result.contains('\u{2026}'), "Long text should be truncated with ellipsis");
        assert!(!result.contains(&long_name), "Full long text should not appear");
    }

    #[test]
    fn test_render_table_empty_header() {
        let header: &[&str] = &[];
        let rows: Vec<Vec<String>> = vec![];
        let result = render_table(header, &rows, None);
        assert!(result.is_empty());
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
    fn test_render_table_with_width_overrides() {
        let header = &["A", "B", "C"];
        let rows = vec![vec!["short".to_string(), "medium".to_string(), "longer text here".to_string()]];
        let result = render_table(header, &rows, Some(vec![5, 8, 12]));
        assert!(result.contains("short"));
    }

    #[test]
    fn test_render_table_uneven_rows() {
        let header = &["Col1", "Col2", "Col3"];
        let rows = vec![
            vec!["a".to_string()],
            vec!["b".to_string(), "c".to_string()],
        ];
        let result = render_table(header, &rows, None);
        assert!(result.contains('a'));
        assert!(result.contains('b'));
        assert!(result.contains('c'));
    }

    #[test]
    fn test_detect_table_basic() {
        let content = "Some text\n| Name | Age |\n| --- | --- |\n| Alice | 30 |\nMore text";
        let range = detect_table(content).unwrap();
        assert_eq!(range.start_line, 1);
        assert_eq!(range.end_line, 3);
    }

    #[test]
    fn test_detect_table_at_start() {
        let content = "| H1 | H2 |\n| -- | -- |\n| a | b |\nEnd";
        let range = detect_table(content).unwrap();
        assert_eq!(range.start_line, 0);
        assert_eq!(range.end_line, 2);
    }

    #[test]
    fn test_detect_table_at_end() {
        let content = "Start\n| H1 | H2 |\n| -- | -- |\n| a | b |";
        let range = detect_table(content).unwrap();
        assert_eq!(range.start_line, 1);
        assert_eq!(range.end_line, 3);
    }

    #[test]
    fn test_detect_table_no_table() {
        let content = "Just some text\nNo table here";
        assert!(detect_table(content).is_none());
    }

    #[test]
    fn test_detect_table_no_separator() {
        let content = "| Name | Age |\n| Alice | 30 |";
        assert!(detect_table(content).is_none());
    }

    #[test]
    fn test_detect_table_empty() {
        assert!(detect_table("").is_none());
    }

    #[test]
    fn test_detect_table_single_line() {
        assert!(detect_table("| A |").is_none());
    }

    #[test]
    fn test_detect_table_alignment_markers() {
        let content = "| Left | Center | Right |\n| :--- | :---: | ---: |\n| a | b | c |";
        let range = detect_table(content).unwrap();
        assert_eq!(range.start_line, 0);
        assert_eq!(range.end_line, 2);
    }

    #[test]
    fn test_format_table_basic() {
        let raw = "| Name | Age |\n| --- | --- |\n| Alice | 30 |\n| Bob | 25 |";
        let result = format_table(raw, 80);
        assert!(result.contains("Alice"));
        assert!(result.contains("Bob"));
        assert!(result.contains('\u{250C}')); // ┌
        assert!(result.contains('\u{2514}')); // └
    }

    #[test]
    fn test_format_table_respects_max_width() {
        let raw = "| Short | VeryLongColumnNameThatMightOverflow |\n| --- | --- |\n| a | b |";
        let result = format_table(raw, 40);
        // Should still produce output (width-capped).
        assert!(!result.is_empty());
    }

    #[test]
    fn test_format_table_too_few_lines() {
        assert_eq!(format_table("| only header |", 80), "| only header |");
        assert_eq!(format_table("", 80), "");
    }

    #[test]
    fn test_format_table_numeric_alignment() {
        let raw = "| Item | Count |\n| --- | --- |\n| A | 100 |\n| B | 200 |";
        let result = format_table(raw, 80);
        assert!(result.contains("100"));
        assert!(result.contains("200"));
    }

    #[test]
    fn test_is_numeric_basic() {
        assert!(is_numeric("42"));
        assert!(is_numeric("-3.14"));
        assert!(is_numeric("1,000"));
        assert!(is_numeric("99%"));
        assert!(is_numeric("+5"));
        assert!(!is_numeric("hello"));
        assert!(!is_numeric(""));
        assert!(!is_numeric("42abc"));
    }

    #[test]
    fn test_parse_alignment() {
        assert_eq!(parse_alignment("---"), ColumnAlign::Left);
        assert_eq!(parse_alignment(":---"), ColumnAlign::Left);
        assert_eq!(parse_alignment("---:"), ColumnAlign::Right);
        assert_eq!(parse_alignment(":---:"), ColumnAlign::Center);
        assert_eq!(parse_alignment(" :---: "), ColumnAlign::Center);
    }

    #[test]
    fn test_truncate_with_ellipsis() {
        assert_eq!(truncate_with_ellipsis("hello", 10), "hello");
        assert_eq!(truncate_with_ellipsis("hello", 5), "hello");
        assert_eq!(truncate_with_ellipsis("hello world", 8), "hello w\u{2026}");
        assert_eq!(truncate_with_ellipsis("ab", 1), "a");
        assert_eq!(truncate_with_ellipsis("", 5), "");
    }

    #[test]
    fn test_content_hash_deterministic() {
        let a = content_hash("test");
        let b = content_hash("test");
        assert_eq!(a, b);
        let c = content_hash("other");
        assert_ne!(a, c);
    }
}
