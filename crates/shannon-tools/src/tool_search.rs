//! Tool search / discovery tool
//!
//! Provides a `ToolSearchTool` that lets the AI discover available tools, their
//! descriptions, categories, authentication requirements, and input schemas.
//!
//! The tool holds an `Arc<std::sync::RwLock<ToolRegistry>>` so it can inspect
//! the live registry at execution time (including tools registered after the
//! search tool itself was created).

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use shannon_core::tools::{ToolInfo, ToolRegistry};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

// ---------------------------------------------------------------------------
// Input / Output types
// ---------------------------------------------------------------------------

/// Input for the tool search operation.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolSearchInput {
    /// Optional free-text query to filter tools by name or description.
    #[serde(default)]
    pub query: Option<String>,
    /// Optional category filter (exact match, case-insensitive).
    #[serde(default)]
    pub category: Option<String>,
}

/// Output from the tool search operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSearchOutput {
    /// Matching tools
    pub tools: Vec<ToolInfo>,
    /// Total number of matching tools
    pub count: usize,
}

// ---------------------------------------------------------------------------
// ToolSearchTool
// ---------------------------------------------------------------------------

/// A tool that lets the AI discover what tools are available in the registry.
///
/// Because the `Tool` trait requires `Send + Sync`, we wrap the registry in
/// `Arc<std::sync::RwLock<...>>` (the std variant, which is always `Send + Sync`).
pub struct ToolSearchTool {
    registry: Arc<RwLock<ToolRegistry>>,
}

impl ToolSearchTool {
    /// Create a new `ToolSearchTool` that reads from the given shared registry.
    pub fn new(registry: Arc<RwLock<ToolRegistry>>) -> Self {
        Self { registry }
    }

    /// Core search logic: list tools from the registry and apply filters.
    fn search(&self, input: ToolSearchInput) -> Result<ToolSearchOutput, ToolError> {
        let registry = self.registry.read().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire registry lock: {e}"))
        })?;

        let all_tools = registry.list_tools_info();

        let query_lower = input
            .query
            .as_deref()
            .map(str::to_lowercase);
        let category_lower = input
            .category
            .as_deref()
            .map(str::to_lowercase);

        let matching: Vec<ToolInfo> = all_tools
            .into_iter()
            .filter(|tool| {
                // Skip the search tool itself to avoid recursive self-reference in results
                if tool.name == "ToolSearch" {
                    return false;
                }

                // Category filter (case-insensitive exact match)
                if let Some(ref cat) = category_lower {
                    if tool.category.to_lowercase() != *cat {
                        return false;
                    }
                }

                // Query filter (case-insensitive substring match on name and description)
                if let Some(ref q) = query_lower {
                    let name_matches = tool.name.to_lowercase().contains(q);
                    let desc_matches = tool.description.to_lowercase().contains(q);
                    if !name_matches && !desc_matches {
                        return false;
                    }
                }

                true
            })
            .collect();

        let count = matching.len();

        Ok(ToolSearchOutput {
            tools: matching,
            count,
        })
    }
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "ToolSearch"
    }

    fn description(&self) -> &str {
        "Discover available tools, their descriptions, categories, and input schemas. \
         Optionally filter by a text query (matches tool name or description) or by category."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Optional search query to filter tools by name or description (case-insensitive substring match)"
                },
                "category": {
                    "type": "string",
                    "description": "Optional category filter (case-insensitive exact match, e.g. 'file', 'git', 'system')"
                }
            }
        })
    }

    fn category(&self) -> &str {
        "discovery"
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        // Accept empty / missing fields gracefully
        let search_input: ToolSearchInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid tool search input: {e}")))?;

        let output = self.search(search_input)?;

        let content = if output.count == 0 {
            "No matching tools found.".to_string()
        } else {
            format!("Found {} tool(s).", output.count)
        };

        Ok(ToolOutput {
            content,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("count".to_string(), json!(output.count));
                map.insert("tools".to_string(), json!(output.tools));
                map
            },
        })
    }
    fn is_read_only(&self) -> bool {        true    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;
    use shannon_core::tools::{Tool as ToolTrait, ToolRegistry};

    /// A minimal fake tool for testing the search tool.
    struct FakeTool {
        name: String,
        description: String,
        category: String,
        requires_auth: bool,
    }

    #[async_trait]
    impl ToolTrait for FakeTool {
        fn name(&self) -> &str {
            &self.name
        }
        fn description(&self) -> &str {
            &self.description
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }
        async fn execute(&self, _input: Value) -> shannon_core::tools::ToolResult<shannon_core::tools::ToolOutput> {
            Ok(shannon_core::tools::ToolOutput {
                content: "ok".into(),
                is_error: false,
                metadata: HashMap::new(),
            })
        }
        fn category(&self) -> &str {
            &self.category
        }
        fn requires_auth(&self) -> bool {
            self.requires_auth
        }
    }

    /// Helper: build a registry pre-populated with several fake tools.
    fn build_test_registry() -> Arc<RwLock<ToolRegistry>> {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(FakeTool {
            name: "ReadFile".into(),
            description: "Read the contents of a file from disk".into(),
            category: "file".into(),
            requires_auth: false,
        }))
        .unwrap();
        reg.register(Box::new(FakeTool {
            name: "WriteFile".into(),
            description: "Write content to a file on disk".into(),
            category: "file".into(),
            requires_auth: false,
        }))
        .unwrap();
        reg.register(Box::new(FakeTool {
            name: "GitDiff".into(),
            description: "Show differences between git commits or the working tree".into(),
            category: "git".into(),
            requires_auth: false,
        }))
        .unwrap();
        reg.register(Box::new(FakeTool {
            name: "Bash".into(),
            description: "Execute a shell command".into(),
            category: "system".into(),
            requires_auth: true,
        }))
        .unwrap();
        Arc::new(RwLock::new(reg))
    }

    // -- trait method tests ------------------------------------------------

    #[test]
    fn test_name() {
        let tool = ToolSearchTool::new(build_test_registry());
        assert_eq!(tool.name(), "ToolSearch");
    }

    #[test]
    fn test_description() {
        let tool = ToolSearchTool::new(build_test_registry());
        let desc = tool.description();
        assert!(desc.to_lowercase().contains("discover"));
        assert!(desc.contains("tools"));
    }

    #[test]
    fn test_category() {
        let tool = ToolSearchTool::new(build_test_registry());
        assert_eq!(tool.category(), "discovery");
    }

    #[test]
    fn test_input_schema() {
        let tool = ToolSearchTool::new(build_test_registry());
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("query"));
        assert!(props.contains_key("category"));
    }

    // -- search logic tests ------------------------------------------------

    #[test]
    fn test_search_no_filters_returns_all_tools() {
        let tool = ToolSearchTool::new(build_test_registry());
        let output = tool
            .search(ToolSearchInput {
                query: None,
                category: None,
            })
            .unwrap();
        // 4 fake tools registered; ToolSearch itself is excluded
        assert_eq!(output.count, 4);
    }

    #[test]
    fn test_search_query_filter_by_name() {
        let tool = ToolSearchTool::new(build_test_registry());
        let output = tool
            .search(ToolSearchInput {
                query: Some("file".into()),
                category: None,
            })
            .unwrap();
        assert_eq!(output.count, 2);
        let names: Vec<&str> = output.tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"ReadFile"));
        assert!(names.contains(&"WriteFile"));
    }

    #[test]
    fn test_search_query_filter_case_insensitive() {
        let tool = ToolSearchTool::new(build_test_registry());
        let output = tool
            .search(ToolSearchInput {
                query: Some("GIT".into()),
                category: None,
            })
            .unwrap();
        assert_eq!(output.count, 1);
        assert_eq!(output.tools[0].name, "GitDiff");
    }

    #[test]
    fn test_search_query_filter_by_description() {
        let tool = ToolSearchTool::new(build_test_registry());
        let output = tool
            .search(ToolSearchInput {
                query: Some("shell".into()),
                category: None,
            })
            .unwrap();
        assert_eq!(output.count, 1);
        assert_eq!(output.tools[0].name, "Bash");
    }

    #[test]
    fn test_search_category_filter() {
        let tool = ToolSearchTool::new(build_test_registry());
        let output = tool
            .search(ToolSearchInput {
                query: None,
                category: Some("file".into()),
            })
            .unwrap();
        assert_eq!(output.count, 2);
        for t in &output.tools {
            assert_eq!(t.category, "file");
        }
    }

    #[test]
    fn test_search_category_filter_case_insensitive() {
        let tool = ToolSearchTool::new(build_test_registry());
        let output = tool
            .search(ToolSearchInput {
                query: None,
                category: Some("SYSTEM".into()),
            })
            .unwrap();
        assert_eq!(output.count, 1);
        assert_eq!(output.tools[0].name, "Bash");
    }

    #[test]
    fn test_search_combined_query_and_category() {
        let tool = ToolSearchTool::new(build_test_registry());
        // Query matches "file" tools, category restricts to "git" -- no overlap
        let output = tool
            .search(ToolSearchInput {
                query: Some("file".into()),
                category: Some("git".into()),
            })
            .unwrap();
        assert_eq!(output.count, 0);
    }

    #[test]
    fn test_search_no_results() {
        let tool = ToolSearchTool::new(build_test_registry());
        let output = tool
            .search(ToolSearchInput {
                query: Some("nonexistent_tool_xyz".into()),
                category: None,
            })
            .unwrap();
        assert_eq!(output.count, 0);
        assert!(output.tools.is_empty());
    }

    #[test]
    fn test_search_excludes_itself() {
        let tool = ToolSearchTool::new(build_test_registry());
        let output = tool
            .search(ToolSearchInput {
                query: Some("ToolSearch".into()),
                category: None,
            })
            .unwrap();
        // Even though the query matches the search tool's name, it should be excluded
        assert_eq!(output.count, 0);
    }

    #[test]
    fn test_search_includes_requires_auth() {
        let tool = ToolSearchTool::new(build_test_registry());
        let output = tool
            .search(ToolSearchInput {
                query: None,
                category: None,
            })
            .unwrap();
        let bash = output.tools.iter().find(|t| t.name == "Bash").unwrap();
        assert!(bash.requires_auth);
        let read = output.tools.iter().find(|t| t.name == "ReadFile").unwrap();
        assert!(!read.requires_auth);
    }

    // -- execute (Tool trait) tests ----------------------------------------

    #[tokio::test]
    async fn test_execute_no_filters() {
        let tool = ToolSearchTool::new(build_test_registry());
        let result = tool
            .execute(json!({}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("4 tool(s)"));
        assert_eq!(result.metadata["count"], 4);
    }

    #[tokio::test]
    async fn test_execute_with_query() {
        let tool = ToolSearchTool::new(build_test_registry());
        let result = tool
            .execute(json!({"query": "file"}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(result.metadata["count"], 2);
    }

    #[tokio::test]
    async fn test_execute_empty_result() {
        let tool = ToolSearchTool::new(build_test_registry());
        let result = tool
            .execute(json!({"query": "nothing_matches_this"}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("No matching"));
        assert_eq!(result.metadata["count"], 0);
    }

    #[tokio::test]
    async fn test_execute_invalid_input() {
        let tool = ToolSearchTool::new(build_test_registry());
        // Pass a non-object value
        let result = tool.execute(json!("not an object")).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ToolError::InvalidInput(_)));
    }

    // -- Send + Sync compile-time verification ----------------------------
    // The following function body will fail to compile if ToolSearchTool is
    // not Send + Sync.
    fn _assert_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ToolSearchTool>();
    }
}
