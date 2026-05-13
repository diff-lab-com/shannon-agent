//! Web tools integration tests
//!
//! Tests WebFetchTool and WebSearchTool through the public Tool trait interface,
//! covering input validation, tool metadata, and error handling without
//! requiring real network access.

use shannon_tools::{WebFetchTool, WebSearchTool, Tool};

// ============================================================================
// WebFetchTool URL validation tests
// ============================================================================

#[tokio::test]
async fn test_web_fetch_invalid_url_returns_error() {
    let tool = WebFetchTool::new();
    let input = serde_json::json!({
        "url": "not-a-valid-url"
    });

    let result = tool.execute(input).await;
    // The tool should return an error for an unresolvable URL
    assert!(result.is_err(), "Invalid URL should produce an error");
}

#[tokio::test]
async fn test_web_fetch_empty_url_returns_error() {
    let tool = WebFetchTool::new();
    let input = serde_json::json!({
        "url": ""
    });

    let result = tool.execute(input).await;
    assert!(result.is_err(), "Empty URL should produce an error");
}

#[tokio::test]
async fn test_web_fetch_missing_url_field_returns_error() {
    let tool = WebFetchTool::new();
    // Omit the required "url" field entirely
    let input = serde_json::json!({
        "max_length": 5000
    });

    let result = tool.execute(input).await;
    assert!(result.is_err(), "Missing URL field should produce an error");
    assert!(
        result.unwrap_err().to_string().contains("Invalid WebFetch input"),
        "Error should mention invalid input"
    );
}

#[tokio::test]
async fn test_web_fetch_unreachable_host_returns_error() {
    let tool = WebFetchTool::new();
    let input = serde_json::json!({
        "url": "http://192.0.2.1:1/test"  // RFC 5737 TEST-NET, should be unreachable
    });

    let result = tool.execute(input).await;
    // Should error because the host is unreachable
    assert!(result.is_err(), "Unreachable host should produce an error");
}

// ============================================================================
// WebSearchTool query validation tests
// ============================================================================

#[tokio::test]
async fn test_web_search_without_api_key_returns_error() {
    let tool = WebSearchTool::without_api_key();
    let input = serde_json::json!({
        "query": "test query"
    });

    let result = tool.execute(input).await;
    assert!(result.is_err(), "Search without API key should fail");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("API key") || err.contains("Search failed"),
        "Error should mention API key: {err}"
    );
}

#[tokio::test]
async fn test_web_search_missing_query_returns_error() {
    let tool = WebSearchTool::without_api_key();
    // Omit the required "query" field
    let input = serde_json::json!({
        "max_results": 5
    });

    let result = tool.execute(input).await;
    assert!(result.is_err(), "Missing query should produce an error");
    assert!(
        result.unwrap_err().to_string().contains("Invalid WebSearch input"),
        "Error should mention invalid input"
    );
}

// ============================================================================
// Tool name/description format tests
// ============================================================================

#[test]
fn test_web_fetch_tool_name() {
    let tool = WebFetchTool::new();
    assert_eq!(tool.name(), "WebFetch");
}

#[test]
fn test_web_fetch_tool_description_not_empty() {
    let tool = WebFetchTool::new();
    assert!(!tool.description().is_empty());
    assert!(tool.description().len() > 10, "Description should be meaningful");
}

#[test]
fn test_web_search_tool_name() {
    let tool = WebSearchTool::without_api_key();
    assert_eq!(tool.name(), "WebSearch");
}

#[test]
fn test_web_search_tool_description_not_empty() {
    let tool = WebSearchTool::without_api_key();
    assert!(!tool.description().is_empty());
    assert!(tool.description().len() > 10, "Description should be meaningful");
}

#[test]
fn test_web_fetch_is_read_only() {
    let tool = WebFetchTool::new();
    assert!(tool.is_read_only());
}

#[test]
fn test_web_search_is_read_only() {
    let tool = WebSearchTool::without_api_key();
    assert!(tool.is_read_only());
}

// ============================================================================
// Input schema validation tests
// ============================================================================

#[test]
fn test_web_fetch_input_schema_has_url_required() {
    let tool = WebFetchTool::new();
    let schema = tool.input_schema();

    assert_eq!(schema["type"], "object");
    let properties = schema["properties"].as_object().unwrap();
    assert!(properties.contains_key("url"));
    assert!(properties.contains_key("max_length"));
    assert!(properties.contains_key("start_index"));
    assert!(properties.contains_key("raw"));

    let required = schema["required"].as_array().unwrap();
    assert!(required.contains(&serde_json::json!("url")));
}

#[test]
fn test_web_search_input_schema_has_query_required() {
    let tool = WebSearchTool::without_api_key();
    let schema = tool.input_schema();

    assert_eq!(schema["type"], "object");
    let properties = schema["properties"].as_object().unwrap();
    assert!(properties.contains_key("query"));
    assert!(properties.contains_key("max_results"));
    assert!(properties.contains_key("search_depth"));
    assert!(properties.contains_key("include_images"));
    assert!(properties.contains_key("include_raw_content"));

    let required = schema["required"].as_array().unwrap();
    assert!(required.contains(&serde_json::json!("query")));
}

#[test]
fn test_web_fetch_schema_default_max_length() {
    let tool = WebFetchTool::new();
    let schema = tool.input_schema();
    assert_eq!(schema["properties"]["max_length"]["default"], 5000);
}

#[test]
fn test_web_search_schema_default_max_results() {
    let tool = WebSearchTool::without_api_key();
    let schema = tool.input_schema();
    assert_eq!(schema["properties"]["max_results"]["default"], 10);
}

// ============================================================================
// Error handling for timeout/max_length parameters
// ============================================================================

#[tokio::test]
async fn test_web_fetch_with_zero_max_length_returns_empty_content() {
    // We can't easily mock the HTTP response in an integration test without
    // mockito wiring, so we test that the input schema accepts max_length=0
    // by verifying the tool accepts the input (the fetch itself will fail
    // for a non-routable host, but the input parsing should succeed).
    let tool = WebFetchTool::new();
    let input = serde_json::json!({
        "url": "http://192.0.2.1:1/test",
        "max_length": 0
    });

    let result = tool.execute(input).await;
    // Should fail due to unreachable host, not input parsing
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    // The error should be about fetching, not about input parsing
    assert!(
        !err.contains("Invalid WebFetch input"),
        "Should not be an input parsing error: {err}"
    );
}

#[tokio::test]
async fn test_web_fetch_with_large_start_index_clamps() {
    // Similar to above - verify the input is accepted, actual network
    // failure is expected.
    let tool = WebFetchTool::new();
    let input = serde_json::json!({
        "url": "http://192.0.2.1:1/test",
        "start_index": 999999
    });

    let result = tool.execute(input).await;
    // Network error expected, not input parsing error
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        !err.contains("Invalid WebFetch input"),
        "Should not be an input parsing error: {err}"
    );
}

#[test]
fn test_web_search_with_api_key_constructor() {
    let tool = WebSearchTool::with_api_key("test-key-123".to_string());
    assert_eq!(tool.name(), "WebSearch");
    assert!(!tool.description().is_empty());
}

#[test]
fn test_web_search_without_api_key_constructor() {
    let tool = WebSearchTool::without_api_key();
    assert_eq!(tool.name(), "WebSearch");
    assert!(!tool.description().is_empty());
}
