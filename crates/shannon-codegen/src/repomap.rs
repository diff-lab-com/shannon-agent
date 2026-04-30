//! Repository map generation
//!
//! Walks directories and generates a summary of code symbols across the project.

use crate::outline::{file_outline, Symbol};
use crate::languages::language_for_path;
use crate::{CodegenError, Result};
use ignore::WalkBuilder;
use serde::Serialize;
use std::path::Path;

/// Repository map containing file summaries
#[derive(Debug, Clone, Serialize)]
pub struct RepoMap {
    /// File summaries
    pub files: Vec<FileSummary>,
    /// Total symbol count
    pub total_symbols: usize,
    /// Total lines of code
    pub total_lines: usize,
}

/// Summary of a single file
#[derive(Debug, Clone, Serialize)]
pub struct FileSummary {
    /// Relative file path
    pub path: String,
    /// Programming language
    pub language: String,
    /// Top-level symbols
    pub symbols: Vec<Symbol>,
    /// Number of lines
    pub lines: usize,
}

/// Generate repository map
///
/// # Arguments
///
/// * `root` - Root directory to scan
/// * `max_files` - Maximum number of files to process
///
/// # Returns
///
/// Repository map with file summaries
///
/// # Errors
///
/// Returns error if directory cannot be read
pub fn generate_repomap(root: &Path, max_files: usize) -> Result<RepoMap> {
    generate_repomap_filtered(root, &[], max_files)
}

/// Generate repository map with extension filter
///
/// # Arguments
///
/// * `root` - Root directory to scan
/// * `extensions` - File extensions to include (empty = all supported)
/// * `max_files` - Maximum number of files to process
///
/// # Returns
///
/// Repository map with file summaries
///
/// # Errors
///
/// Returns error if directory cannot be read
pub fn generate_repomap_filtered(
    root: &Path,
    extensions: &[&str],
    max_files: usize,
) -> Result<RepoMap> {
    let mut files = Vec::new();
    let mut total_symbols = 0;
    let mut total_lines = 0;

    // Build walker with .gitignore respect
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .parents(true)
        .git_ignore(true)
        .git_exclude(true)
        .build();

    // Process files
    for entry in walker {
        let entry = entry.map_err(|e| CodegenError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        )))?;

        let path = entry.path();

        // Skip directories
        if path.is_dir() {
            continue;
        }

        // Check extension filter
        if !extensions.is_empty() {
            let ext = path.extension().and_then(|e| e.to_str());
            let ext_match = ext.map(|e| extensions.contains(&e)).unwrap_or(false);
            if !ext_match {
                continue;
            }
        }

        // Check if supported language
        let lang_config = match language_for_path(path) {
            Some(l) => l,
            None => continue,
        };

        // Skip if max files reached
        if files.len() >= max_files {
            break;
        }

        // Extract symbols
        let symbols = match file_outline(path) {
            Ok(s) => s,
            Err(_) => continue, // Skip files that fail to parse
        };

        // Count lines
        let lines = match std::fs::read_to_string(path) {
            Ok(content) => content.lines().count(),
            Err(_) => 0,
        };

        // Get relative path
        let relative_path = path.strip_prefix(root)
            .ok()
            .and_then(|p| p.to_str())
            .unwrap_or_else(|| path.to_str().unwrap_or("unknown"))
            .to_string();

        files.push(FileSummary {
            path: relative_path,
            language: lang_config.name.to_string(),
            symbols,
            lines,
        });

        total_symbols += files.last().map(|f| f.symbols.len()).unwrap_or(0);
        total_lines += lines;
    }

    // Sort files by path
    files.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(RepoMap {
        files,
        total_symbols,
        total_lines,
    })
}

/// Format repository map as markdown
///
/// # Arguments
///
/// * `repo_map` - Repository map to format
///
/// # Returns
///
/// Markdown formatted string
pub fn format_repomap_markdown(repo_map: &RepoMap) -> String {
    let mut output = String::new();

    output.push_str("# Repository Map\n\n");
    output.push_str(&format!(
        "**Files**: {} | **Symbols**: {} | **Lines**: {}\n\n",
        repo_map.files.len(),
        repo_map.total_symbols,
        repo_map.total_lines
    ));

    for file in &repo_map.files {
        output.push_str(&format!("## `{}` ({})\n\n", file.path, file.language));

        if file.symbols.is_empty() {
            output.push_str("*No symbols found*\n\n");
            continue;
        }

        for symbol in &file.symbols {
            format_symbol(&mut output, symbol, 0);
        }

        output.push('\n');
    }

    output
}

/// Format a symbol with indentation
#[allow(dead_code)]
fn format_symbol(output: &mut String, symbol: &Symbol, indent: usize) {
    let indent_str = "  ".repeat(indent);
    let icon = symbol.kind.icon();
    let kind = symbol.kind.display_name();

    output.push_str(&format!(
        "{}{} **{}** - {}:{}\n",
        indent_str,
        icon,
        symbol.name,
        kind,
        symbol.start_line
    ));

    for child in &symbol.children {
        format_symbol(output, child, indent + 1);
    }
}

/// Format repository map as JSON
///
/// # Arguments
///
/// * `repo_map` - Repository map to format
///
/// # Returns
///
/// JSON formatted string
///
/// # Errors
///
/// Returns error if serialization fails
#[allow(dead_code)]
pub fn format_repomap_json(repo_map: &RepoMap) -> Result<String> {
    serde_json::to_string_pretty(repo_map)
        .map_err(|e| CodegenError::Other(e.into()))
}

/// Generate compact repository summary
///
/// # Arguments
///
/// * `repo_map` - Repository map to summarize
///
/// # Returns
///
/// Compact summary string
#[allow(dead_code)]
pub fn format_repomap_compact(repo_map: &RepoMap) -> String {
    let mut output = String::new();

    output.push_str(&format!(
        "Repo: {} files, {} symbols, {} lines\n\n",
        repo_map.files.len(),
        repo_map.total_symbols,
        repo_map.total_lines
    ));

    for file in &repo_map.files {
        let symbol_names: Vec<&str> = file.symbols.iter()
            .map(|s| s.name.as_str())
            .collect();

        output.push_str(&format!(
            "{}: {} symbols: {}\n",
            file.path,
            file.symbols.len(),
            symbol_names.join(", ")
        ));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_repomap() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();

        // Create test files
        std::fs::write(
            root.join("test.rs"),
            r#"
pub fn hello() {}
pub struct Foo;
"#,
        ).unwrap();

        std::fs::write(
            root.join("test.py"),
            r#"
def hello():
    pass
"#,
        ).unwrap();

        let repo_map = generate_repomap(root, 10).unwrap();
        assert_eq!(repo_map.files.len(), 2);
        assert_eq!(repo_map.total_symbols, 3); // 2 from Rust, 1 from Python
    }

    #[test]
    fn test_format_repomap_markdown() {
        let repo_map = RepoMap {
            files: vec![FileSummary {
                path: "test.rs".to_string(),
                language: "Rust".to_string(),
                symbols: vec![],
                lines: 10,
            }],
            total_symbols: 0,
            total_lines: 10,
        };

        let markdown = format_repomap_markdown(&repo_map);
        assert!(markdown.contains("# Repository Map"));
        assert!(markdown.contains("test.rs"));
    }

    #[test]
    fn test_format_repomap_compact() {
        let repo_map = RepoMap {
            files: vec![FileSummary {
                path: "test.rs".to_string(),
                language: "Rust".to_string(),
                symbols: vec![],
                lines: 10,
            }],
            total_symbols: 0,
            total_lines: 10,
        };

        let compact = format_repomap_compact(&repo_map);
        assert!(compact.contains("1 files"));
        assert!(compact.contains("test.rs"));
    }
}
