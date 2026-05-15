//! Web operation tools
//!
//! Provides implementations for:
//! - WebFetch: Fetch and extract content from URLs (with HTML-to-text conversion)
//! - WebSearch: Search the web via Tavily API (or other configurable providers)

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

/// Web operation types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation")]
pub enum WebOperation {
    Fetch(WebFetchInput),
    Search(WebSearchInput),
}

// ---------------------------------------------------------------------------
// WebFetch
// ---------------------------------------------------------------------------

/// WebFetch input parameters
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebFetchInput {
    /// URL to fetch
    pub url: String,

    /// Maximum number of characters to return
    #[serde(default = "default_max_length")]
    pub max_length: usize,

    /// Start index for pagination
    #[serde(default)]
    pub start_index: usize,

    /// Return raw HTML instead of simplified content
    #[serde(default)]
    pub raw: bool,
}

fn default_max_length() -> usize {
    5000
}

/// WebFetch output
#[derive(Debug, Serialize)]
pub struct WebFetchOutput {
    /// URL that was fetched
    pub url: String,

    /// Extracted content
    pub content: String,

    /// Content length
    pub content_length: usize,

    /// Whether more content is available
    pub has_more: bool,

    /// Next start index for pagination
    pub next_start_index: Option<usize>,
}

/// WebFetch tool implementation
pub struct WebFetchTool {
    description: String,
    client: Client,
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WebFetchTool {
    pub fn new() -> Self {
        Self {
            description: "Fetches a URL from the internet and optionally extracts its contents".to_string(),
            client: Client::builder()
                .user_agent("ShannonCode/1.0")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|e| {
                    tracing::error!("Failed to create HTTP client: {e}");
                    Client::new()
                }),
        }
    }

    async fn fetch_url(
        &self,
        url: &str,
        max_length: usize,
        start_index: usize,
        raw: bool,
    ) -> Result<WebFetchOutput, Box<dyn std::error::Error + Send + Sync>> {
        let response = self.client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()).into());
        }

        let full_content = response.text().await?;

        // Convert HTML to plain text unless raw mode is requested
        let processed = if raw {
            full_content
        } else {
            strip_html_tags(&full_content)
        };

        let content_length = processed.len();

        // Extract requested range (clamp start_index to avoid out-of-bounds panic)
        let start_index = start_index.min(processed.len());
        let end_index = (start_index + max_length).min(processed.len());
        let content = processed
            .get(start_index..end_index)
            .unwrap_or("")
            .to_string();

        let has_more = end_index < content_length;
        let next_start_index = if has_more { Some(end_index) } else { None };

        Ok(WebFetchOutput {
            url: url.to_string(),
            content,
            content_length: end_index - start_index,
            has_more,
            next_start_index,
        })
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let fetch_input: WebFetchInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid WebFetch input: {e}")))?;

        let output = self
            .fetch_url(
                &fetch_input.url,
                fetch_input.max_length,
                fetch_input.start_index,
                fetch_input.raw,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to fetch URL: {e}")))?;

        Ok(ToolOutput {
            content: format!("Successfully fetched {} bytes from {}", output.content_length, output.url),
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("url".to_string(), json!(output.url));
                map.insert("content_length".to_string(), json!(output.content_length));
                map.insert("has_more".to_string(), json!(output.has_more));
                if let Some(next) = output.next_start_index {
                    map.insert("next_start_index".to_string(), json!(next));
                }
                map.insert("content".to_string(), json!(output.content));
                map
            },
        })
    }

    fn name(&self) -> &str {
        "WebFetch"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                },
                "max_length": {
                    "type": "integer",
                    "description": "Maximum number of characters to return",
                    "default": 5000
                },
                "start_index": {
                    "type": "integer",
                    "description": "Start index for pagination"
                },
                "raw": {
                    "type": "boolean",
                    "description": "Return raw HTML instead of simplified content"
                }
            },
            "required": ["url"]
        })
    }
    fn is_read_only(&self) -> bool {        true    }
}

// ---------------------------------------------------------------------------
// WebSearch
// ---------------------------------------------------------------------------

/// Supported search providers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchProvider {
    Tavily,
}

impl SearchProvider {
    /// Parse provider name from string (case-insensitive). Defaults to Tavily.
    fn from_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "tavily" => SearchProvider::Tavily,
            _ => SearchProvider::Tavily,
        }
    }
}

/// WebSearch input parameters
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebSearchInput {
    /// Search query
    pub query: String,

    /// Maximum number of results
    #[serde(default = "default_max_results")]
    pub max_results: usize,

    /// Search depth (basic or advanced)
    #[serde(default)]
    pub search_depth: String,

    /// Include images in results
    #[serde(default)]
    pub include_images: bool,

    /// Include raw content
    #[serde(default)]
    pub include_raw_content: bool,
}

fn default_max_results() -> usize {
    10
}

/// WebSearch result item
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchResult {
    /// Result title
    pub title: String,

    /// Result URL
    pub url: String,

    /// Content snippet
    pub snippet: String,

    /// Optional score/relevance
    pub score: Option<f32>,

    /// Optional published date
    pub published_date: Option<String>,
}

/// WebSearch output
#[derive(Debug, Serialize)]
pub struct WebSearchOutput {
    /// Search query that was executed
    pub query: String,

    /// Search results
    pub results: Vec<SearchResult>,

    /// Number of results returned
    pub count: usize,

    /// Whether more results are available
    pub has_more: bool,
}

// ---------------------------------------------------------------------------
// Tavily API types
// ---------------------------------------------------------------------------

/// Request body sent to the Tavily search API
#[derive(Serialize)]
struct TavilyRequest {
    query: String,
    max_results: usize,
    include_answer: bool,
    search_depth: String,
}

/// A single result from the Tavily response
#[derive(Deserialize)]
struct TavilyResult {
    title: String,
    url: String,
    content: String,
    score: Option<f32>,
    published_date: Option<String>,
}

/// Top-level Tavily API response
#[derive(Deserialize)]
struct TavilyResponse {
    results: Vec<TavilyResult>,
}

// ---------------------------------------------------------------------------
// WebSearchTool
// ---------------------------------------------------------------------------

/// WebSearch tool implementation
///
/// Uses the Tavily search API by default. Configure via environment variables:
/// - `SHANNON_SEARCH_API_KEY` (required) -- your Tavily API key
/// - `SHANNON_SEARCH_PROVIDER` (optional, defaults to `tavily`)
///
/// Falls back to `TAVILY_API_KEY` for backward compatibility.
pub struct WebSearchTool {
    description: String,
    client: Client,
    api_key: Option<String>,
    provider: SearchProvider,
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSearchTool {
    pub fn new() -> Self {
        // Primary: SHANNON_SEARCH_API_KEY. Fallback: TAVILY_API_KEY.
        let api_key = std::env::var("SHANNON_SEARCH_API_KEY")
            .ok()
            .or_else(|| std::env::var("TAVILY_API_KEY").ok());

        let provider = std::env::var("SHANNON_SEARCH_PROVIDER")
            .map(|p| SearchProvider::from_name(&p))
            .unwrap_or(SearchProvider::Tavily);

        Self {
            description: "Search the web for information using a real search API".to_string(),
            client: Client::builder()
                .user_agent("ShannonCode/1.0")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|e| {
                    tracing::error!("Failed to create HTTP client: {e}");
                    Client::new()
                }),
            api_key,
            provider,
        }
    }

    /// Create a WebSearchTool with an explicit API key (useful for testing).
    pub fn with_api_key(key: String) -> Self {
        Self {
            description: "Search the web for information using a real search API".to_string(),
            client: Client::builder()
                .user_agent("ShannonCode/1.0")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|e| panic!("Failed to create HTTP client: {e}")),
            api_key: Some(key),
            provider: SearchProvider::Tavily,
        }
    }

    /// Create a WebSearchTool with no API key (useful for testing the no-key path).
    pub fn without_api_key() -> Self {
        Self {
            description: "Search the web for information using a real search API".to_string(),
            client: Client::builder()
                .user_agent("ShannonCode/1.0")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|e| panic!("Failed to create HTTP client: {e}")),
            api_key: None,
            provider: SearchProvider::Tavily,
        }
    }

    async fn search(
        &self,
        query: &str,
        max_results: usize,
        search_depth: &str,
    ) -> Result<WebSearchOutput, Box<dyn std::error::Error + Send + Sync>> {
        // ---- No API key configured ------------------------------------------------
        let api_key = match &self.api_key {
            Some(key) => key,
            None => {
                return Err(
                    "WebSearch requires an API key. Set the SHANNON_SEARCH_API_KEY environment variable to your Tavily API key (get one at https://app.tavily.com)."
                        .into(),
                );
            }
        };

        match self.provider {
            SearchProvider::Tavily => {
                self.search_tavily(query, max_results, search_depth, api_key)
                    .await
            }
        }
    }

    /// Call the Tavily search API and map the response into our generic types.
    async fn search_tavily(
        &self,
        query: &str,
        max_results: usize,
        search_depth: &str,
        api_key: &str,
    ) -> Result<WebSearchOutput, Box<dyn std::error::Error + Send + Sync>> {
        let request_body = TavilyRequest {
            query: query.to_string(),
            max_results: max_results.min(20), // Tavily caps at 20
            include_answer: true,
            search_depth: if search_depth == "advanced" {
                "advanced".to_string()
            } else {
                "basic".to_string()
            },
        };

        let response = self
            .client
            .post("https://api.tavily.com/search")
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "Tavily API returned HTTP {status}: {body}"
            )
            .into());
        }

        let tavily_response: TavilyResponse = response.json().await?;

        let results: Vec<SearchResult> = tavily_response
            .results
            .into_iter()
            .map(|r| SearchResult {
                title: r.title,
                url: r.url,
                snippet: r.content,
                score: r.score,
                published_date: r.published_date,
            })
            .collect();

        let count = results.len();

        Ok(WebSearchOutput {
            query: query.to_string(),
            results,
            count,
            has_more: false,
        })
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let search_input: WebSearchInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid WebSearch input: {e}")))?;

        let output = self
            .search(
                &search_input.query,
                search_input.max_results,
                &search_input.search_depth,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Search failed: {e}")))?;

        let results_json: Vec<serde_json::Value> = output.results.iter().map(|r| {
            json!({
                "title": r.title,
                "url": r.url,
                "snippet": r.snippet,
                "score": r.score,
                "published_date": r.published_date
            })
        }).collect();

        Ok(ToolOutput {
            content: format!("Found {} search results for: {}", output.count, output.query),
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("query".to_string(), json!(output.query));
                map.insert("count".to_string(), json!(output.count));
                map.insert("has_more".to_string(), json!(output.has_more));
                map.insert("results".to_string(), json!(results_json));
                map
            },
        })
    }

    fn name(&self) -> &str {
        "WebSearch"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results (1-20)",
                    "default": 10
                },
                "search_depth": {
                    "type": "string",
                    "description": "Search depth: 'basic' or 'advanced'",
                    "default": "basic"
                },
                "include_images": {
                    "type": "boolean",
                    "description": "Include images in results"
                },
                "include_raw_content": {
                    "type": "boolean",
                    "description": "Include raw content"
                }
            },
            "required": ["query"]
        })
    }
    fn is_read_only(&self) -> bool {        true    }
}

// ---------------------------------------------------------------------------
// HTML stripping
// ---------------------------------------------------------------------------

/// Remove HTML tags and decode common entities to produce readable plain text.
///
/// This is a lightweight, no-alloc-where-possible approach: we iterate over
/// characters, skip everything inside `<...>`, collapse whitespace, and
/// handle the most common HTML entities.
pub fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut inside_tag = false;
    let mut chars = html.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '<' => {
                // Check for <!-- ... --> comments
                if chars.peek() == Some(&'!') {
                    // Skip comment
                    chars.next(); // consume '!'
                    if chars.peek() == Some(&'-') {
                        chars.next(); // consume '-'
                        if chars.peek() == Some(&'-') {
                            chars.next(); // consume '-'
                            // Skip until closing -->
                            loop {
                                match chars.next() {
                                    Some('-') => {
                                        if chars.peek() == Some(&'-') {
                                            chars.next();
                                            if chars.peek() == Some(&'>') {
                                                chars.next();
                                                // Insert space after comment so adjacent text doesn't merge
                                                out.push(' ');
                                                break;
                                            }
                                        }
                                    }
                                    None => break,
                                    _ => {}
                                }
                            }
                        }
                    }
                } else {
                    inside_tag = true;
                }
            }
            '>' if inside_tag => {
                inside_tag = false;
                // Insert a space after closing tags so adjacent elements don't merge
                out.push(' ');
            }
            '&' if !inside_tag => {
                // Decode common HTML entities
                let entity = decode_html_entity(&mut chars);
                out.push_str(&entity);
            }
            _ if !inside_tag => {
                out.push(ch);
            }
            _ => {} // skip characters inside tags
        }
    }

    // Collapse runs of whitespace into single spaces
    collapse_whitespace(&out)
}

/// Decode a single HTML entity (e.g. &amp; &lt; &#39; &#x27;) starting
/// right after the `&` has been consumed.
fn decode_html_entity(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut buf = String::with_capacity(8);
    loop {
        match chars.peek() {
            Some(&';') => {
                chars.next(); // consume ';'
                break;
            }
            Some(&c) if c.is_ascii_alphanumeric() || c == '#' => {
                buf.push(c);
                chars.next();
            }
            _ => {
                // Not a valid entity, return literal '&' + what we consumed
                return format!("&{buf}");
            }
        }
    }

    match buf.as_str() {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        "nbsp" => " ".to_string(),
        s if s.starts_with('#') => {
            // Numeric entity: &#123; or &#x1A;
            let num_str = &s[1..];
            let codepoint = if let Some(hex) = num_str.strip_prefix('x') {
                u32::from_str_radix(hex, 16).ok()
            } else {
                num_str.parse::<u32>().ok()
            };
            match codepoint {
                Some(cp) => char::from_u32(cp).map(|c| c.to_string()).unwrap_or_default(),
                None => "&".to_string(),
            }
        }
        _ => format!("&{buf};"),
    }
}

/// Collapse consecutive whitespace characters into single spaces and trim.
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;

    for ch in s.chars() {
        if ch == '\n' || ch == '\r' {
            // Keep newlines but collapse multiple into one
            if !prev_space {
                out.push('\n');
                prev_space = true;
            }
        } else if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }

    out.trim().to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- HTML stripping tests ---------------------------------------------

    #[test]
    fn test_strip_simple_tags() {
        let html = "<p>Hello <b>world</b></p>";
        assert_eq!(strip_html_tags(html), "Hello world");
    }

    #[test]
    fn test_strip_nested_tags() {
        let html = "<div><span>deep</span><span>text</span></div>";
        assert_eq!(strip_html_tags(html), "deep text");
    }

    #[test]
    fn test_strip_preserves_text_between_tags() {
        let html = "start<p>middle</p>end";
        assert_eq!(strip_html_tags(html), "start middle end");
    }

    #[test]
    fn test_decode_amp_entity() {
        let html = "a &amp; b";
        assert_eq!(strip_html_tags(html), "a & b");
    }

    #[test]
    fn test_decode_lt_gt_entities() {
        let html = "a &lt; b &gt; c";
        assert_eq!(strip_html_tags(html), "a < b > c");
    }

    #[test]
    fn test_decode_quot_entity() {
        let html = "&quot;hello&quot;";
        assert_eq!(strip_html_tags(html), "\"hello\"");
    }

    #[test]
    fn test_decode_numeric_entity() {
        let html = "&#65;&#66;&#67;";
        assert_eq!(strip_html_tags(html), "ABC");
    }

    #[test]
    fn test_decode_hex_entity() {
        let html = "&#x41;&#x42;&#x43;";
        assert_eq!(strip_html_tags(html), "ABC");
    }

    #[test]
    fn test_decode_nbsp_entity() {
        let html = "a&nbsp;b";
        assert_eq!(strip_html_tags(html), "a b");
    }

    #[test]
    fn test_decode_apos_entity() {
        let html = "it&apos;s";
        assert_eq!(strip_html_tags(html), "it's");
    }

    #[test]
    fn test_strip_html_comments() {
        let html = "before<!-- comment -->after";
        assert_eq!(strip_html_tags(html), "before after");
    }

    #[test]
    fn test_strip_multiline_comments() {
        let html = "a<!-- multi\nline\ncomment -->b";
        assert_eq!(strip_html_tags(html), "a b");
    }

    #[test]
    fn test_collapse_whitespace() {
        let html = "<p>  Hello   \n\n   World  </p>";
        let result = strip_html_tags(html);
        // Whitespace should be collapsed but newlines preserved as single
        assert!(result.contains("Hello"));
        assert!(result.contains("World"));
        assert!(!result.contains("  ")); // no double spaces
    }

    #[test]
    fn test_strip_script_and_style() {
        // Script/style content is still rendered as text with our simple stripper,
        // but tags are removed
        let html = "<script>alert('x')</script><p>visible</p>";
        let result = strip_html_tags(html);
        assert!(result.contains("visible"));
        assert!(!result.contains("<script>"));
    }

    #[test]
    fn test_empty_html() {
        assert_eq!(strip_html_tags(""), "");
    }

    #[test]
    fn test_no_tags() {
        assert_eq!(strip_html_tags("plain text"), "plain text");
    }

    #[test]
    fn test_self_closing_tags() {
        let html = "<br/>line1<br />line2";
        let result = strip_html_tags(html);
        assert!(result.contains("line1"));
        assert!(result.contains("line2"));
    }

    #[test]
    fn test_attributes_in_tags() {
        let html = r#"<a href="http://example.com" class="link">text</a>"#;
        assert_eq!(strip_html_tags(html), "text");
    }

    // ---- Tavily response parsing tests -----------------------------------

    #[test]
    fn test_parse_tavily_response() {
        let json_body = r#"{
            "results": [
                {
                    "title": "Test Result",
                    "url": "https://example.com",
                    "content": "This is a test snippet",
                    "score": 0.95,
                    "published_date": "2025-01-15"
                },
                {
                    "title": "Second Result",
                    "url": "https://example.org",
                    "content": "Another snippet",
                    "score": null,
                    "published_date": null
                }
            ]
        }"#;

        let tavily: TavilyResponse = serde_json::from_str(json_body).unwrap();
        assert_eq!(tavily.results.len(), 2);
        assert_eq!(tavily.results[0].title, "Test Result");
        assert_eq!(tavily.results[0].url, "https://example.com");
        assert_eq!(tavily.results[0].score, Some(0.95));
        assert_eq!(tavily.results[1].score, None);
        assert_eq!(tavily.results[1].published_date, None);
    }

    #[test]
    fn test_parse_tavily_empty_results() {
        let json_body = r#"{"results": []}"#;
        let tavily: TavilyResponse = serde_json::from_str(json_body).unwrap();
        assert!(tavily.results.is_empty());
    }

    #[test]
    fn test_tavily_request_serialization() {
        let req = TavilyRequest {
            query: "rust programming".to_string(),
            max_results: 5,
            include_answer: true,
            search_depth: "basic".to_string(),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["query"], "rust programming");
        assert_eq!(json["max_results"], 5);
        assert_eq!(json["include_answer"], true);
        assert_eq!(json["search_depth"], "basic");
    }

    // ---- Search provider tests -------------------------------------------

    #[test]
    fn test_search_provider_from_name() {
        assert_eq!(SearchProvider::from_name("tavily"), SearchProvider::Tavily);
        assert_eq!(SearchProvider::from_name("TAVILY"), SearchProvider::Tavily);
        assert_eq!(SearchProvider::from_name("Tavily"), SearchProvider::Tavily);
        assert_eq!(SearchProvider::from_name("unknown"), SearchProvider::Tavily);
        assert_eq!(SearchProvider::from_name(""), SearchProvider::Tavily);
    }

    // ---- Search input parsing tests ---------------------------------------

    #[test]
    fn test_search_input_deserialization() {
        let json = json!({
            "query": "test query",
            "max_results": 5,
            "search_depth": "advanced"
        });
        let input: WebSearchInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.query, "test query");
        assert_eq!(input.max_results, 5);
        assert_eq!(input.search_depth, "advanced");
        assert!(!input.include_images);
        assert!(!input.include_raw_content);
    }

    #[test]
    fn test_search_input_defaults() {
        let json = json!({"query": "test"});
        let input: WebSearchInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.max_results, 10);
        assert_eq!(input.search_depth, "");
        assert!(!input.include_images);
    }

    // ---- Fetch input parsing tests ----------------------------------------

    #[test]
    fn test_fetch_input_deserialization() {
        let json = json!({
            "url": "https://example.com",
            "max_length": 1000,
            "start_index": 500,
            "raw": true
        });
        let input: WebFetchInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.url, "https://example.com");
        assert_eq!(input.max_length, 1000);
        assert_eq!(input.start_index, 500);
        assert!(input.raw);
    }

    #[test]
    fn test_fetch_input_defaults() {
        let json = json!({"url": "https://example.com"});
        let input: WebFetchInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.max_length, 5000);
        assert_eq!(input.start_index, 0);
        assert!(!input.raw);
    }

    // ---- Tool trait basic tests ------------------------------------------

    #[test]
    fn test_web_search_tool_name() {
        let tool = WebSearchTool::without_api_key();
        assert_eq!(tool.name(), "WebSearch");
    }

    #[test]
    fn test_web_search_tool_description() {
        let tool = WebSearchTool::without_api_key();
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_web_search_tool_schema() {
        let tool = WebSearchTool::without_api_key();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["query"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("query")));
    }

    #[test]
    fn test_web_fetch_tool_name() {
        let tool = WebFetchTool::new();
        assert_eq!(tool.name(), "WebFetch");
    }

    #[test]
    fn test_web_fetch_tool_description() {
        let tool = WebFetchTool::new();
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_web_fetch_tool_schema() {
        let tool = WebFetchTool::new();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("url")));
    }

    // ---- SearchResult serialization --------------------------------------

    #[test]
    fn test_search_result_serialization() {
        let result = SearchResult {
            title: "Test".to_string(),
            url: "https://example.com".to_string(),
            snippet: "A snippet".to_string(),
            score: Some(0.5), // Use a value that rounds cleanly in f32
            published_date: Some("2025-01-01".to_string()),
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["title"], "Test");
        assert_eq!(json["url"], "https://example.com");
        assert_eq!(json["score"], 0.5);
    }

    // ---- decode_html_entity unit tests ------------------------------------

    #[test]
    fn test_decode_entity_amp() {
        let mut chars = "amp;".chars().peekable();
        assert_eq!(decode_html_entity(&mut chars), "&");
    }

    #[test]
    fn test_decode_entity_lt() {
        let mut chars = "lt;".chars().peekable();
        assert_eq!(decode_html_entity(&mut chars), "<");
    }

    #[test]
    fn test_decode_entity_gt() {
        let mut chars = "gt;".chars().peekable();
        assert_eq!(decode_html_entity(&mut chars), ">");
    }

    #[test]
    fn test_decode_entity_numeric() {
        let mut chars = "#65;".chars().peekable();
        assert_eq!(decode_html_entity(&mut chars), "A");
    }

    #[test]
    fn test_decode_entity_hex() {
        let mut chars = "#x42;".chars().peekable();
        assert_eq!(decode_html_entity(&mut chars), "B");
    }

    #[test]
    fn test_decode_entity_unknown() {
        let mut chars = "unknown;".chars().peekable();
        assert_eq!(decode_html_entity(&mut chars), "&unknown;");
    }

    #[test]
    fn test_decode_entity_broken() {
        // Entity without closing semicolon -- the peek loop won't find ';'
        let mut chars = "amp test".chars().peekable();
        // 'a','m','p',' ' -- space is not alphanumeric so loop breaks
        assert_eq!(decode_html_entity(&mut chars), "&amp");
    }

    // ---- collapse_whitespace unit tests -----------------------------------

    #[test]
    fn test_collapse_whitespace_basic() {
        assert_eq!(collapse_whitespace("  a   b  c  "), "a b c");
    }

    #[test]
    fn test_collapse_whitespace_newlines() {
        assert_eq!(collapse_whitespace("a\n\nb"), "a\nb");
    }

    #[test]
    fn test_collapse_whitespace_mixed() {
        let result = collapse_whitespace("a \n \n b");
        assert!(result.contains("a"));
        assert!(result.contains("b"));
    }

    #[test]
    fn test_collapse_whitespace_empty() {
        assert_eq!(collapse_whitespace(""), "");
    }

    #[test]
    fn test_collapse_whitespace_trim() {
        assert_eq!(collapse_whitespace("  hello  "), "hello");
    }
}
