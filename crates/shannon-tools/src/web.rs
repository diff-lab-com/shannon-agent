//! Web operation tools
//!
//! Provides implementations for:
//! - WebFetch: Fetch and extract content from URLs
//! - WebSearch: Search the web for information

use crate::{Tool, ToolError, ToolResult};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Web operation types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation")]
pub enum WebOperation {
    Fetch(WebFetchInput),
    Search(WebSearchInput),
}

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
#[derive(Debug, Serialize, Deserialize)]
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

/// WebFetch tool implementation
pub struct WebFetchTool {
    description: String,
    client: Client,
}

impl WebFetchTool {
    pub fn new() -> Self {
        Self {
            description: "Fetches a URL from the internet and optionally extracts its contents".to_string(),
            client: Client::builder()
                .user_agent("Claude-Code/1.0")
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    async fn fetch_url(
        &self,
        url: &str,
        max_length: usize,
        start_index: usize,
        _raw: bool,
    ) -> Result<WebFetchOutput, ToolError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| ToolError::WebError(format!("Failed to fetch URL: {}", e)))?;

        if !response.status().is_success() {
            return Err(ToolError::WebError(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let full_content = response
            .text()
            .await
            .map_err(|e| ToolError::WebError(format!("Failed to read response: {}", e)))?;

        let content_length = full_content.len();

        // Extract requested range
        let end_index = (start_index + max_length).min(content_length);
        let content = full_content
            .get(start_index..end_index)
            .unwrap_or(&full_content[start_index..])
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
    async fn execute(&self, input: serde_json::Value) -> ToolResult<serde_json::Value> {
        let fetch_input: WebFetchInput = serde_json::from_value(input)?;

        let output = self
            .fetch_url(
                &fetch_input.url,
                fetch_input.max_length,
                fetch_input.start_index,
                fetch_input.raw,
            )
            .await?;

        serde_json::to_value(output).map_err(ToolError::from)
    }

    fn name(&self) -> &str {
        "WebFetch"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn validate_input(&self, input: &serde_json::Value) -> Result<(), ToolError> {
        if !input.is_object() {
            return Err(ToolError::WebError("Input must be an object".to_string()));
        }

        if input.get("url").is_none() {
            return Err(ToolError::WebError("Missing required field: url".to_string()));
        }

        // Validate URL format
        if let Some(url) = input.get("url").and_then(|v| v.as_str()) {
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return Err(ToolError::WebError("URL must start with http:// or https://".to_string()));
            }
        }

        Ok(())
    }
}

/// WebSearch tool implementation
pub struct WebSearchTool {
    description: String,
    client: Client,
    api_key: Option<String>,
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self {
            description: "A web search API that works just like Google Search".to_string(),
            client: Client::builder()
                .user_agent("Claude-Code/1.0")
                .build()
                .expect("Failed to create HTTP client"),
            api_key: std::env::var("TAVILY_API_KEY").ok(),
        }
    }

    async fn search(&self, query: &str, max_results: usize) -> Result<WebSearchOutput, ToolError> {
        // For now, return mock results
        // TODO: Integrate with Tavily or another search API

        let mock_results = vec![SearchResult {
            title: format!("Search Results for: {}", query),
            url: "https://example.com".to_string(),
            snippet: "Web search integration requires API key configuration".to_string(),
            score: Some(1.0),
            published_date: None,
        }];

        Ok(WebSearchOutput {
            query: query.to_string(),
            results: mock_results,
            count: 1,
            has_more: false,
        })
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<serde_json::Value> {
        let search_input: WebSearchInput = serde_json::from_value(input)?;

        let output = self
            .search(&search_input.query, search_input.max_results)
            .await?;

        serde_json::to_value(output).map_err(ToolError::from)
    }

    fn name(&self) -> &str {
        "WebSearch"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn validate_input(&self, input: &serde_json::Value) -> Result<(), ToolError> {
        if !input.is_object() {
            return Err(ToolError::WebError("Input must be an object".to_string()));
        }

        if input.get("query").is_none() {
            return Err(ToolError::WebError("Missing required field: query".to_string()));
        }

        Ok(())
    }
}
