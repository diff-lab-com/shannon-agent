//! Smart context selection — automatically includes relevant files based on user queries.
//!
//! When a user types a query like "fix the bug in src/auth.rs" or "update the login handler",
//! this module detects file paths, code identifiers, and feature keywords, then searches
//! the codebase to include relevant file snippets in the LLM context.

use std::path::Path;

/// Maximum number of files to include in auto-context.
const MAX_CONTEXT_FILES: usize = 5;
/// Maximum lines per file snippet.
const MAX_SNIPPET_LINES: usize = 50;
/// Maximum total characters for auto-context.
const MAX_CONTEXT_CHARS: usize = 8000;

/// Result of smart context analysis for a query.
#[derive(Debug, Clone)]
pub struct SmartContextResult {
    /// File snippets to include in context.
    pub snippets: Vec<FileSnippet>,
    /// How the context was selected.
    pub selection_method: SelectionMethod,
}

/// A file snippet extracted for context.
#[derive(Debug, Clone)]
pub struct FileSnippet {
    /// Relative file path.
    pub path: String,
    /// Lines of content (truncated if needed).
    pub content: String,
    /// Why this file was selected.
    pub reason: String,
}

/// How context was selected.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectionMethod {
    /// Direct file path mentioned in query.
    ExplicitPath,
    /// Code identifier found in files.
    IdentifierSearch,
    /// Keyword-based search.
    KeywordSearch,
    /// No relevant context found.
    None,
}

/// Analyze a user query and find relevant file context.
///
/// Uses three strategies in order:
/// 1. Extract explicit file paths from the query
/// 2. Search for code identifiers (CamelCase, snake_case patterns)
/// 3. Fall back to keyword-based grep
pub fn find_relevant_context(query: &str, working_dir: &Path) -> SmartContextResult {
    // Strategy 1: Explicit file paths
    let explicit = extract_explicit_paths(query, working_dir);
    if !explicit.is_empty() {
        let snippets = read_snippets(&explicit, working_dir);
        if !snippets.is_empty() {
            return SmartContextResult {
                snippets,
                selection_method: SelectionMethod::ExplicitPath,
            };
        }
    }

    // Strategy 2: Code identifiers
    let identifiers = extract_identifiers(query);
    if !identifiers.is_empty() {
        let files = search_identifiers(&identifiers, working_dir);
        if !files.is_empty() {
            let snippets = read_snippets(&files, working_dir);
            if !snippets.is_empty() {
                return SmartContextResult {
                    snippets,
                    selection_method: SelectionMethod::IdentifierSearch,
                };
            }
        }
    }

    // Strategy 3: Keyword search
    let keywords = extract_keywords(query);
    if !keywords.is_empty() {
        let files = search_keywords(&keywords, working_dir);
        if !files.is_empty() {
            let snippets = read_snippets(&files, working_dir);
            if !snippets.is_empty() {
                return SmartContextResult {
                    snippets,
                    selection_method: SelectionMethod::KeywordSearch,
                };
            }
        }
    }

    SmartContextResult {
        snippets: Vec::new(),
        selection_method: SelectionMethod::None,
    }
}

/// Format smart context results as a system prompt section.
pub fn format_context_for_prompt(result: &SmartContextResult) -> Option<String> {
    if result.snippets.is_empty() {
        return None;
    }

    let method = match result.selection_method {
        SelectionMethod::ExplicitPath => "mentioned files",
        SelectionMethod::IdentifierSearch => "found matching identifiers",
        SelectionMethod::KeywordSearch => "keyword search",
        SelectionMethod::None => return None,
    };

    let mut output = format!("## Auto-included Context ({method})\n\n");
    let mut total_chars = 0;

    for snippet in &result.snippets {
        if total_chars + snippet.content.len() > MAX_CONTEXT_CHARS {
            break;
        }
        output.push_str(&format!(
            "### {}\n({})\n```\n{}\n```\n\n",
            snippet.path, snippet.reason, snippet.content
        ));
        total_chars += snippet.content.len();
    }

    Some(output)
}

/// Extract explicit file paths from a query string.
fn extract_explicit_paths(query: &str, working_dir: &Path) -> Vec<String> {
    let mut paths = Vec::new();
    let words = query.split_whitespace();

    for word in words {
        // Strip surrounding punctuation
        let cleaned = word.trim_matches(|c: char| {
            c == '"'
                || c == '\''
                || c == '`'
                || c == ','
                || c == '.'
                || c == '('
                || c == ')'
                || c == ';'
        });

        // Check if it looks like a file path
        if looks_like_file_path(cleaned) {
            let full_path = working_dir.join(cleaned);
            if full_path.exists() {
                paths.push(cleaned.to_string());
            }
        }
    }

    paths.dedup();
    paths.truncate(MAX_CONTEXT_FILES);
    paths
}

/// Check if a string looks like a file path.
fn looks_like_file_path(s: &str) -> bool {
    if s.is_empty() || s.len() < 3 {
        return false;
    }

    // Must contain a dot (extension) or slash
    let has_dot = s.contains('.') && !s.starts_with('.');
    let has_slash = s.contains('/');
    let has_backslash = s.contains('\\');

    if !has_dot && !has_slash && !has_backslash {
        return false;
    }

    // Skip URLs and common non-path patterns
    if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("git@") {
        return false;
    }

    // Skip flags and options
    if s.starts_with('-') || s.starts_with('+') {
        return false;
    }

    // Must have a valid-looking extension
    if let Some(ext) = s.rsplit('.').next() {
        // Common source code extensions
        let valid_extensions = [
            "rs",
            "toml",
            "json",
            "yaml",
            "yml",
            "md",
            "txt",
            "py",
            "js",
            "ts",
            "tsx",
            "jsx",
            "go",
            "java",
            "c",
            "h",
            "cpp",
            "rb",
            "sh",
            "bash",
            "sql",
            "html",
            "css",
            "scss",
            "cfg",
            "ini",
            "xml",
            "proto",
            "dockerfile",
        ];
        if !valid_extensions.contains(&ext) {
            return false;
        }
    }

    true
}

/// Extract code identifiers from a query (CamelCase, snake_case with underscores).
fn extract_identifiers(query: &str) -> Vec<String> {
    let mut identifiers = Vec::new();

    for word in query.split_whitespace() {
        let cleaned = word.trim_matches(|c: char| {
            c == '"'
                || c == '\''
                || c == '`'
                || c == ','
                || c == '.'
                || c == '('
                || c == ')'
                || c == ';'
                || c == ':'
                || c == '!'
        });

        // Skip common English words and short words
        if cleaned.len() < 3 {
            continue;
        }

        let common_words = [
            "the", "and", "for", "but", "not", "you", "all", "can", "had", "her", "was", "one",
            "our", "out", "are", "has", "been", "have", "make", "like", "just", "over", "such",
            "take", "than", "them", "very", "what", "when", "where", "which", "this", "that",
            "with", "from", "they", "will", "would", "could", "should", "about", "into", "then",
            "also", "some", "more", "want", "need", "does", "help", "please", "code", "file",
            "files", "function", "class", "method", "variable", "module", "change", "fix", "add",
            "remove", "update", "create", "delete", "check", "show", "get", "set",
        ];

        if common_words.contains(&cleaned.to_lowercase().as_str()) {
            continue;
        }

        // Identifier patterns: CamelCase or snake_case
        let has_camel =
            cleaned.chars().any(|c| c.is_uppercase()) && cleaned.chars().any(|c| c.is_lowercase());
        let has_underscore = cleaned.contains('_');
        let is_long_enough = cleaned.len() >= 4;

        if (has_camel || has_underscore) && is_long_enough {
            identifiers.push(cleaned.to_string());
        }
    }

    identifiers.dedup();
    identifiers.truncate(5);
    identifiers
}

/// Extract meaningful keywords from a query for grep search.
fn extract_keywords(query: &str) -> Vec<String> {
    let mut keywords = Vec::new();

    for word in query.split_whitespace() {
        let cleaned = word.trim_matches(|c: char| c.is_ascii_punctuation());
        let lower = cleaned.to_lowercase();

        if lower.len() < 4 {
            continue;
        }

        let stop_words = [
            "the", "and", "for", "but", "not", "you", "all", "can", "this", "that", "with", "from",
            "they", "will", "would", "could", "should", "about", "into", "then", "also", "some",
            "more", "want", "need", "help", "please", "just", "like", "have", "been", "does",
            "what", "when", "where", "which", "there", "their", "than", "make",
        ];

        if stop_words.contains(&lower.as_str()) {
            continue;
        }

        keywords.push(lower);
    }

    keywords.dedup();
    keywords.truncate(3);
    keywords
}

/// Search for files containing identifiers using grep.
fn search_identifiers(identifiers: &[String], working_dir: &Path) -> Vec<String> {
    let mut results = Vec::new();

    for ident in identifiers {
        let output = std::process::Command::new("grep")
            .args([
                "-rl",
                "-F",
                "--include=*.rs",
                "--include=*.py",
                "--include=*.ts",
                "--include=*.js",
                "--include=*.go",
                "--include=*.java",
                "--include=*.toml",
                ident,
                ".",
            ])
            .current_dir(working_dir)
            .output();

        if let Ok(out) = output {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                for line in stdout.lines().take(3) {
                    let path = line.trim().to_string();
                    if !path.is_empty() && !results.contains(&path) {
                        results.push(path);
                    }
                }
            }
        }
    }

    results.truncate(MAX_CONTEXT_FILES);
    results
}

/// Search for files containing keywords.
fn search_keywords(keywords: &[String], working_dir: &Path) -> Vec<String> {
    // Build fixed-string patterns with multiple -e flags to avoid regex injection
    let mut base_args: Vec<String> = vec![
        "-rl".to_string(),
        "-F".to_string(),
        "--include=*.rs".to_string(),
        "--include=*.py".to_string(),
        "--include=*.ts".to_string(),
        "--include=*.js".to_string(),
        "--include=*.toml".to_string(),
    ];
    for kw in keywords {
        base_args.push("-e".to_string());
        base_args.push(kw.clone());
    }
    base_args.push(".".to_string());

    let output = std::process::Command::new("grep")
        .args(&base_args)
        .current_dir(working_dir)
        .output();

    let mut results = Vec::new();
    if let Ok(out) = output {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines().take(MAX_CONTEXT_FILES) {
                let path = line.trim().to_string();
                if !path.is_empty() {
                    results.push(path);
                }
            }
        }
    }

    results
}

/// Read file snippets for a list of paths.
fn read_snippets(paths: &[String], working_dir: &Path) -> Vec<FileSnippet> {
    let mut snippets = Vec::new();

    for path_str in paths {
        let full_path = working_dir.join(path_str);
        match std::fs::read_to_string(&full_path) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let snippet_content = if lines.len() > MAX_SNIPPET_LINES {
                    let truncated: Vec<&str> =
                        lines.iter().take(MAX_SNIPPET_LINES).copied().collect();
                    format!(
                        "{}\n... ({} more lines)",
                        truncated.join("\n"),
                        lines.len() - MAX_SNIPPET_LINES
                    )
                } else {
                    content.clone()
                };

                snippets.push(FileSnippet {
                    path: path_str.clone(),
                    content: snippet_content,
                    reason: "matched query".to_string(),
                });
            }
            Err(_) => continue,
        }

        if snippets.len() >= MAX_CONTEXT_FILES {
            break;
        }
    }

    snippets
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_looks_like_file_path_valid() {
        assert!(looks_like_file_path("src/auth.rs"));
        assert!(looks_like_file_path("Cargo.toml"));
        assert!(looks_like_file_path("lib/python/file.py"));
        assert!(looks_like_file_path("config.yaml"));
    }

    #[test]
    fn test_looks_like_file_path_invalid() {
        assert!(!looks_like_file_path("hello"));
        assert!(!looks_like_file_path("--flag"));
        assert!(!looks_like_file_path("https://example.com"));
        assert!(!looks_like_file_path("git@github.com"));
        assert!(!looks_like_file_path(""));
        assert!(!looks_like_file_path("ab"));
    }

    #[test]
    fn test_extract_identifiers() {
        let ids = extract_identifiers("fix the handle_login bug in auth_service");
        assert!(ids.contains(&"handle_login".to_string()));
        assert!(ids.contains(&"auth_service".to_string()));
    }

    #[test]
    fn test_extract_identifiers_camelcase() {
        let ids = extract_identifiers("update QueryEngine to use StreamingHandler");
        assert!(ids.contains(&"QueryEngine".to_string()));
        assert!(ids.contains(&"StreamingHandler".to_string()));
    }

    #[test]
    fn test_extract_keywords_filters_stopwords() {
        let kws = extract_keywords("fix the bug in the auth module");
        assert!(!kws.contains(&"the".to_string()));
        assert!(kws.contains(&"auth".to_string()));
    }

    #[test]
    fn test_format_context_empty() {
        let result = SmartContextResult {
            snippets: Vec::new(),
            selection_method: SelectionMethod::None,
        };
        assert!(format_context_for_prompt(&result).is_none());
    }

    #[test]
    fn test_format_context_with_snippets() {
        let result = SmartContextResult {
            snippets: vec![FileSnippet {
                path: "src/main.rs".to_string(),
                content: "fn main() {}".to_string(),
                reason: "mentioned in query".to_string(),
            }],
            selection_method: SelectionMethod::ExplicitPath,
        };
        let formatted = format_context_for_prompt(&result).unwrap();
        assert!(formatted.contains("src/main.rs"));
        assert!(formatted.contains("fn main()"));
        assert!(formatted.contains("mentioned files"));
    }

    #[test]
    fn test_extract_explicit_paths_with_real_files() {
        let dir = std::env::current_dir().unwrap();
        // This test runs in the shannon-code repo, so Cargo.toml should exist
        let paths = extract_explicit_paths("check Cargo.toml and src/main.rs", &dir);
        assert!(
            paths.contains(&"Cargo.toml".to_string()),
            "Should find Cargo.toml"
        );
    }

    #[test]
    fn test_smart_context_deduplication() {
        let dir = std::env::current_dir().unwrap();
        let paths = extract_explicit_paths("fix Cargo.toml and Cargo.toml", &dir);
        let count = paths.iter().filter(|p| *p == "Cargo.toml").count();
        assert_eq!(count, 1, "Should deduplicate paths");
    }

    #[test]
    fn test_find_relevant_context_with_explicit_path() {
        let dir = std::env::current_dir().unwrap();
        let result = find_relevant_context("show me the contents of Cargo.toml", &dir);
        assert_eq!(result.selection_method, SelectionMethod::ExplicitPath);
        assert!(!result.snippets.is_empty());
        assert!(result.snippets[0].path.contains("Cargo.toml"));
    }

    #[test]
    fn test_snippet_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let long_file = dir.path().join("long.rs");
        let content = (0..100)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&long_file, &content).unwrap();

        let snippets = read_snippets(&["long.rs".to_string()], dir.path());
        assert_eq!(snippets.len(), 1);
        assert!(snippets[0].content.contains("... (50 more lines)"));
    }
}
