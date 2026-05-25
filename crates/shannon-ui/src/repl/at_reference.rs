//! @ Reference Enhancement — extended processing for the @ file picker.
//!
//! When a user selects a file, directory, or URL via the `@` picker:
//! - **PDF files**: text is extracted via `pdftotext` and injected as a code block.
//! - **URLs** (`@https://...` / `@http://...`): content is fetched and converted to text.
//! - **Directories**: a tree listing is generated with configurable depth.
//! - **Regular files**: content is read and injected as a code block with language detection.

use std::path::Path;

/// Default maximum depth for directory tree listings.
const DEFAULT_TREE_DEPTH: usize = 3;

/// Maximum file content to inject (50 KiB, same as PDF limit).
const FILE_CONTENT_LIMIT: usize = 50 * 1024;

/// Directories that are always skipped when generating a tree listing.
const IGNORED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    "__pycache__",
    ".svn",
    ".hg",
    "vendor",
    ".idea",
    ".vscode",
    ".cache",
    "dist",
    "build",
    ".next",
    ".nuxt",
    ".turbo",
    ".vercel",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    "venv",
    ".venv",
    "env",
    ".env",
    ".direnv",
];

// ── Public types ──────────────────────────────────────────────────────

/// The kind of @ reference that was selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtReferenceKind {
    /// A regular file — extract content and inject it as a code block.
    File,
    /// A PDF file — extract text and inject it.
    Pdf,
    /// A URL — fetch and inject the content.
    Url(String),
    /// A directory — generate a tree listing.
    Directory,
}

/// Result of processing an @ reference.
#[derive(Debug, Clone)]
pub struct AtReferenceResult {
    /// The text to inject into the prompt (replacing the trailing `@`).
    pub injected_text: String,
    /// Optional message to show in the chat as a system notification.
    pub status_message: Option<String>,
    /// Whether an error occurred during processing.
    pub is_error: bool,
}

impl AtReferenceResult {
    /// Create a successful result with the given injected text.
    #[allow(dead_code)] // KEEP: future use
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            injected_text: text.into(),
            status_message: None,
            is_error: false,
        }
    }

    /// Create a successful result with injected text and a status message.
    pub fn with_status(text: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            injected_text: text.into(),
            status_message: Some(msg.into()),
            is_error: false,
        }
    }

    /// Create an error result.
    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            injected_text: String::new(),
            status_message: Some(msg.into()),
            is_error: true,
        }
    }
}

// ── Detection ─────────────────────────────────────────────────────────

/// Detect the kind of @ reference from the selected path or URL.
pub fn detect_reference_kind(path: &str) -> AtReferenceKind {
    // URL detection: @https://... or @http://...
    if path.starts_with("http://") || path.starts_with("https://") {
        return AtReferenceKind::Url(path.to_string());
    }

    // Directory detection
    if Path::new(path).is_dir() {
        return AtReferenceKind::Directory;
    }

    // PDF detection by extension
    if path
        .rsplit('.')
        .next()
        .map(|e| e.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
    {
        return AtReferenceKind::Pdf;
    }

    AtReferenceKind::File
}

/// Detect whether an input string typed after `@` is a URL.
/// Returns `Some(url)` if the text starts with `http://` or `https://`.
#[allow(dead_code)] // KEEP: future use
pub fn detect_url_in_input(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        Some(trimmed.to_string())
    } else {
        None
    }
}

// ── PDF processing ────────────────────────────────────────────────────

/// Extract text from a PDF file using `pdftotext` (from poppler-utils).
///
/// Returns a formatted string ready to inject into the message, including
/// the filename and page count metadata.
pub fn extract_pdf_text(file_path: &str) -> AtReferenceResult {
    let path = Path::new(file_path);

    if !path.exists() {
        return AtReferenceResult::error(format!("PDF file not found: {file_path}"));
    }

    // Get page count via pdfinfo
    let page_count = get_pdf_page_count(file_path);

    // Extract text via pdftotext
    let text = match extract_pdf_raw_text(file_path) {
        Ok(t) => t,
        Err(e) => {
            return AtReferenceResult::error(format!(
                "Failed to extract PDF text: {e}. \
                 Install poppler-utils (`apt install poppler-utils` or `brew install poppler`)."
            ));
        }
    };

    if text.trim().is_empty() {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| file_path.to_string());
        let page_info = page_count
            .map(|n| format!(" ({n} pages)"))
            .unwrap_or_default();
        return AtReferenceResult::with_status(
            format!("@{file_path}"),
            format!(
                "PDF \"{file_name}\"{page_info} appears to contain no extractable text. \
                 It may be a scanned document requiring OCR."
            ),
        );
    }

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.to_string());
    let page_info = page_count
        .map(|n| format!(" ({n} pages)"))
        .unwrap_or_default();

    // Truncate very large PDFs to avoid overwhelming the context
    let max_bytes = 50_000;
    let (content, truncated) = if text.len() > max_bytes {
        let mut end = max_bytes;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        (text[..end].to_string(), true)
    } else {
        (text, false)
    };

    let mut injected = format!("**PDF: {file_name}{page_info}**\n\n```\n{content}\n```");
    if truncated {
        injected.push_str("\n\n*[Content truncated — PDF is too large to include in full]*");
    }

    AtReferenceResult::with_status(injected, format!("Attached PDF \"{file_name}\"{page_info}"))
}

/// Get the page count from a PDF using `pdfinfo`.
fn get_pdf_page_count(file_path: &str) -> Result<usize, String> {
    let output = std::process::Command::new("pdfinfo")
        .arg(file_path)
        .output()
        .map_err(|e| format!("Failed to run pdfinfo: {e}"))?;

    if !output.status.success() {
        return Err("pdfinfo returned non-zero exit code".to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("Pages:") {
            if let Ok(count) = rest.trim().parse::<usize>() {
                return Ok(count);
            }
        }
    }

    Err("Could not parse page count from pdfinfo output".to_string())
}

/// Extract raw text from a PDF using `pdftotext -layout`.
fn extract_pdf_raw_text(file_path: &str) -> Result<String, String> {
    let output = std::process::Command::new("pdftotext")
        .args(["-layout", file_path, "-"])
        .output()
        .map_err(|e| format!("Failed to run pdftotext: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("pdftotext failed: {stderr}"));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ── URL fetching ──────────────────────────────────────────────────────

/// Fetch content from a URL and format it for injection into a message.
///
/// Uses `reqwest` async client via tokio runtime block_on, and the
/// `strip_html_tags` function from `shannon_tools::web`.
pub fn fetch_url_content(url: &str) -> AtReferenceResult {
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => {
            // No tokio runtime available — create a temporary one
            match tokio::runtime::Runtime::new() {
                Ok(rt) => {
                    // We'll use block_on below with this runtime
                    return fetch_url_content_with_rt(url, &rt);
                }
                Err(e) => {
                    return AtReferenceResult::error(format!(
                        "Failed to create tokio runtime for URL fetch: {e}"
                    ));
                }
            }
        }
    };
    fetch_url_content_with_handle(url, &rt)
}

fn fetch_url_content_with_rt(url: &str, rt: &tokio::runtime::Runtime) -> AtReferenceResult {
    let client = reqwest::Client::builder()
        .user_agent("ShannonCode/1.0")
        .timeout(std::time::Duration::from_secs(15))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(e) => return AtReferenceResult::error(format!("Failed to create HTTP client: {e}")),
    };

    let response = match rt.block_on(client.get(url).send()) {
        Ok(r) => r,
        Err(e) => {
            return AtReferenceResult::error(format!(
                "Failed to fetch URL: {e}. \
                 Check that the URL is correct and reachable."
            ));
        }
    };

    rt.block_on(process_url_response(url, response))
}

fn fetch_url_content_with_handle(url: &str, handle: &tokio::runtime::Handle) -> AtReferenceResult {
    let client = reqwest::Client::builder()
        .user_agent("ShannonCode/1.0")
        .timeout(std::time::Duration::from_secs(15))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(e) => return AtReferenceResult::error(format!("Failed to create HTTP client: {e}")),
    };

    // Use block_in_place to avoid blocking the async runtime
    let fut = async move {
        let response = match client.get(url).send().await {
            Ok(r) => r,
            Err(e) => {
                return AtReferenceResult::error(format!(
                    "Failed to fetch URL: {e}. \
                     Check that the URL is correct and reachable."
                ));
            }
        };
        process_url_response(url, response).await
    };

    tokio::task::block_in_place(|| handle.block_on(fut))
}

async fn process_url_response(url: &str, response: reqwest::Response) -> AtReferenceResult {
    let status = response.status();
    if !status.is_success() {
        return AtReferenceResult::error(format!("HTTP error fetching URL: {status}"));
    }

    let body = match response.text().await {
        Ok(t) => t,
        Err(e) => return AtReferenceResult::error(format!("Failed to read response body: {e}")),
    };

    // Convert HTML to plain text using the same logic as WebFetchTool
    let content = if looks_like_html(&body) {
        shannon_tools::web::strip_html_tags(&body)
    } else {
        body
    };

    if content.trim().is_empty() {
        return AtReferenceResult::error(format!("URL {url} returned empty content."));
    }

    let content_length = content.len();
    let max_bytes = 50_000;
    let (display_content, truncated) = if content.len() > max_bytes {
        let mut end = max_bytes;
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        (content[..end].to_string(), true)
    } else {
        (content, false)
    };

    let mut injected = format!(
        "**URL: {url}**\n**Content length: {content_length} characters**\n\n{display_content}"
    );
    if truncated {
        injected.push_str("\n\n*[Content truncated — page is too large to include in full]*");
    }

    AtReferenceResult::with_status(
        injected,
        format!("Fetched content from {url} ({content_length} chars)"),
    )
}

/// Heuristic to check if the body looks like HTML.
fn looks_like_html(body: &str) -> bool {
    let lower = body.trim_start().to_ascii_lowercase();
    lower.starts_with("<!doctype html")
        || lower.starts_with("<html")
        || lower.starts_with("<head")
        || (lower.contains('<') && lower.contains("</") && lower.contains('>'))
}

// ── Directory tree ────────────────────────────────────────────────────

/// Generate a directory tree listing for the given path.
///
/// Respects `.gitignore` conventions for common directories and limits
/// depth to `max_depth`. Returns a formatted tree string using Unicode
/// box-drawing characters.
pub fn generate_directory_tree(dir_path: &str, max_depth: Option<usize>) -> AtReferenceResult {
    let max_depth = max_depth.unwrap_or(DEFAULT_TREE_DEPTH);
    let path = Path::new(dir_path);

    if !path.exists() {
        return AtReferenceResult::error(format!("Directory not found: {dir_path}"));
    }

    if !path.is_dir() {
        return AtReferenceResult::error(format!("Path is not a directory: {dir_path}"));
    }

    let dir_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| dir_path.to_string());

    let mut tree = format!("{dir_name}/\n");
    let mut total_files: usize = 0;
    let mut total_dirs: usize = 0;

    build_tree_recursive(
        path,
        &mut tree,
        "",
        true,
        0,
        max_depth,
        &mut total_files,
        &mut total_dirs,
    );

    let injected = format!(
        "**Directory: {dir_name}**\n**{total_files} files, {total_dirs} subdirectories**\n\n```\n{tree}\n```"
    );

    AtReferenceResult::with_status(
        injected,
        format!(
            "Attached directory tree for \"{dir_name}\" ({total_files} files, {total_dirs} dirs, max depth {max_depth})"
        ),
    )
}

/// Recursively build the tree listing.
#[allow(clippy::too_many_arguments)]
fn build_tree_recursive(
    dir: &Path,
    output: &mut String,
    prefix: &str,
    is_last: bool,
    current_depth: usize,
    max_depth: usize,
    total_files: &mut usize,
    total_dirs: &mut usize,
) {
    if current_depth >= max_depth {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            output.push_str(&format!(
                "{prefix}{}[Error reading directory: {e}]\n",
                if is_last { "└── " } else { "├── " },
            ));
            return;
        }
    };

    // Collect and sort entries: directories first, then files
    let mut dirs = Vec::new();
    let mut files = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files/dirs (start with .) and ignored patterns
        if name.starts_with('.') {
            continue;
        }

        if entry.path().is_dir() {
            if !IGNORED_DIRS.contains(&name.as_str()) {
                dirs.push(name);
                *total_dirs += 1;
            }
        } else {
            files.push(name);
            *total_files += 1;
        }
    }

    dirs.sort();
    files.sort();

    let all_entries: Vec<(String, bool)> = dirs
        .iter()
        .map(|d| (d.clone(), true))
        .chain(files.iter().map(|f| (f.clone(), false)))
        .collect();

    let count = all_entries.len();
    for (i, (name, is_dir)) in all_entries.iter().enumerate() {
        let is_last_entry = i == count - 1;
        let connector = if is_last_entry {
            "└── "
        } else {
            "├── "
        };
        let suffix = if *is_dir { "/" } else { "" };

        // Get file size for files
        let size_info = if !is_dir {
            let full_path = dir.join(name);
            full_path
                .metadata()
                .ok()
                .map(|m| format_file_size(m.len()))
                .map(|s| format!("  ({s})"))
                .unwrap_or_default()
        } else {
            String::new()
        };

        output.push_str(&format!("{prefix}{connector}{name}{suffix}{size_info}\n"));

        // Recurse into subdirectories
        if *is_dir {
            let new_prefix = if is_last_entry {
                format!("{prefix}    ")
            } else {
                format!("{prefix}│   ")
            };
            build_tree_recursive(
                &dir.join(name),
                output,
                &new_prefix,
                is_last_entry,
                current_depth + 1,
                max_depth,
                total_files,
                total_dirs,
            );
        }
    }
}

/// Format a file size in human-readable form.
fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;

    if bytes < KB {
        format!("{bytes}B")
    } else if bytes < MB {
        format!("{:.1}KB", bytes as f64 / KB as f64)
    } else {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    }
}

// ── File content extraction ────────────────────────────────────────────

/// Extract the content of a regular file and format it for injection.
///
/// Reads the file, detects language from its extension, truncates if
/// larger than `FILE_CONTENT_LIMIT`, and returns a fenced code block.
pub fn extract_file_content(file_path: &str) -> AtReferenceResult {
    let path = Path::new(file_path);

    if !path.exists() {
        return AtReferenceResult::error(format!("File not found: {file_path}"));
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            // Binary or non-UTF-8 file — just insert the path
            return AtReferenceResult::with_status(
                format!("@{file_path}"),
                format!("Could not read file as text: {e}. Path inserted instead."),
            );
        }
    };

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.to_string());

    let lang = detect_language(path);
    let content_len = content.len();

    if content_len > FILE_CONTENT_LIMIT {
        let mut end = FILE_CONTENT_LIMIT;
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        let truncated = &content[..end];
        AtReferenceResult::with_status(
            format!(
                "**File: {file_name}**\n```{lang}\n{truncated}\n```\n*[Truncated — showing first {FILE_CONTENT_LIMIT} of {content_len} bytes]*"
            ),
            format!("Attached \"{file_name}\" (truncated, {content_len} bytes)"),
        )
    } else {
        AtReferenceResult::with_status(
            format!("**File: {file_name}**\n```{lang}\n{content}\n```"),
            format!("Attached \"{file_name}\" ({content_len} bytes)"),
        )
    }
}

/// Detect the language identifier for a fenced code block from the file extension.
fn detect_language(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "rust",
        "py" => "python",
        "js" => "javascript",
        "ts" => "typescript",
        "tsx" => "tsx",
        "jsx" => "jsx",
        "go" => "go",
        "java" => "java",
        "rb" => "ruby",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        "md" => "markdown",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" => "scss",
        "sh" | "bash" => "bash",
        "zsh" => "zsh",
        "sql" => "sql",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "kt" => "kotlin",
        "swift" => "swift",
        "lua" => "lua",
        "r" => "r",
        "xml" => "xml",
        "proto" => "protobuf",
        "dockerfile" => "dockerfile",
        _ => "",
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── detect_reference_kind tests ────────────────────────────────

    #[test]
    fn test_detect_pdf() {
        assert_eq!(detect_reference_kind("report.pdf"), AtReferenceKind::Pdf);
        assert_eq!(detect_reference_kind("PDF"), AtReferenceKind::Pdf);
        assert_eq!(detect_reference_kind("doc.PDF"), AtReferenceKind::Pdf);
    }

    #[test]
    fn test_detect_file() {
        assert_eq!(detect_reference_kind("main.rs"), AtReferenceKind::File);
        assert_eq!(detect_reference_kind("config.toml"), AtReferenceKind::File);
        assert_eq!(detect_reference_kind("image.png"), AtReferenceKind::File);
    }

    #[test]
    fn test_detect_url() {
        assert_eq!(
            detect_reference_kind("https://example.com/page"),
            AtReferenceKind::Url("https://example.com/page".to_string())
        );
        assert_eq!(
            detect_reference_kind("http://foo.bar/api"),
            AtReferenceKind::Url("http://foo.bar/api".to_string())
        );
    }

    // ── detect_url_in_input tests ──────────────────────────────────

    #[test]
    fn test_detect_url_in_input_valid() {
        assert_eq!(
            detect_url_in_input("https://example.com"),
            Some("https://example.com".to_string())
        );
        assert_eq!(
            detect_url_in_input("  http://foo.bar  "),
            Some("http://foo.bar".to_string())
        );
    }

    #[test]
    fn test_detect_url_in_input_invalid() {
        assert_eq!(detect_url_in_input("not a url"), None);
        assert_eq!(detect_url_in_input("ftp://example.com"), None);
        assert_eq!(detect_url_in_input(""), None);
    }

    // ── AtReferenceResult tests ────────────────────────────────────

    #[test]
    fn test_result_ok() {
        let r = AtReferenceResult::ok("hello");
        assert_eq!(r.injected_text, "hello");
        assert!(r.status_message.is_none());
        assert!(!r.is_error);
    }

    #[test]
    fn test_result_with_status() {
        let r = AtReferenceResult::with_status("text", "status");
        assert_eq!(r.injected_text, "text");
        assert_eq!(r.status_message, Some("status".to_string()));
        assert!(!r.is_error);
    }

    #[test]
    fn test_result_error() {
        let r = AtReferenceResult::error("bad thing");
        assert!(r.injected_text.is_empty());
        assert_eq!(r.status_message, Some("bad thing".to_string()));
        assert!(r.is_error);
    }

    // ── looks_like_html tests ──────────────────────────────────────

    #[test]
    fn test_looks_like_html_doctype() {
        assert!(looks_like_html("<!DOCTYPE html><html></html>"));
    }

    #[test]
    fn test_looks_like_html_tag() {
        assert!(looks_like_html("<html><body>Hello</body></html>"));
    }

    #[test]
    fn test_looks_like_html_plain_text() {
        assert!(!looks_like_html("Just some plain text here"));
    }

    #[test]
    fn test_looks_like_html_json() {
        // JSON with < in strings should not trigger false positive
        // because it lacks both </ and >
        assert!(!looks_like_html("{\"key\": \"value\"}"));
    }

    // ── format_file_size tests ─────────────────────────────────────

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_file_size(0), "0B");
        assert_eq!(format_file_size(512), "512B");
    }

    #[test]
    fn test_format_kilobytes() {
        assert_eq!(format_file_size(1024), "1.0KB");
        assert_eq!(format_file_size(1536), "1.5KB");
    }

    #[test]
    fn test_format_megabytes() {
        assert_eq!(format_file_size(1024 * 1024), "1.0MB");
        assert_eq!(format_file_size(2 * 1024 * 1024 + 512 * 1024), "2.5MB");
    }

    // ── Directory tree tests ───────────────────────────────────────

    #[test]
    fn test_generate_tree_nonexistent_dir() {
        let result = generate_directory_tree("/nonexistent/path/xyz", None);
        assert!(result.is_error);
        assert!(result.status_message.unwrap().contains("not found"));
    }

    #[test]
    fn test_generate_tree_file_not_dir() {
        let result = generate_directory_tree("/etc/hostname", None);
        assert!(result.is_error);
        assert!(result.status_message.unwrap().contains("not a directory"));
    }

    #[test]
    fn test_generate_tree_actual_dir() {
        // Use a directory that should exist on any Linux system
        let result = generate_directory_tree("/tmp", Some(1));
        assert!(!result.is_error);
        assert!(result.injected_text.contains("Directory:"));
        assert!(result.status_message.is_some());
    }

    // ── PDF tests ──────────────────────────────────────────────────

    #[test]
    fn test_extract_pdf_nonexistent() {
        let result = extract_pdf_text("/nonexistent/file.pdf");
        assert!(result.is_error);
        assert!(result.status_message.unwrap().contains("not found"));
    }

    // ── URL fetching tests ─────────────────────────────────────────

    #[test]
    fn test_fetch_invalid_url() {
        // This should fail because the URL is not reachable.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let result = fetch_url_content_with_rt("http://0.0.0.0:1/impossible", &rt);
        assert!(result.is_error);
    }

    #[test]
    fn test_fetch_bad_status() {
        // Use a URL that will return a 404
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let result = fetch_url_content_with_rt("https://httpbin.org/status/404", &rt);
        assert!(result.is_error);
        assert!(result.status_message.unwrap().contains("HTTP error"));
    }

    // ── Ignored dirs test ──────────────────────────────────────────

    #[test]
    fn test_ignored_dirs_contains_common_entries() {
        assert!(IGNORED_DIRS.contains(&"node_modules"));
        assert!(IGNORED_DIRS.contains(&"target"));
        assert!(IGNORED_DIRS.contains(&".git"));
        assert!(IGNORED_DIRS.contains(&"__pycache__"));
        assert!(IGNORED_DIRS.contains(&"venv"));
        assert!(IGNORED_DIRS.contains(&".venv"));
    }

    // ── File content extraction tests ──────────────────────────────

    #[test]
    fn test_extract_file_nonexistent() {
        let result = extract_file_content("/nonexistent/file.rs");
        assert!(result.is_error);
        assert!(result.status_message.unwrap().contains("not found"));
    }

    #[test]
    fn test_extract_file_actual() {
        // Use Cargo.toml which always exists in the project
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let path = format!("{manifest_dir}/Cargo.toml");
        let result = extract_file_content(&path);
        assert!(
            !result.is_error,
            "Expected success, got error: {:?}",
            result.status_message
        );
        assert!(result.injected_text.contains("```toml"));
        assert!(result.status_message.is_some());
    }

    // ── detect_language tests ──────────────────────────────────────

    #[test]
    fn test_detect_language_rust() {
        assert_eq!(detect_language(Path::new("main.rs")), "rust");
    }

    #[test]
    fn test_detect_language_python() {
        assert_eq!(detect_language(Path::new("app.py")), "python");
    }

    #[test]
    fn test_detect_language_unknown() {
        assert_eq!(detect_language(Path::new("Makefile")), "");
    }

    #[test]
    fn test_detect_language_case_insensitive() {
        assert_eq!(detect_language(Path::new("app.PY")), "python");
        assert_eq!(detect_language(Path::new("lib.RS")), "rust");
    }
}
