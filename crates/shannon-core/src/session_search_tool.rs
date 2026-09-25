//! Model-facing session search tool (`SessionSearch`).
//!
//! Session history was previously a black box to the model: `/search` is a
//! REPL-only linear scan, `shannon trace` has no content search, and no
//! tool exposed past sessions — so anything not carried by compact
//! summaries or curated memory was unreachable. This tool opens the
//! episodic layer: a keyword search over the event-sourced transcripts
//! (`~/.shannon/sessions/<uuid>/events.jsonl`), so the model can recall
//! "what did we decide last time about X" without any index infrastructure.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::PathBuf;

use shannon_tool_interface::{Tool, ToolError, ToolOutput, ToolResult};

use crate::session_log::SessionStore;

/// Search past session transcripts by keyword.
pub struct SessionSearchTool {
    /// Sessions container directory (e.g. `~/.shannon/sessions`).
    container: PathBuf,
}

impl SessionSearchTool {
    /// Create the tool over the sessions container at `container`.
    pub fn new(container: impl Into<PathBuf>) -> Self {
        Self {
            container: container.into(),
        }
    }
}

#[async_trait]
impl Tool for SessionSearchTool {
    fn name(&self) -> &str {
        "SessionSearch"
    }

    fn description(&self) -> &str {
        "Search the user's past sessions (transcripts of previous \
         conversations) by keyword. Returns matching excerpts with session \
         ids, titles, and timestamps. Use to recall prior decisions, \
         debugging sessions, or anything discussed before — e.g. \"what did \
         we decide about the auth refactor last week?\". Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Case-insensitive keyword or phrase to search for"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of matches to return (default 8, max 25)",
                    "default": 8
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::InvalidInput("query must be a non-empty string".into()))?
            .to_string();
        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(8)
            .clamp(1, 25) as usize;

        // The scan is blocking filesystem IO over possibly many transcript
        // files — keep it off the async reactor.
        let container = self.container.clone();
        let search_query = query.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let store = SessionStore::new(container);
            store.search_all_with_stats(&search_query, limit)
        })
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("session search task failed: {e}")))?
        .map_err(|e| ToolError::ExecutionFailed(format!("session search failed: {e}")))?;

        if outcome.hits.is_empty() {
            return Ok(ToolOutput {
                content: format!(
                    "No sessions matched \"{}\" (searched {} of {} sessions).",
                    query, outcome.sessions_scanned, outcome.sessions_total
                ),
                is_error: false,
                metadata: {
                    let mut m = std::collections::HashMap::new();
                    m.insert(
                        "sessions_scanned".to_string(),
                        json!(outcome.sessions_scanned),
                    );
                    m.insert("sessions_total".to_string(), json!(outcome.sessions_total));
                    m
                },
            });
        }

        let mut out = format!(
            "Found {} match(es) for \"{}\" (searched {} of {} sessions):\n\n",
            outcome.hits.len(),
            query,
            outcome.sessions_scanned,
            outcome.sessions_total
        );
        for hit in &outcome.hits {
            let title = hit
                .title
                .as_deref()
                .or(hit.summary.as_deref())
                .unwrap_or("(untitled session)");
            let when = hit.timestamp.as_deref().unwrap_or("?");
            // Short session id: the same 8-char prefix /resume and the
            // desktop session picker accept.
            let short_id: String = hit.session_id.chars().take(8).collect();
            out.push_str(&format!(
                "- [{}] {} — {}\n  {}\n",
                short_id, when, title, hit.snippet
            ));
        }
        out.push_str("\nUse `shannon --resume <id>` (or /resume) to open a session.");

        Ok(ToolOutput {
            content: out,
            is_error: false,
            metadata: {
                let mut m = std::collections::HashMap::new();
                m.insert("matches".to_string(), json!(outcome.hits.len()));
                m.insert(
                    "sessions_scanned".to_string(),
                    json!(outcome.sessions_scanned),
                );
                m.insert("sessions_total".to_string(), json!(outcome.sessions_total));
                m
            },
        })
    }

    fn category(&self) -> &str {
        "memory"
    }

    fn is_read_only(&self) -> bool {
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn temp_container() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("shannon-session-search-test")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn search_empty_container_reports_zero() {
        let tool = SessionSearchTool::new(temp_container());
        let out = tool.execute(json!({ "query": "postgres" })).await.unwrap();
        assert!(!out.is_error);
        assert!(
            out.content.contains("No sessions matched"),
            "{}",
            out.content
        );
        let _ = std::fs::remove_dir_all(tool.container);
    }

    #[tokio::test]
    async fn search_requires_query() {
        let tool = SessionSearchTool::new(temp_container());
        assert!(tool.execute(json!({})).await.is_err());
        assert!(tool.execute(json!({ "query": "  " })).await.is_err());
        let _ = std::fs::remove_dir_all(tool.container);
    }

    #[tokio::test]
    async fn search_clamps_limit() {
        // limit > 25 must not error — it clamps.
        let tool = SessionSearchTool::new(temp_container());
        let out = tool
            .execute(json!({ "query": "x", "limit": 1000 }))
            .await
            .unwrap();
        assert!(!out.is_error);
        let _ = std::fs::remove_dir_all(tool.container);
    }
}
