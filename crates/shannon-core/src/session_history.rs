//! # Session History Management
//!
//! Provides session history listing, filtering, searching, archiving, and
//! resumption capabilities. Builds on top of the persistent session storage
//! managed by [`crate::state::StateManager`].

use crate::api::{ContentBlock, Message, MessageContent};
use crate::query_engine::CostTracker;
use crate::state::SessionData;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info};
use uuid::Uuid;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during session history operations.
#[derive(Error, Debug)]
pub enum SessionHistoryError {
    #[error("Session not found: {0}")]
    SessionNotFound(Uuid),

    #[error("State error: {0}")]
    StateError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid filter: {0}")]
    InvalidFilter(String),

    #[error("Title generation error: {0}")]
    TitleGenerationError(String),
}

// ============================================================================
// Sort / Filter Types
// ============================================================================

/// Field to sort session listings by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionSortField {
    UpdatedAt,
    CreatedAt,
    MessageCount,
    TotalTokens,
    TotalCost,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortOrder {
    Descending,
    Ascending,
}

/// Filter criteria for session listings.
#[derive(Debug, Clone)]
pub struct SessionFilter {
    pub project_path: Option<String>,
    pub date_after: Option<chrono::DateTime<chrono::Utc>>,
    pub date_before: Option<chrono::DateTime<chrono::Utc>>,
    pub include_archived: bool,
    pub limit: usize,
    pub sort_by: SessionSortField,
    pub sort_order: SortOrder,
    pub tags: Vec<String>,
}

impl Default for SessionFilter {
    fn default() -> Self {
        Self {
            project_path: None,
            date_after: None,
            date_before: None,
            include_archived: false,
            limit: 50,
            sort_by: SessionSortField::UpdatedAt,
            sort_order: SortOrder::Descending,
            tags: Vec::new(),
        }
    }
}

// ============================================================================
// Data Types
// ============================================================================

/// A single session history entry suitable for display / listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHistoryEntry {
    pub session_id: Uuid,
    pub title: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub message_count: usize,
    pub total_tokens: u64,
    pub total_cost: f64,
    pub model: String,
    pub project_path: Option<String>,
    pub tags: Vec<String>,
    pub is_archived: bool,
    pub file_size_bytes: u64,
}

/// Metadata extracted from a persisted session for resumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub model: String,
    pub tools_used: Vec<String>,
    pub files_accessed: Vec<String>,
    pub duration_seconds: u64,
}

/// Everything needed to resume a previous session.
#[derive(Debug, Clone)]
pub struct ResumeInfo {
    pub session_id: Uuid,
    pub title: String,
    pub messages: Vec<Message>,
    pub cost_tracker: CostTracker,
    pub metadata: SessionMetadata,
}

// ============================================================================
// Session History Manager
// ============================================================================

/// Manages session history -- listing, searching, archiving, and resuming.
///
/// Operates on the same JSON files produced by `StateManager` under the
/// configured sessions directory.
pub struct SessionHistoryManager {
    sessions_dir: PathBuf,
    max_sessions: usize,
    max_session_age: Duration,
}

impl SessionHistoryManager {
    /// Default maximum number of sessions to keep.
    const DEFAULT_MAX_SESSIONS: usize = 100;
    /// Default maximum session age before auto-archive (30 days).
    const DEFAULT_MAX_SESSION_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

    /// Create a new history manager backed by the given directory.
    ///
    /// The directory is created if it does not exist.
    pub fn new(sessions_dir: PathBuf) -> Result<Self, SessionHistoryError> {
        fs::create_dir_all(&sessions_dir)?;
        Ok(Self {
            sessions_dir,
            max_sessions: Self::DEFAULT_MAX_SESSIONS,
            max_session_age: Self::DEFAULT_MAX_SESSION_AGE,
        })
    }

    /// Create with custom limits.
    pub fn with_config(
        sessions_dir: PathBuf,
        max_sessions: usize,
        max_session_age: Duration,
    ) -> Result<Self, SessionHistoryError> {
        fs::create_dir_all(&sessions_dir)?;
        Ok(Self {
            sessions_dir,
            max_sessions,
            max_session_age,
        })
    }

    /// Build the file path for a session UUID.
    fn session_file_path(&self, session_id: &Uuid) -> PathBuf {
        self.sessions_dir.join(format!("{session_id}.json"))
    }

    /// Read and parse a single session JSON file.
    fn read_session_file(&self, session_id: &Uuid) -> Result<SessionData, SessionHistoryError> {
        let path = self.session_file_path(session_id);
        if !path.exists() {
            return Err(SessionHistoryError::SessionNotFound(*session_id));
        }
        let contents = fs::read_to_string(&path)?;
        let data: SessionData =
            serde_json::from_str(&contents).map_err(|e| {
                SessionHistoryError::DeserializationError(e.to_string())
            })?;
        Ok(data)
    }

    /// Convert `SessionData` into a `SessionHistoryEntry`.
    fn entry_from_session_data(
        &self,
        data: &SessionData,
    ) -> Result<SessionHistoryEntry, SessionHistoryError> {
        let path = self.session_file_path(&data.session_id);
        let file_size = path.metadata().map(|m| m.len()).unwrap_or(0);

        let total_tokens =
            data.metadata.total_input_tokens + data.metadata.total_output_tokens;
        let total_cost = CostTracker::calculate_cost(
            &data.metadata.model,
            data.metadata.total_input_tokens,
            data.metadata.total_output_tokens,
        );

        let title = data
            .metadata
            .title
            .clone()
            .unwrap_or_else(|| {
                data.first_user_message_preview(60)
                    .unwrap_or_else(|| "Untitled session".to_string())
            });

        Ok(SessionHistoryEntry {
            session_id: data.session_id,
            title,
            created_at: data.metadata.created_at,
            updated_at: data.metadata.updated_at,
            message_count: data.messages.len(),
            total_tokens,
            total_cost,
            model: data.metadata.model.clone(),
            project_path: None,
            tags: Vec::new(),
            is_archived: false,
            file_size_bytes: file_size,
        })
    }

    // ----------------------------------------------------------------
    // Public API
    // ----------------------------------------------------------------

    /// List sessions matching the given filter.
    pub fn list_sessions(
        &self,
        filter: &SessionFilter,
    ) -> Result<Vec<SessionHistoryEntry>, SessionHistoryError> {
        if !self.sessions_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries: Vec<SessionHistoryEntry> = Vec::new();

        let dir_entries = fs::read_dir(&self.sessions_dir)?;
        for entry in dir_entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();

            // Only consider .json files.
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let contents = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let data: SessionData = match serde_json::from_str(&contents) {
                Ok(d) => d,
                Err(_) => continue,
            };

            let history_entry = self.entry_from_session_data(&data)?;
            entries.push(history_entry);
        }

        // -- Apply filters --

        // Date range
        if let Some(after) = filter.date_after {
            entries.retain(|e| e.updated_at > after);
        }
        if let Some(before) = filter.date_before {
            entries.retain(|e| e.updated_at < before);
        }

        // Tags (not stored on disk yet, but filter if present on entry)
        if !filter.tags.is_empty() {
            entries.retain(|e| {
                filter
                    .tags
                    .iter()
                    .any(|tag| e.tags.iter().any(|t| t == tag))
            });
        }

        // Project path
        if let Some(ref proj) = filter.project_path {
            entries.retain(|e| {
                e.project_path
                    .as_ref()
                    .map(|p| p == proj)
                    .unwrap_or(false)
            });
        }

        // -- Sort --
        match filter.sort_by {
            SessionSortField::UpdatedAt => {
                entries.sort_by(|a, b| {
                    if filter.sort_order == SortOrder::Descending {
                        b.updated_at.cmp(&a.updated_at)
                    } else {
                        a.updated_at.cmp(&b.updated_at)
                    }
                });
            }
            SessionSortField::CreatedAt => {
                entries.sort_by(|a, b| {
                    if filter.sort_order == SortOrder::Descending {
                        b.created_at.cmp(&a.created_at)
                    } else {
                        a.created_at.cmp(&b.created_at)
                    }
                });
            }
            SessionSortField::MessageCount => {
                entries.sort_by(|a, b| {
                    if filter.sort_order == SortOrder::Descending {
                        b.message_count.cmp(&a.message_count)
                    } else {
                        a.message_count.cmp(&b.message_count)
                    }
                });
            }
            SessionSortField::TotalTokens => {
                entries.sort_by(|a, b| {
                    if filter.sort_order == SortOrder::Descending {
                        b.total_tokens.cmp(&a.total_tokens)
                    } else {
                        a.total_tokens.cmp(&b.total_tokens)
                    }
                });
            }
            SessionSortField::TotalCost => {
                entries.sort_by(|a, b| {
                    if filter.sort_order == SortOrder::Descending {
                        b.total_cost
                            .partial_cmp(&a.total_cost)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.total_cost
                            .partial_cmp(&b.total_cost)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }
                });
            }
        }

        // -- Limit --
        entries.truncate(filter.limit);

        Ok(entries)
    }

    /// Get a single session entry by ID.
    pub fn get_session(
        &self,
        session_id: &Uuid,
    ) -> Result<Option<SessionHistoryEntry>, SessionHistoryError> {
        let path = self.session_file_path(session_id);
        if !path.exists() {
            return Ok(None);
        }
        let data = self.read_session_file(session_id)?;
        let entry = self.entry_from_session_data(&data)?;
        Ok(Some(entry))
    }

    /// Search sessions by matching the query against titles and message content.
    ///
    /// The search is case-insensitive and checks the session title, plus the
    /// text content of every message.
    pub fn search_sessions(
        &self,
        query: &str,
    ) -> Result<Vec<SessionHistoryEntry>, SessionHistoryError> {
        let all = self.list_sessions(&SessionFilter {
            limit: self.max_sessions,
            ..Default::default()
        })?;

        let query_lower = query.to_lowercase();

        let matches: Vec<SessionHistoryEntry> = all
            .into_iter()
            .filter(|entry| {
                // Match against title
                if entry.title.to_lowercase().contains(&query_lower) {
                    return true;
                }

                // Match against message content (read full session only when needed)
                if let Ok(data) = self.read_session_file(&entry.session_id) {
                    for msg in &data.messages {
                        if let Some(text) = extract_text_content(msg) {
                            if text.to_lowercase().contains(&query_lower) {
                                return true;
                            }
                        }
                    }
                }

                false
            })
            .collect();

        Ok(matches)
    }

    /// Delete a session from disk.
    pub fn delete_session(&self, session_id: &Uuid) -> Result<(), SessionHistoryError> {
        let path = self.session_file_path(session_id);
        if path.exists() {
            fs::remove_file(&path)?;
            info!(session_id = %session_id, "Deleted session");
        } else {
            return Err(SessionHistoryError::SessionNotFound(*session_id));
        }
        Ok(())
    }

    /// Archive a session by moving its JSON file to an `archived/` subdirectory.
    pub fn archive_session(&self, session_id: &Uuid) -> Result<(), SessionHistoryError> {
        let src = self.session_file_path(session_id);
        if !src.exists() {
            return Err(SessionHistoryError::SessionNotFound(*session_id));
        }

        let archive_dir = self.sessions_dir.join("archived");
        fs::create_dir_all(&archive_dir)?;

        let dst = archive_dir.join(format!("{session_id}.json"));
        fs::rename(&src, &dst)?;
        info!(session_id = %session_id, "Archived session");
        Ok(())
    }

    /// Remove sessions exceeding `max_session_age` and enforce `max_sessions` limit.
    ///
    /// Returns the number of sessions that were cleaned up.
    pub fn cleanup_old_sessions(&self) -> Result<usize, SessionHistoryError> {
        let cutoff = chrono::Utc::now() - chrono::Duration::from_std(self.max_session_age)
            .unwrap_or(chrono::Duration::zero());

        let all = self.list_sessions(&SessionFilter {
            limit: self.max_sessions * 2, // fetch more so we can trim
            include_archived: true,
            ..Default::default()
        })?;

        let mut removed = 0usize;

        for entry in &all {
            if entry.updated_at < cutoff
                && self.archive_session(&entry.session_id).is_ok() {
                    removed += 1;
                }
        }

        // Enforce max_sessions by archiving the oldest beyond the limit.
        let remaining = self.list_sessions(&SessionFilter {
            limit: self.max_sessions * 2,
            ..Default::default()
        })?;

        if remaining.len() > self.max_sessions {
            let excess = &remaining[self.max_sessions..];
            for entry in excess {
                if self.archive_session(&entry.session_id).is_ok() {
                    removed += 1;
                }
            }
        }

        if removed > 0 {
            info!(removed = removed, "Cleaned up old sessions");
        }

        Ok(removed)
    }

    /// Generate a title for a session from its messages.
    ///
    /// Uses a simple heuristic: the first user message, truncated to a
    /// reasonable length. A future version could call an AI model.
    pub fn generate_title(
        &self,
        session_id: &Uuid,
        messages: &[Message],
    ) -> Result<String, SessionHistoryError> {
        let title = generate_title_from_messages(messages);
        debug!(session_id = %session_id, title = %title, "Generated session title");
        Ok(title)
    }

    /// Load messages for a specific session (needed for resume).
    pub fn get_session_messages(
        &self,
        session_id: &Uuid,
    ) -> Result<Vec<Message>, SessionHistoryError> {
        let data = self.read_session_file(session_id)?;
        Ok(data.messages)
    }

    /// Build a `ResumeInfo` bundle for resuming a session.
    pub fn resume_session(
        &self,
        session_id: &Uuid,
    ) -> Result<ResumeInfo, SessionHistoryError> {
        let data = self.read_session_file(session_id)?;

        let title = data
            .metadata
            .title
            .clone()
            .unwrap_or_else(|| {
                data.first_user_message_preview(60)
                    .unwrap_or_else(|| "Untitled session".to_string())
            });

        let mut cost_tracker = CostTracker::new(data.metadata.model.clone());
        cost_tracker.record_usage(
            &data.metadata.model,
            data.metadata.total_input_tokens,
            data.metadata.total_output_tokens,
        );

        let tools_used = extract_tools_used(&data.messages);
        let files_accessed = extract_files_accessed(&data.messages);
        let duration_seconds = (data.metadata.updated_at - data.metadata.created_at)
            .num_seconds()
            .unsigned_abs();

        let metadata = SessionMetadata {
            model: data.metadata.model,
            tools_used,
            files_accessed,
            duration_seconds,
        };

        info!(session_id = %session_id, "Prepared session for resume");

        Ok(ResumeInfo {
            session_id: data.session_id,
            title,
            messages: data.messages,
            cost_tracker,
            metadata,
        })
    }
}

// ============================================================================
// Free helpers
// ============================================================================

/// Generate a title from a list of messages using a simple heuristic.
///
/// Takes the first user message and truncates it to 60 characters, appending
/// "..." if truncated.
pub fn generate_title_from_messages(messages: &[Message]) -> String {
    const MAX_TITLE_LEN: usize = 60;

    for msg in messages {
        if msg.role == "user" {
            if let Some(text) = extract_text_content(msg) {
                let trimmed = text.trim().to_string();
                if !trimmed.is_empty() {
                    return if trimmed.len() > MAX_TITLE_LEN {
                        format!("{}...", &trimmed[..MAX_TITLE_LEN.saturating_sub(3)])
                    } else {
                        trimmed
                    };
                }
            }
        }
    }

    "Untitled session".to_string()
}

/// Extract plain text from a message (handles both Text and Blocks variants).
fn extract_text_content(msg: &Message) -> Option<String> {
    match &msg.content {
        MessageContent::Text(t) => Some(t.clone()),
        MessageContent::Blocks(blocks) => {
            let texts: Vec<&str> = blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join(" "))
            }
        }
    }
}

/// Extract unique tool names used in a message list.
fn extract_tools_used(messages: &[Message]) -> Vec<String> {
    let mut tools = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for msg in messages {
        if let MessageContent::Blocks(blocks) = &msg.content {
            for block in blocks {
                if let ContentBlock::ToolUse { name, .. } = block {
                    if seen.insert(name.clone()) {
                        tools.push(name.clone());
                    }
                }
            }
        }
    }

    tools
}

/// Extract file paths referenced in tool use inputs.
fn extract_files_accessed(messages: &[Message]) -> Vec<String> {
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for msg in messages {
        if let MessageContent::Blocks(blocks) = &msg.content {
            for block in blocks {
                if let ContentBlock::ToolUse { name, input, .. } = block {
                    // Check common file-related fields in tool inputs.
                    let file_fields = [
                        "file_path",
                        "path",
                        "file",
                        "source",
                        "source_file",
                        "target",
                        "target_file",
                        "directory",
                    ];
                    for field in &file_fields {
                        if let Some(value) = input.get(*field) {
                            if let Some(s) = value.as_str() {
                                if seen.insert(s.to_string()) {
                                    files.push(s.to_string());
                                }
                            }
                        }
                    }
                    // Also check for a "files" array.
                    if let Some(arr) = input.get("files").and_then(|v| v.as_array()) {
                        for item in arr {
                            if let Some(s) = item.as_str() {
                                if seen.insert(s.to_string()) {
                                    files.push(s.to_string());
                                }
                            }
                        }
                    }
                    let _ = name; // acknowledged
                }
            }
        }
    }

    files
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::MessageContent;
    use crate::state::{SessionPersistMetadata, StateManager};

    // -- helpers --

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("shannon-test-history")
            .join(Uuid::new_v4().to_string());
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn manager() -> SessionHistoryManager {
        SessionHistoryManager::new(temp_dir()).unwrap()
    }

    fn write_session(
        mgr: &SessionHistoryManager,
        session_id: &Uuid,
        model: &str,
        messages: Vec<Message>,
    ) {
        let state_mgr = StateManager::with_sessions_dir(mgr.sessions_dir.clone()).unwrap();
        let metadata = SessionPersistMetadata {
            model: model.to_string(),
            ..Default::default()
        };
        state_mgr
            .save_session(session_id, &messages, &metadata)
            .unwrap();
    }

    fn sample_messages() -> Vec<Message> {
        vec![
            Message {
                role: "user".into(),
                content: MessageContent::Text("Implement a parser for CSV files".into()),
            },
            Message {
                role: "assistant".into(),
                content: MessageContent::Text("I'll help you build a CSV parser.".into()),
            },
            Message {
                role: "user".into(),
                content: MessageContent::Text("Add error handling".into()),
            },
        ]
    }

    fn sample_messages_with_tools() -> Vec<Message> {
        vec![
            Message {
                role: "user".into(),
                content: MessageContent::Text("Read the config file".into()),
            },
            Message {
                role: "assistant".into(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::ToolUse {
                        id: "tool_1".into(),
                        name: "file_read".into(),
                        input: serde_json::json!({"file_path": "/tmp/config.toml"}),
                    },
                ]),
            },
            Message {
                role: "user".into(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::ToolResult {
                        tool_use_id: "tool_1".into(),
                        content: Some(crate::api::ToolResultContent::Single(
                            "key = value".into(),
                        )),
                        is_error: Some(false),
                    },
                ]),
            },
        ]
    }

    // -- tests --

    #[test]
    fn test_manager_creation() {
        let dir = temp_dir();
        let mgr = SessionHistoryManager::new(dir.clone()).unwrap();
        assert!(dir.exists());
        assert_eq!(mgr.max_sessions, 100);
    }

    #[test]
    fn test_manager_with_config() {
        let dir = temp_dir();
        let mgr =
            SessionHistoryManager::with_config(dir, 10, Duration::from_secs(60)).unwrap();
        assert_eq!(mgr.max_sessions, 10);
        assert_eq!(mgr.max_session_age, Duration::from_secs(60));
    }

    #[test]
    fn test_list_sessions_empty() {
        let mgr = manager();
        let entries = mgr.list_sessions(&SessionFilter::default()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_list_sessions_populated() {
        let mgr = manager();
        let id = Uuid::new_v4();
        write_session(&mgr, &id, "claude-3-5-sonnet-20241022", sample_messages());

        let entries = mgr.list_sessions(&SessionFilter::default()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].session_id, id);
        assert_eq!(entries[0].model, "claude-3-5-sonnet-20241022");
        assert_eq!(entries[0].message_count, 3);
    }

    #[test]
    fn test_list_sessions_sorted_by_created_at() {
        let mgr = manager();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        write_session(&mgr, &id1, "model-a", sample_messages());
        // Small sleep to ensure different timestamps
        std::thread::sleep(std::time::Duration::from_millis(10));
        write_session(&mgr, &id2, "model-b", sample_messages());

        let filter = SessionFilter {
            sort_by: SessionSortField::CreatedAt,
            sort_order: SortOrder::Ascending,
            ..Default::default()
        };
        let entries = mgr.list_sessions(&filter).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].session_id, id1);
        assert_eq!(entries[1].session_id, id2);
    }

    #[test]
    fn test_list_sessions_with_limit() {
        let mgr = manager();
        for _ in 0..5 {
            let id = Uuid::new_v4();
            write_session(&mgr, &id, "model", sample_messages());
        }

        let filter = SessionFilter {
            limit: 3,
            ..Default::default()
        };
        let entries = mgr.list_sessions(&filter).unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_get_session_found() {
        let mgr = manager();
        let id = Uuid::new_v4();
        write_session(&mgr, &id, "model", sample_messages());

        let entry = mgr.get_session(&id).unwrap().unwrap();
        assert_eq!(entry.session_id, id);
        assert_eq!(entry.message_count, 3);
    }

    #[test]
    fn test_get_session_not_found() {
        let mgr = manager();
        let result = mgr.get_session(&Uuid::new_v4()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_search_sessions() {
        let mgr = manager();
        let id = Uuid::new_v4();
        write_session(&mgr, &id, "model", sample_messages());

        let results = mgr.search_sessions("CSV parser").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].session_id, id);

        let no_results = mgr.search_sessions("nonexistent query xyz").unwrap();
        assert!(no_results.is_empty());
    }

    #[test]
    fn test_delete_session() {
        let mgr = manager();
        let id = Uuid::new_v4();
        write_session(&mgr, &id, "model", sample_messages());

        mgr.delete_session(&id).unwrap();
        assert!(!mgr.session_file_path(&id).exists());

        let entries = mgr.list_sessions(&SessionFilter::default()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_delete_session_not_found() {
        let mgr = manager();
        let result = mgr.delete_session(&Uuid::new_v4());
        assert!(result.is_err());
    }

    #[test]
    fn test_archive_session() {
        let mgr = manager();
        let id = Uuid::new_v4();
        write_session(&mgr, &id, "model", sample_messages());

        mgr.archive_session(&id).unwrap();

        // Original file should be gone.
        assert!(!mgr.session_file_path(&id).exists());

        // File should exist under archived/.
        let archived = mgr.sessions_dir.join("archived").join(format!("{id}.json"));
        assert!(archived.exists());

        // Should not appear in normal listing.
        let entries = mgr.list_sessions(&SessionFilter::default()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_resume_session() {
        let mgr = manager();
        let id = Uuid::new_v4();
        write_session(&mgr, &id, "claude-3-5-sonnet-20241022", sample_messages());

        let info = mgr.resume_session(&id).unwrap();
        assert_eq!(info.session_id, id);
        assert_eq!(info.messages.len(), 3);
        assert_eq!(info.cost_tracker.model_name, "claude-3-5-sonnet-20241022");
        assert_eq!(info.metadata.model, "claude-3-5-sonnet-20241022");
    }

    #[test]
    fn test_resume_session_with_tools() {
        let mgr = manager();
        let id = Uuid::new_v4();
        write_session(&mgr, &id, "model", sample_messages_with_tools());

        let info = mgr.resume_session(&id).unwrap();
        assert!(info.metadata.tools_used.contains(&"file_read".to_string()));
        assert!(info.metadata.files_accessed.contains(&"/tmp/config.toml".to_string()));
    }

    #[test]
    fn test_generate_title_from_messages() {
        let msgs = sample_messages();
        let title = generate_title_from_messages(&msgs);
        assert_eq!(title, "Implement a parser for CSV files");
    }

    #[test]
    fn test_generate_title_from_messages_long() {
        let msgs = vec![Message {
            role: "user".into(),
            content: MessageContent::Text("A".repeat(200)),
        }];
        let title = generate_title_from_messages(&msgs);
        assert!(title.len() <= 63); // 60 chars + "..."
        assert!(title.ends_with("..."));
    }

    #[test]
    fn test_generate_title_from_messages_empty() {
        let title = generate_title_from_messages(&[]);
        assert_eq!(title, "Untitled session");
    }

    #[test]
    fn test_generate_title_via_manager() {
        let mgr = manager();
        let id = Uuid::new_v4();
        let msgs = sample_messages();
        let title = mgr.generate_title(&id, &msgs).unwrap();
        assert_eq!(title, "Implement a parser for CSV files");
    }

    #[test]
    fn test_get_session_messages() {
        let mgr = manager();
        let id = Uuid::new_v4();
        write_session(&mgr, &id, "model", sample_messages());

        let loaded = mgr.get_session_messages(&id).unwrap();
        assert_eq!(loaded.len(), 3);
    }

    #[test]
    fn test_cleanup_old_sessions() {
        let mgr = SessionHistoryManager::with_config(
            temp_dir(),
            2,
            Duration::from_secs(0), // everything is "old"
        )
        .unwrap();

        for _ in 0..4 {
            let id = Uuid::new_v4();
            write_session(&mgr, &id, "model", sample_messages());
        }

        let removed = mgr.cleanup_old_sessions().unwrap();
        assert!(removed > 0);

        // After cleanup, at most max_sessions remain.
        let remaining = mgr.list_sessions(&SessionFilter::default()).unwrap();
        assert!(remaining.len() <= 2);
    }

    #[test]
    fn test_entry_file_size() {
        let mgr = manager();
        let id = Uuid::new_v4();
        write_session(&mgr, &id, "model", sample_messages());

        let entry = mgr.get_session(&id).unwrap().unwrap();
        assert!(entry.file_size_bytes > 0);
    }
}
