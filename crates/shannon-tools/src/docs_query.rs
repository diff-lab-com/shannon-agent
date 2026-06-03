//! Documentation query tool
//!
//! Provides a built-in tool that resolves library/package names and fetches
//! official documentation, similar to Context7. The LLM can use this tool to
//! get up-to-date documentation for libraries and frameworks during a session.
//!
//! ## How it works
//!
//! 1. The caller supplies a `library` name (e.g. `"react"`) and an optional
//!    `query` describing what they want to know.
//! 2. The tool resolves the library to the best documentation source:
//!    - Rust crates: docs.rs + crates.io README
//!    - JavaScript/TypeScript: npmjs.com README
//!    - Python: PyPI project description
//!    - Other: falls back to a generic registry lookup
//! 3. It fetches the content, strips HTML, and returns a formatted result.
//!
//! Results are cached in-memory for the lifetime of the tool so repeated
//! queries for the same library don't hit the network again.

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Input / Output types
// ---------------------------------------------------------------------------

/// Input parameters for the DocsQuery tool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DocsQueryInput {
    /// Library or package name to look up (e.g. "react", "serde", "numpy").
    pub library: String,

    /// Optional specific question about the library (e.g. "useEffect cleanup").
    #[serde(default)]
    pub query: Option<String>,

    /// Optional version constraint (e.g. "18", "1.0"). Currently informational
    /// only -- the tool always fetches the latest docs.
    #[serde(default)]
    pub version: Option<String>,
}

/// A single resolved documentation source.
#[derive(Debug, Clone, Serialize)]
pub struct DocsSource {
    /// Human-readable name of the source (e.g. "docs.rs", "npm").
    pub source: String,
    /// URL that was fetched.
    pub url: String,
    /// Extracted documentation content (plain text).
    pub content: String,
}

/// Output produced by the DocsQuery tool.
#[derive(Debug, Clone, Serialize)]
pub struct DocsQueryOutput {
    /// The library that was resolved.
    pub library: String,
    /// Resolved version (if available from the registry).
    pub version: Option<String>,
    /// Documentation sources that were consulted.
    pub sources: Vec<DocsSource>,
    /// Whether the result came from the in-memory cache.
    pub cached: bool,
}

// ---------------------------------------------------------------------------
// Language / registry detection
// ---------------------------------------------------------------------------

/// Detected ecosystem for a library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ecosystem {
    Rust,
    JavaScript,
    Python,
    Unknown,
}

/// Detect which ecosystem a library belongs to based on name heuristics.
///
/// This is intentionally simple -- the MVP does not need a perfect classifier.
/// Unknown libraries fall through to a generic lookup.
fn detect_ecosystem(library: &str) -> Ecosystem {
    let lower = library.to_lowercase();

    // Well-known Rust crates
    let rust_crates = [
        "serde",
        "tokio",
        "reqwest",
        "clap",
        "anyhow",
        "thiserror",
        "tracing",
        "ratatui",
        "cargo",
        "rustls",
        "hyper",
        "actix",
        "axum",
        "rocket",
        "warp",
        "bevy",
        "egui",
        "rayon",
        "crossbeam",
        "parking_lot",
        "chrono",
        "uuid",
        "regex",
        "rand",
        "log",
        "env_logger",
        "futures",
        "async-trait",
        "once_cell",
        "lazy_static",
        "indexmap",
        "hashbrown",
        "dashmap",
        "smallvec",
        "either",
        "itertools",
        "nom",
        "winnow",
        "syn",
        "quote",
        "proc-macro2",
        "heck",
        "toml",
        "serde_json",
        "serde_yaml",
        "url",
        "base64",
        "hex",
        "sha2",
    ];
    if rust_crates.contains(&lower.as_str()) {
        return Ecosystem::Rust;
    }

    // Common JS/TS packages
    let js_packages = [
        "react",
        "react-dom",
        "vue",
        "angular",
        "next",
        "nextjs",
        "nuxt",
        "svelte",
        "express",
        "koa",
        "fastify",
        "typescript",
        "webpack",
        "vite",
        "esbuild",
        "rollup",
        "jest",
        "vitest",
        "mocha",
        "eslint",
        "prettier",
        "axios",
        "lodash",
        "underscore",
        "moment",
        "dayjs",
        "tailwindcss",
        "postcss",
        "sass",
        "prisma",
        "zod",
        "yup",
        "joi",
        "redux",
        "zustand",
        "pinia",
        "three",
        "d3",
        "chartjs",
        "ws",
        "chalk",
        "commander",
        "inquirer",
        "playwright",
        "puppeteer",
        "electron",
    ];
    if js_packages.contains(&lower.as_str()) {
        return Ecosystem::JavaScript;
    }

    // Common Python packages
    let py_packages = [
        "numpy",
        "pandas",
        "scipy",
        "matplotlib",
        "scikit-learn",
        "sklearn",
        "tensorflow",
        "torch",
        "pytorch",
        "flask",
        "django",
        "fastapi",
        "requests",
        "beautifulsoup4",
        "bs4",
        "selenium",
        "pytest",
        "pydantic",
        "sqlalchemy",
        "celery",
        "redis",
        "pillow",
        "click",
        "typer",
        "rich",
        "httpx",
        "aiohttp",
        "tornado",
        "jinja2",
        "werkzeug",
    ];
    if py_packages.contains(&lower.as_str()) {
        return Ecosystem::Python;
    }

    Ecosystem::Unknown
}

/// Select documentation URLs for the given library and ecosystem.
fn resolve_doc_urls(library: &str, ecosystem: Ecosystem) -> Vec<(&'static str, String)> {
    match ecosystem {
        Ecosystem::Rust => {
            vec![
                (
                    "docs.rs",
                    format!("https://docs.rs/{library}/latest/{library}/"),
                ),
                (
                    "crates.io",
                    format!("https://crates.io/api/v1/crates/{library}"),
                ),
            ]
        }
        Ecosystem::JavaScript => {
            vec![("npm", format!("https://registry.npmjs.org/{library}"))]
        }
        Ecosystem::Python => {
            vec![("PyPI", format!("https://pypi.org/pypi/{library}/json"))]
        }
        Ecosystem::Unknown => {
            vec![
                ("npm", format!("https://registry.npmjs.org/{library}")),
                ("PyPI", format!("https://pypi.org/pypi/{library}/json")),
                (
                    "crates.io",
                    format!("https://crates.io/api/v1/crates/{library}"),
                ),
            ]
        }
    }
}

// ---------------------------------------------------------------------------
// Response parsing helpers
// ---------------------------------------------------------------------------

/// Extract the description / README from a crates.io API response.
fn parse_crates_io_response(body: &str) -> (Option<String>, Option<String>) {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(body) else {
        return (None, None);
    };
    let crate_obj = val.get("crate").unwrap_or(&val);
    let desc = crate_obj
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let version = crate_obj
        .get("max_version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    (desc, version)
}

/// Extract the description from an npm registry response.
fn parse_npm_response(body: &str) -> (Option<String>, Option<String>) {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(body) else {
        return (None, None);
    };
    let desc = val
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let readme = val
        .get("readme")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let version = val
        .get("dist-tags")
        .and_then(|dt| dt.get("latest"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    // Prefer the README (often much longer and more useful) over description
    let content = readme.or(desc).or_else(|| {
        val.get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    });
    (content, version)
}

/// Extract the description from a PyPI JSON API response.
fn parse_pypi_response(body: &str) -> (Option<String>, Option<String>) {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(body) else {
        return (None, None);
    };
    let info = val.get("info").unwrap_or(&val);
    let desc = info
        .get("summary")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let version = info
        .get("version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    (desc, version)
}

// ---------------------------------------------------------------------------
// HTML stripping (lightweight, reuses pattern from web.rs)
// ---------------------------------------------------------------------------

/// Strip HTML tags and collapse whitespace. Used for docs.rs pages that
/// return HTML instead of JSON.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut inside_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => inside_tag = true,
            '>' => {
                inside_tag = false;
                out.push(' ');
            }
            _ if !inside_tag => out.push(ch),
            _ => {}
        }
    }
    // Collapse whitespace
    let mut result = String::with_capacity(out.len());
    let mut prev_ws = false;
    for ch in out.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                result.push(' ');
                prev_ws = true;
            }
        } else {
            result.push(ch);
            prev_ws = false;
        }
    }
    result.trim().to_string()
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

/// In-memory cache keyed by library name.
type Cache = HashMap<String, DocsQueryOutput>;

// ---------------------------------------------------------------------------
// DocsQueryTool
// ---------------------------------------------------------------------------

/// Documentation query tool.
///
/// Resolves library/package names and fetches official documentation from
/// public package registries (docs.rs, npm, PyPI). Results are cached for
/// the lifetime of the tool instance.
pub struct DocsQueryTool {
    description: String,
    client: reqwest::Client,
    cache: Mutex<Cache>,
}

impl Default for DocsQueryTool {
    fn default() -> Self {
        Self::new()
    }
}

impl DocsQueryTool {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("ShannonCode/1.0 (docs-query)")
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("Failed to create HTTP client for DocsQuery: {e}");
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(20))
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new())
            });

        Self {
            description: "Fetch up-to-date documentation for libraries and frameworks. \
                Resolves package names via docs.rs (Rust), npm (JavaScript/TypeScript), \
                and PyPI (Python). Returns documentation snippets useful for writing correct code."
                .to_string(),
            client,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Create with a specific HTTP client (for testing).
    pub fn with_client(client: reqwest::Client) -> Self {
        Self {
            description: "Fetch up-to-date documentation for libraries and frameworks.".to_string(),
            client,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Clear the in-memory cache.
    pub fn clear_cache(&self) {
        let mut cache = self.cache.lock().expect("DocsQuery cache lock poisoned");
        cache.clear();
    }

    async fn resolve_docs(
        &self,
        input: &DocsQueryInput,
    ) -> Result<DocsQueryOutput, Box<dyn std::error::Error + Send + Sync>> {
        let library = input.library.trim().to_string();
        if library.is_empty() {
            return Err("Library name must not be empty".into());
        }

        // Check cache first
        {
            let cache = self.cache.lock().expect("DocsQuery cache lock poisoned");
            if let Some(cached) = cache.get(&library) {
                let mut output = cached.clone();
                output.cached = true;
                return Ok(output);
            }
        }

        let ecosystem = detect_ecosystem(&library);
        let urls = resolve_doc_urls(&library, ecosystem);
        let mut sources: Vec<DocsSource> = Vec::new();
        let mut resolved_version: Option<String> = None;

        for (source_name, url) in &urls {
            match self.fetch_and_parse(source_name, url).await {
                Ok((content, version)) => {
                    if let Some(v) = version {
                        resolved_version = Some(v);
                    }
                    sources.push(DocsSource {
                        source: source_name.to_string(),
                        url: url.clone(),
                        content,
                    });
                    // Stop after the first successful fetch
                    break;
                }
                Err(e) => {
                    tracing::debug!(
                        "DocsQuery: failed to fetch from {source_name} for {library}: {e}"
                    );
                }
            }
        }

        if sources.is_empty() {
            return Err(format!(
                "Could not find documentation for '{library}'. Tried: {}",
                urls.iter()
                    .map(|(name, _)| name.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .into());
        }

        // If we only got a short description from a registry API, try to
        // enrich it by fetching the actual docs page for Rust crates.
        if ecosystem == Ecosystem::Rust && sources.len() == 1 && sources[0].content.len() < 500 {
            let doc_url = format!("https://docs.rs/{library}/latest/{library}/");
            match self.client.get(&doc_url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(html) = resp.text().await {
                        let text = strip_html(&html);
                        let truncated = if text.len() > 8000 {
                            format!(
                                "{}...\n\n[truncated -- full docs at {doc_url}]",
                                &text[..8000]
                            )
                        } else {
                            text
                        };
                        sources[0].content = truncated;
                        sources[0].url = doc_url;
                        sources[0].source = "docs.rs (full)".to_string();
                    }
                }
                _ => {}
            }
        }

        let output = DocsQueryOutput {
            library: library.clone(),
            version: resolved_version,
            sources,
            cached: false,
        };

        // Store in cache
        {
            let mut cache = self.cache.lock().expect("DocsQuery cache lock poisoned");
            cache.insert(library, output.clone());
        }

        Ok(output)
    }

    /// Fetch a URL and parse the response according to the source type.
    async fn fetch_and_parse(
        &self,
        source: &str,
        url: &str,
    ) -> Result<(String, Option<String>), Box<dyn std::error::Error + Send + Sync>> {
        let response = self.client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()).into());
        }

        let body = response.text().await?;

        match source {
            "crates.io" => {
                let (desc, version) = parse_crates_io_response(&body);
                let content =
                    desc.unwrap_or_else(|| strip_html(&body).chars().take(2000).collect());
                Ok((content, version))
            }
            "npm" => {
                let (content, version) = parse_npm_response(&body);
                let content =
                    content.unwrap_or_else(|| strip_html(&body).chars().take(2000).collect());
                // Truncate very long readmes
                let truncated = if content.len() > 8000 {
                    format!(
                        "{}...\n\n[truncated -- full docs at https://www.npmjs.com/package]",
                        &content[..8000]
                    )
                } else {
                    content
                };
                Ok((truncated, version))
            }
            "PyPI" => {
                let (desc, version) = parse_pypi_response(&body);
                let content =
                    desc.unwrap_or_else(|| strip_html(&body).chars().take(2000).collect());
                Ok((content, version))
            }
            _ => {
                // Generic: try JSON parse, fall back to HTML stripping
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body) {
                    let desc = val
                        .get("description")
                        .or_else(|| val.get("summary"))
                        .or_else(|| val.get("readme"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let version = val
                        .get("version")
                        .or_else(|| val.get("max_version"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    Ok((desc, version))
                } else {
                    let text = strip_html(&body);
                    let truncated: String = text.chars().take(4000).collect();
                    Ok((truncated, None))
                }
            }
        }
    }
}

#[async_trait]
impl Tool for DocsQueryTool {
    fn name(&self) -> &str {
        "DocsQuery"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "library": {
                    "type": "string",
                    "description": "Library or package name to look up (e.g. 'react', 'serde', 'numpy', 'express')"
                },
                "query": {
                    "type": "string",
                    "description": "Optional specific question about the library (e.g. 'how to use useEffect cleanup')"
                },
                "version": {
                    "type": "string",
                    "description": "Optional version constraint (e.g. '18', '1.0'). Currently informational only."
                }
            },
            "required": ["library"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let query_input: DocsQueryInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid DocsQuery input: {e}")))?;

        let output = self
            .resolve_docs(&query_input)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("DocsQuery failed: {e}")))?;

        // Build a human-readable content string
        let mut content = String::new();

        content.push_str(&format!("# Documentation for {}\n", output.library));

        if let Some(ref version) = output.version {
            content.push_str(&format!("**Latest version**: {version}\n\n"));
        }

        if output.cached {
            content.push_str("(result from cache)\n\n");
        }

        for source in &output.sources {
            content.push_str(&format!(
                "## Source: {} ({})\n\n",
                source.source, source.url
            ));
            content.push_str(&source.content);
            content.push_str("\n\n");
        }

        // If the user asked a specific query, add a note
        if let Some(ref q) = query_input.query {
            content.push_str(&format!(
                "---\n*Query: '{q}' -- use the documentation above to answer.*\n"
            ));
        }

        let mut metadata = HashMap::new();
        metadata.insert("library".to_string(), json!(output.library));
        if let Some(ref v) = output.version {
            metadata.insert("version".to_string(), json!(v));
        }
        metadata.insert("cached".to_string(), json!(output.cached));
        metadata.insert(
            "sources".to_string(),
            json!(
                output
                    .sources
                    .iter()
                    .map(|s| json!({
                        "source": s.source,
                        "url": s.url,
                        "content_length": s.content.len(),
                    }))
                    .collect::<Vec<_>>()
            ),
        );

        Ok(ToolOutput {
            content,
            is_error: false,
            metadata,
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn category(&self) -> &str {
        "documentation"
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Ecosystem detection tests ----------------------------------------

    #[test]
    fn test_detect_rust_ecosystem() {
        assert_eq!(detect_ecosystem("serde"), Ecosystem::Rust);
        assert_eq!(detect_ecosystem("tokio"), Ecosystem::Rust);
        assert_eq!(detect_ecosystem("reqwest"), Ecosystem::Rust);
        assert_eq!(detect_ecosystem("clap"), Ecosystem::Rust);
        assert_eq!(detect_ecosystem("Serde"), Ecosystem::Rust);
    }

    #[test]
    fn test_detect_js_ecosystem() {
        assert_eq!(detect_ecosystem("react"), Ecosystem::JavaScript);
        assert_eq!(detect_ecosystem("express"), Ecosystem::JavaScript);
        assert_eq!(detect_ecosystem("next"), Ecosystem::JavaScript);
        assert_eq!(detect_ecosystem("React"), Ecosystem::JavaScript);
        assert_eq!(detect_ecosystem("tailwindcss"), Ecosystem::JavaScript);
    }

    #[test]
    fn test_detect_python_ecosystem() {
        assert_eq!(detect_ecosystem("numpy"), Ecosystem::Python);
        assert_eq!(detect_ecosystem("django"), Ecosystem::Python);
        assert_eq!(detect_ecosystem("fastapi"), Ecosystem::Python);
        assert_eq!(detect_ecosystem("Pydantic"), Ecosystem::Python);
    }

    #[test]
    fn test_detect_unknown_ecosystem() {
        assert_eq!(detect_ecosystem("some-random-lib"), Ecosystem::Unknown);
        assert_eq!(detect_ecosystem("my-cool-package"), Ecosystem::Unknown);
    }

    // ---- URL resolution tests ---------------------------------------------

    #[test]
    fn test_resolve_rust_urls() {
        let urls = resolve_doc_urls("serde", Ecosystem::Rust);
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0].0, "docs.rs");
        assert!(urls[0].1.contains("docs.rs/serde"));
        assert_eq!(urls[1].0, "crates.io");
        assert!(urls[1].1.contains("crates.io/api/v1/crates/serde"));
    }

    #[test]
    fn test_resolve_js_urls() {
        let urls = resolve_doc_urls("react", Ecosystem::JavaScript);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].0, "npm");
        assert!(urls[0].1.contains("registry.npmjs.org/react"));
    }

    #[test]
    fn test_resolve_python_urls() {
        let urls = resolve_doc_urls("numpy", Ecosystem::Python);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].0, "PyPI");
        assert!(urls[0].1.contains("pypi.org/pypi/numpy"));
    }

    #[test]
    fn test_resolve_unknown_tries_all() {
        let urls = resolve_doc_urls("mystery-lib", Ecosystem::Unknown);
        assert_eq!(urls.len(), 3);
    }

    // ---- Response parsing tests -------------------------------------------

    #[test]
    fn test_parse_crates_io_response() {
        let body = json!({
            "crate": {
                "id": "serde",
                "name": "serde",
                "description": "A generic serialization/deserialization framework",
                "max_version": "1.0.215",
                "downloads": 500000000
            }
        })
        .to_string();
        let (desc, version) = parse_crates_io_response(&body);
        assert_eq!(
            desc.unwrap(),
            "A generic serialization/deserialization framework"
        );
        assert_eq!(version.unwrap(), "1.0.215");
    }

    #[test]
    fn test_parse_crates_io_missing_fields() {
        let body = json!({"crate": {}}).to_string();
        let (desc, version) = parse_crates_io_response(&body);
        assert!(desc.is_none());
        assert!(version.is_none());
    }

    #[test]
    fn test_parse_crates_io_invalid_json() {
        let (desc, version) = parse_crates_io_response("not json");
        assert!(desc.is_none());
        assert!(version.is_none());
    }

    #[test]
    fn test_parse_npm_response_with_readme() {
        let body = json!({
            "name": "react",
            "description": "React is a JavaScript library.",
            "readme": "# React\n\nA declarative library.\n\n## Installation\n\nnpm install react",
            "dist-tags": {
                "latest": "18.3.1"
            }
        })
        .to_string();
        let (content, version) = parse_npm_response(&body);
        let content = content.unwrap();
        assert!(content.contains("React"));
        assert!(content.contains("npm install react"));
        assert_eq!(version.unwrap(), "18.3.1");
    }

    #[test]
    fn test_parse_npm_response_no_readme() {
        let body = json!({
            "name": "some-pkg",
            "description": "A simple package.",
            "dist-tags": {
                "latest": "1.0.0"
            }
        })
        .to_string();
        let (content, version) = parse_npm_response(&body);
        assert_eq!(content.unwrap(), "A simple package.");
        assert_eq!(version.unwrap(), "1.0.0");
    }

    #[test]
    fn test_parse_npm_response_invalid() {
        let (content, version) = parse_npm_response("bad json");
        assert!(content.is_none());
        assert!(version.is_none());
    }

    #[test]
    fn test_parse_pypi_response() {
        let body = json!({
            "info": {
                "name": "numpy",
                "summary": "Fundamental package for array computing in Python",
                "version": "2.1.3"
            }
        })
        .to_string();
        let (desc, version) = parse_pypi_response(&body);
        assert_eq!(
            desc.unwrap(),
            "Fundamental package for array computing in Python"
        );
        assert_eq!(version.unwrap(), "2.1.3");
    }

    #[test]
    fn test_parse_pypi_response_missing_info() {
        let body = json!({"info": {}}).to_string();
        let (desc, version) = parse_pypi_response(&body);
        assert!(desc.is_none());
        assert!(version.is_none());
    }

    // ---- HTML stripping tests ---------------------------------------------

    #[test]
    fn test_strip_html_simple() {
        assert_eq!(strip_html("<p>Hello <b>world</b></p>"), "Hello world");
    }

    #[test]
    fn test_strip_html_with_entities() {
        let result = strip_html("a &amp; b");
        assert!(result.contains("a"));
        assert!(result.contains("b"));
    }

    #[test]
    fn test_strip_html_empty() {
        assert_eq!(strip_html(""), "");
    }

    #[test]
    fn test_strip_html_no_tags() {
        assert_eq!(strip_html("plain text"), "plain text");
    }

    #[test]
    fn test_strip_html_collapses_whitespace() {
        let result = strip_html("<div>  lots   of    spaces  </div>");
        assert_eq!(result, "lots of spaces");
    }

    // ---- Input deserialization tests ---------------------------------------

    #[test]
    fn test_input_deserialization_full() {
        let input_json = json!({
            "library": "react",
            "query": "useEffect cleanup",
            "version": "18"
        });
        let input: DocsQueryInput = serde_json::from_value(input_json).unwrap();
        assert_eq!(input.library, "react");
        assert_eq!(input.query.unwrap(), "useEffect cleanup");
        assert_eq!(input.version.unwrap(), "18");
    }

    #[test]
    fn test_input_deserialization_minimal() {
        let input_json = json!({"library": "serde"});
        let input: DocsQueryInput = serde_json::from_value(input_json).unwrap();
        assert_eq!(input.library, "serde");
        assert!(input.query.is_none());
        assert!(input.version.is_none());
    }

    #[test]
    fn test_input_deserialization_missing_library() {
        let input_json = json!({"query": "test"});
        let result = serde_json::from_value::<DocsQueryInput>(input_json);
        assert!(result.is_err());
    }

    // ---- Tool trait tests --------------------------------------------------

    #[test]
    fn test_tool_name() {
        let tool = DocsQueryTool::new();
        assert_eq!(tool.name(), "DocsQuery");
    }

    #[test]
    fn test_tool_description_not_empty() {
        let tool = DocsQueryTool::new();
        assert!(!tool.description().is_empty());
        assert!(tool.description().contains("documentation"));
    }

    #[test]
    fn test_tool_schema() {
        let tool = DocsQueryTool::new();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("library")));
        assert!(schema["properties"]["library"].is_object());
        assert!(schema["properties"]["query"].is_object());
        assert!(schema["properties"]["version"].is_object());
    }

    #[test]
    fn test_tool_is_read_only() {
        let tool = DocsQueryTool::new();
        assert!(tool.is_read_only());
        assert!(tool.is_concurrency_safe());
    }

    #[test]
    fn test_tool_is_not_destructive() {
        let tool = DocsQueryTool::new();
        assert!(!tool.is_destructive());
    }

    #[test]
    fn test_tool_does_not_require_auth() {
        let tool = DocsQueryTool::new();
        assert!(!tool.requires_auth());
    }

    #[test]
    fn test_tool_category() {
        let tool = DocsQueryTool::new();
        assert_eq!(tool.category(), "documentation");
    }

    // ---- Cache tests -------------------------------------------------------

    #[test]
    fn test_cache_starts_empty() {
        let tool = DocsQueryTool::new();
        let cache = tool.cache.lock().unwrap();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_clear() {
        let tool = DocsQueryTool::new();
        {
            let mut cache = tool.cache.lock().unwrap();
            cache.insert(
                "test".to_string(),
                DocsQueryOutput {
                    library: "test".to_string(),
                    version: None,
                    sources: vec![],
                    cached: false,
                },
            );
        }
        {
            let cache = tool.cache.lock().unwrap();
            assert_eq!(cache.len(), 1);
        }
        tool.clear_cache();
        {
            let cache = tool.cache.lock().unwrap();
            assert!(cache.is_empty());
        }
    }

    // ---- Execute with invalid input tests ----------------------------------

    #[tokio::test]
    async fn test_execute_invalid_input() {
        let tool = DocsQueryTool::new();
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
        match result {
            Err(ToolError::InvalidInput(msg)) => {
                assert!(msg.contains("DocsQuery"));
            }
            _ => panic!("Expected InvalidInput error, got: {result:?}"),
        }
    }

    #[tokio::test]
    async fn test_execute_empty_library() {
        let tool = DocsQueryTool::new();
        let result = tool.execute(json!({"library": ""})).await;
        assert!(result.is_err());
        match result {
            Err(ToolError::ExecutionFailed(msg)) => {
                assert!(msg.contains("must not be empty"));
            }
            _ => panic!("Expected ExecutionFailed error, got: {result:?}"),
        }
    }

    // ---- Serialization tests -----------------------------------------------

    #[test]
    fn test_docs_source_serialization() {
        let source = DocsSource {
            source: "npm".to_string(),
            url: "https://registry.npmjs.org/react".to_string(),
            content: "React documentation".to_string(),
        };
        let val = serde_json::to_value(&source).unwrap();
        assert_eq!(val["source"], "npm");
        assert_eq!(val["url"], "https://registry.npmjs.org/react");
        assert_eq!(val["content"], "React documentation");
    }

    #[test]
    fn test_docs_query_output_serialization() {
        let output = DocsQueryOutput {
            library: "react".to_string(),
            version: Some("18.3.1".to_string()),
            sources: vec![DocsSource {
                source: "npm".to_string(),
                url: "https://registry.npmjs.org/react".to_string(),
                content: "React docs".to_string(),
            }],
            cached: false,
        };
        let val = serde_json::to_value(&output).unwrap();
        assert_eq!(val["library"], "react");
        assert_eq!(val["version"], "18.3.1");
        assert_eq!(val["cached"], false);
        assert_eq!(val["sources"].as_array().unwrap().len(), 1);
    }

    // ---- Output structure test ---------------------------------------------

    #[test]
    fn test_output_structure() {
        let output = DocsQueryOutput {
            library: "react".to_string(),
            version: Some("18.3.1".to_string()),
            sources: vec![DocsSource {
                source: "npm".to_string(),
                url: "https://registry.npmjs.org/react".to_string(),
                content: "React documentation content".to_string(),
            }],
            cached: false,
        };
        assert_eq!(output.library, "react");
        assert_eq!(output.version.as_deref(), Some("18.3.1"));
        assert_eq!(output.sources.len(), 1);
        assert_eq!(output.sources[0].content, "React documentation content");
        assert!(!output.cached);
    }

    // ---- Integration-style test with mock server --------------------------

    #[tokio::test]
    async fn test_full_execute_with_mockito() {
        let mut server = mockito::Server::new_async().await;

        let body = json!({
            "name": "mocklib",
            "description": "A mock library for testing.",
            "readme": "# MockLib\n\nThis is a test library.\n\n## Usage\n\nnpm install mocklib",
            "dist-tags": { "latest": "2.0.0" }
        })
        .to_string();

        let mock = server
            .mock("GET", "/mocklib")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&body)
            .create_async()
            .await;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();

        // Fetch from mock server and parse
        let response_url = format!("{}/mocklib", server.url());
        let resp = client.get(&response_url).send().await.unwrap();
        let resp_body = resp.text().await.unwrap();

        let (content, version) = parse_npm_response(&resp_body);
        assert!(content.unwrap().contains("MockLib"));
        assert_eq!(version.unwrap(), "2.0.0");

        mock.assert_async().await;
    }
}
