//! Web operation tools
//!
//! Provides implementations for:
//! - WebFetch: Fetch and extract content from URLs
//! - WebSearch: Search the web for information

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

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
    ) -> Result<WebFetchOutput, Box<dyn std::error::Error + Send + Sync>> {
        let response = self.client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()).into());
        }

        let full_content = response.text().await?;

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
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let fetch_input: WebFetchInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid WebFetch input: {}", e)))?;

        let output = self
            .fetch_url(
                &fetch_input.url,
                fetch_input.max_length,
                fetch_input.start_index,
                fetch_input.raw,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to fetch URL: {}", e)))?;

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

    async fn search(&self, query: &str, max_results: usize) -> Result<WebSearchOutput, Box<dyn std::error::Error + Send + Sync>> {
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
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let search_input: WebSearchInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid WebSearch input: {}", e)))?;

        let output = self
            .search(&search_input.query, search_input.max_results)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Search failed: {}", e)))?;

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
                    "description": "Maximum number of results",
                    "default": 10
                },
                "search_depth": {
                    "type": "string",
                    "description": "Search depth (basic or advanced)"
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
}
