//! Model-facing memory tools (`MemorySave` / `MemoryForget`).
//!
//! Until now the model had no way to curate its own memory: extraction was
//! background-only and users had to reach for `/remember` or the desktop UI.
//! These tools let the agent save durable facts and forget stale ones, like
//! Claude Code's curated memory write path / Letta's self-editing memory.
//!
//! The tools own a private [`MemoryStore`] handle on the SAME storage
//! directory as the host's injection store. Multi-writer correctness is the
//! store's core design (append-only writes + flock'd rewrite + tombstone
//! reconcile), so a second in-process handle is safe and avoids refactoring
//! [`QueryEngine`](crate::query_engine::QueryEngine) onto a shared-arc store.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Mutex;

use shannon_tool_interface::{Tool, ToolError, ToolOutput, ToolResult};

use super::store::{AddOutcome, MemoryStore};
use super::types::{MemoryCategory, MemoryEntry};

/// Current project key for tool writes — the canonical working directory.
fn current_project() -> String {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .to_string_lossy()
        .to_string()
}

/// Save a durable fact to the project's curated memory.
pub struct MemorySaveTool {
    store: Mutex<MemoryStore>,
}

impl MemorySaveTool {
    /// Create the tool against the shared memory storage directory
    /// (typically `~/.shannon/memories`).
    pub fn new(storage_path: PathBuf) -> Self {
        let mut store = MemoryStore::new(storage_path);
        let _ = store.load();
        Self {
            store: Mutex::new(store),
        }
    }
}

#[async_trait]
impl Tool for MemorySaveTool {
    fn name(&self) -> &str {
        "MemorySave"
    }

    fn description(&self) -> &str {
        "Save a durable fact about the project or the user's preferences to \
         long-term project memory (survives across sessions). Use for stable, \
         reusable knowledge — architectural decisions, preferred workflows, \
         recurring user corrections. Do NOT save transient task state, code \
         snippets, or anything already written in project instruction files \
         (CLAUDE.md / SHANNON.md)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The fact to remember — one concise, self-contained sentence"
                },
                "category": {
                    "type": "string",
                    "description": "Memory category",
                    "enum": ["Preference", "Pattern", "Decision", "Error", "Context"],
                    "default": "Context"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional short tags (e.g. [\"build\", \"cargo\"])"
                }
            },
            "required": ["content"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::InvalidInput("content must be a non-empty string".into()))?;

        if content.len() > 2000 {
            return Err(ToolError::InvalidInput(
                "content too long (max 2000 chars) — split into multiple facts".into(),
            ));
        }

        let category = match input.get("category").and_then(|v| v.as_str()) {
            Some("Preference") => MemoryCategory::Preference,
            Some("Pattern") => MemoryCategory::Pattern,
            Some("Decision") => MemoryCategory::Decision,
            Some("Error") => MemoryCategory::Error,
            _ => MemoryCategory::Context,
        };
        let tags: Vec<String> = input
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let mut entry = MemoryEntry::new(&current_project(), category, content);
        entry.tags = tags;
        entry.source_kind = Some(MemoryEntry::SOURCE_MANUAL.to_string());

        let mut store = self
            .store
            .lock()
            .map_err(|_| ToolError::ExecutionFailed("memory store lock poisoned".into()))?;
        let (outcome, id) = store
            .add_or_update_with_id(entry)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to save memory: {e}")))?;

        let note = match outcome {
            AddOutcome::Updated => " (merged with an existing memory)",
            AddOutcome::Inserted => "",
        };
        Ok(ToolOutput {
            content: format!("Saved memory {}{note}", &id[..8.min(id.len())]),
            is_error: false,
            metadata: {
                let mut m = std::collections::HashMap::new();
                m.insert("id".to_string(), json!(id));
                m
            },
        })
    }

    fn category(&self) -> &str {
        "memory"
    }

    fn is_read_only(&self) -> bool {
        false
    }
}

/// Delete a stale/incorrect memory by id prefix (as shown by `MemorySave` /
/// the `/recall` listing).
pub struct MemoryForgetTool {
    store: Mutex<MemoryStore>,
}

impl MemoryForgetTool {
    pub fn new(storage_path: PathBuf) -> Self {
        let mut store = MemoryStore::new(storage_path);
        let _ = store.load();
        Self {
            store: Mutex::new(store),
        }
    }
}

#[async_trait]
impl Tool for MemoryForgetTool {
    fn name(&self) -> &str {
        "MemoryForget"
    }

    fn description(&self) -> &str {
        "Delete an entry from long-term project memory by id (an id prefix as \
         short as 8 characters is accepted). Use when a remembered fact is \
         stale or was recorded in error."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Memory id or id prefix (min 8 chars)"
                }
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let id = input
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| s.len() >= 8)
            .ok_or_else(|| {
                ToolError::InvalidInput("id must be a memory id prefix (min 8 chars)".into())
            })?;

        let project = current_project();
        let mut store = self
            .store
            .lock()
            .map_err(|_| ToolError::ExecutionFailed("memory store lock poisoned".into()))?;
        let candidates: Vec<String> = store
            .project_memories(&project)
            .into_iter()
            .map(|m| m.id)
            .filter(|mem_id| mem_id.starts_with(id))
            .collect();

        match candidates.len() {
            0 => Ok(ToolOutput {
                content: format!("No memory found matching prefix {id}"),
                is_error: true,
                metadata: Default::default(),
            }),
            1 => {
                let deleted = store
                    .delete(&candidates[0])
                    .map_err(|e| ToolError::ExecutionFailed(format!("failed to delete: {e}")))?;
                Ok(ToolOutput {
                    content: if deleted {
                        format!("Deleted memory {}", &candidates[0][..8])
                    } else {
                        "Memory already absent".to_string()
                    },
                    is_error: false,
                    metadata: Default::default(),
                })
            }
            _ => Ok(ToolOutput {
                content: format!(
                    "Ambiguous prefix {id} — matches {} memories. Use a longer prefix.",
                    candidates.len()
                ),
                is_error: true,
                metadata: Default::default(),
            }),
        }
    }

    fn category(&self) -> &str {
        "memory"
    }

    fn is_read_only(&self) -> bool {
        false
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn temp_store_dir() -> PathBuf {
        std::env::temp_dir()
            .join("shannon-memory-tools-test")
            .join(uuid::Uuid::new_v4().to_string())
    }

    #[tokio::test]
    async fn memory_save_then_forget_roundtrip() {
        let dir = temp_store_dir();
        let save = MemorySaveTool::new(dir.clone());
        let out = save
            .execute(json!({ "content": "Use cargo nextest for tests", "category": "Pattern" }))
            .await
            .unwrap();
        assert!(!out.is_error);
        assert!(out.content.starts_with("Saved memory"));

        // Second save of the same fact (same category) merges instead of
        // duplicating; a different category is a distinct memory.
        let out2 = save
            .execute(json!({ "content": "Use cargo nextest for tests", "category": "Pattern" }))
            .await
            .unwrap();
        assert!(out2.content.contains("merged"), "{}", out2.content);

        // Forget by id prefix from the first save's metadata.
        let id = out
            .metadata
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string();
        let forget = MemoryForgetTool::new(dir.clone());
        let out3 = forget.execute(json!({ "id": id })).await.unwrap();
        assert!(!out3.is_error, "{}", out3.content);
        assert!(out3.content.starts_with("Deleted memory"));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn memory_forget_rejects_ambiguous_prefix() {
        let dir = temp_store_dir();
        let save = MemorySaveTool::new(dir.clone());
        save.execute(json!({ "content": "Fact one about deploy flow" }))
            .await
            .unwrap();
        save.execute(json!({ "content": "Fact two about deploy flow" }))
            .await
            .unwrap();

        let forget = MemoryForgetTool::new(dir.clone());
        let out = forget.execute(json!({ "id": "12345678" })).await.unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("No memory found"));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn memory_save_validates_content() {
        let dir = temp_store_dir();
        let save = MemorySaveTool::new(dir.clone());
        assert!(save.execute(json!({})).await.is_err());
        assert!(save.execute(json!({ "content": "" })).await.is_err());
        assert!(
            save.execute(json!({ "content": "x".repeat(3000) }))
                .await
                .is_err()
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
