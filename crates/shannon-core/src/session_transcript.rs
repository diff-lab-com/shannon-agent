//! # Session Transcript
//!
//! Persistent storage and search of conversation transcripts. Each transcript
//! entry captures a single message exchange (role, content, timestamp, optional
//! tool calls, and arbitrary metadata). Entries are stored as JSON Lines files
//! under `~/.shannon/transcripts/<session_id>.jsonl`.
//!
//! ## Architecture
//!
//! - [`TranscriptEntry`]: A single conversation turn
//! - [`TranscriptRole`]: Discriminator for user / assistant / system / tool
//! - [`TranscriptStore`]: Disk-backed JSONL storage
//! - [`TranscriptSearch`]: Full-text search across sessions
//! - [`TranscriptQuery`]: Filtered query builder
//! - [`TranscriptStats`]: Per-session and global statistics
//!
//! ## Usage
//!
//! ```rust,no_run
//! use shannon_core::session_transcript::{TranscriptStore, TranscriptEntry, TranscriptRole};
//!
//! let store = TranscriptStore::new_in_dir("/tmp/transcripts".into()).unwrap();
//! let entry = TranscriptEntry::new(TranscriptRole::User, "hello".into(), "session-1".into());
//! store.append_entry(&entry).unwrap();
//!
//! let results = store.search_text("hello").unwrap();
//! assert_eq!(results.len(), 1);
//! ```

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tracing::{debug, info};
use uuid::Uuid;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during transcript operations.
#[derive(Error, Debug)]
pub enum TranscriptError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("Store not initialized")]
    NotInitialized,
}

// ============================================================================
// Data Types
// ============================================================================

/// The role of a participant in the conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TranscriptRole {
    User,
    Assistant,
    System,
    Tool,
}

impl std::fmt::Display for TranscriptRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Assistant => write!(f, "assistant"),
            Self::System => write!(f, "system"),
            Self::Tool => write!(f, "tool"),
        }
    }
}

impl From<&str> for TranscriptRole {
    fn from(s: &str) -> Self {
        match s {
            "assistant" | "model" => Self::Assistant,
            "system" => Self::System,
            "tool" => Self::Tool,
            _ => Self::User,
        }
    }
}

/// A single transcript entry representing one message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    /// Unique identifier for this entry.
    pub id: Uuid,
    /// The role of the message author.
    pub role: TranscriptRole,
    /// The text content of the message.
    pub content: String,
    /// When the message was created.
    pub timestamp: DateTime<Utc>,
    /// The session this entry belongs to.
    pub session_id: String,
    /// Optional list of tool calls invoked during this turn.
    pub tool_calls: Vec<ToolCallRecord>,
    /// Arbitrary key-value metadata.
    pub metadata: HashMap<String, Value>,
}

/// A record of a single tool invocation within a transcript entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    /// The tool name.
    pub name: String,
    /// The input arguments as JSON.
    pub input: Value,
    /// Whether the tool call succeeded.
    pub success: bool,
    /// Duration in milliseconds, if measured.
    pub duration_ms: Option<u64>,
}

impl TranscriptEntry {
    /// Create a new transcript entry with the given role, content, and session ID.
    pub fn new(role: TranscriptRole, content: String, session_id: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            role,
            content,
            timestamp: Utc::now(),
            session_id,
            tool_calls: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Create a new entry with a custom timestamp.
    pub fn with_timestamp(
        role: TranscriptRole,
        content: String,
        session_id: String,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            role,
            content,
            timestamp,
            session_id,
            tool_calls: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Add a tool call record to this entry.
    pub fn with_tool_call(mut self, call: ToolCallRecord) -> Self {
        self.tool_calls.push(call);
        self
    }

    /// Add metadata to this entry.
    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

// ============================================================================
// Transcript Query
// ============================================================================

/// Filter criteria for transcript searches.
#[derive(Debug, Clone)]
pub struct TranscriptQuery {
    /// Filter by session ID.
    pub session_id: Option<String>,
    /// Filter by role.
    pub role: Option<TranscriptRole>,
    /// Only include entries after this timestamp.
    pub date_after: Option<DateTime<Utc>>,
    /// Only include entries before this timestamp.
    pub date_before: Option<DateTime<Utc>>,
    /// Content pattern (case-insensitive substring match).
    pub content_pattern: Option<String>,
    /// Maximum number of results.
    pub limit: usize,
}

impl Default for TranscriptQuery {
    fn default() -> Self {
        Self {
            session_id: None,
            role: None,
            date_after: None,
            date_before: None,
            content_pattern: None,
            limit: 100,
        }
    }
}

impl TranscriptQuery {
    /// Create a new query builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by session ID.
    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    /// Filter by role.
    pub fn role(mut self, role: TranscriptRole) -> Self {
        self.role = Some(role);
        self
    }

    /// Filter to entries after this timestamp.
    pub fn after(mut self, ts: DateTime<Utc>) -> Self {
        self.date_after = Some(ts);
        self
    }

    /// Filter to entries before this timestamp.
    pub fn before(mut self, ts: DateTime<Utc>) -> Self {
        self.date_before = Some(ts);
        self
    }

    /// Filter by content pattern (case-insensitive).
    pub fn content_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.content_pattern = Some(pattern.into());
        self
    }

    /// Set maximum results.
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Check if an entry matches all query filters.
    pub fn matches(&self, entry: &TranscriptEntry) -> bool {
        if let Some(ref sid) = self.session_id {
            if &entry.session_id != sid {
                return false;
            }
        }
        if let Some(role) = self.role {
            if entry.role != role {
                return false;
            }
        }
        if let Some(after) = self.date_after {
            if entry.timestamp < after {
                return false;
            }
        }
        if let Some(before) = self.date_before {
            if entry.timestamp > before {
                return false;
            }
        }
        if let Some(ref pattern) = self.content_pattern {
            if !entry.content.to_lowercase().contains(&pattern.to_lowercase()) {
                return false;
            }
        }
        true
    }
}

// ============================================================================
// Transcript Stats
// ============================================================================

/// Statistics for a single session's transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTranscriptStats {
    pub session_id: String,
    pub total_entries: usize,
    pub user_entries: usize,
    pub assistant_entries: usize,
    pub tool_entries: usize,
    pub system_entries: usize,
    pub total_tool_calls: usize,
    pub first_entry: Option<DateTime<Utc>>,
    pub last_entry: Option<DateTime<Utc>>,
    pub total_chars: usize,
}

/// Global statistics across all transcript sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalTranscriptStats {
    pub total_sessions: usize,
    pub total_entries: usize,
    pub total_tool_calls: usize,
    pub total_chars: usize,
    pub oldest_entry: Option<DateTime<Utc>>,
    pub newest_entry: Option<DateTime<Utc>>,
    pub session_stats: HashMap<String, SessionTranscriptStats>,
}

// ============================================================================
// Transcript Store
// ============================================================================

/// Persistent JSONL-backed transcript store.
///
/// Each session is stored as a `.jsonl` file under the transcripts directory.
/// One JSON object per line, with entries appended in chronological order.
pub struct TranscriptStore {
    transcripts_dir: PathBuf,
}

impl TranscriptStore {
    /// Create a new store backed by the given directory.
    ///
    /// The directory is created if it does not exist.
    pub fn new_in_dir(transcripts_dir: PathBuf) -> Result<Self, TranscriptError> {
        fs::create_dir_all(&transcripts_dir)?;
        Ok(Self { transcripts_dir })
    }

    /// Create a store in the default location (`~/.shannon/transcripts/`).
    pub fn new() -> Result<Self, TranscriptError> {
        let base = dirs::home_dir()
            .ok_or(TranscriptError::NotInitialized)?;
        let dir = base.join(".shannon").join("transcripts");
        Self::new_in_dir(dir)
    }

    /// Get the file path for a session.
    fn session_path(&self, session_id: &str) -> PathBuf {
        self.transcripts_dir.join(format!("{session_id}.jsonl"))
    }

    /// Append a transcript entry to its session file.
    pub fn append_entry(&self, entry: &TranscriptEntry) -> Result<(), TranscriptError> {
        let path = self.session_path(&entry.session_id);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let line = serde_json::to_string(entry)
            .map_err(|e| TranscriptError::Serialization(e.to_string()))?;
        writeln!(file, "{line}")?;
        debug!(
            session_id = %entry.session_id,
            entry_id = %entry.id,
            "Appended transcript entry"
        );
        Ok(())
    }

    /// Read all entries for a given session.
    pub fn get_session(&self, session_id: &str) -> Result<Vec<TranscriptEntry>, TranscriptError> {
        let path = self.session_path(session_id);
        if !path.exists() {
            return Err(TranscriptError::SessionNotFound(session_id.to_string()));
        }

        let file = fs::File::open(&path)?;
        let reader = std::io::BufReader::new(file);
        let mut entries = Vec::new();

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<TranscriptEntry>(trimmed) {
                Ok(entry) => entries.push(entry),
                Err(_) => continue,
            }
        }

        // Entries are already in chronological order since we append.
        Ok(entries)
    }

    /// Delete all entries for a session.
    pub fn delete_session(&self, session_id: &str) -> Result<(), TranscriptError> {
        let path = self.session_path(session_id);
        if !path.exists() {
            return Err(TranscriptError::SessionNotFound(session_id.to_string()));
        }
        fs::remove_file(&path)?;
        info!(session_id = %session_id, "Deleted transcript session");
        Ok(())
    }

    /// List all session IDs in the store.
    pub fn list_sessions(&self) -> Result<Vec<String>, TranscriptError> {
        if !self.transcripts_dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        for entry in fs::read_dir(&self.transcripts_dir)? {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    sessions.push(stem.to_string());
                }
            }
        }

        Ok(sessions)
    }

    /// Search entries across all sessions using a query.
    pub fn search(&self, query: &TranscriptQuery) -> Result<Vec<TranscriptEntry>, TranscriptError> {
        let sessions = if let Some(ref sid) = query.session_id {
            vec![sid.clone()]
        } else {
            self.list_sessions()?
        };

        let mut results = Vec::new();

        for session_id in &sessions {
            let entries = match self.get_session(session_id) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in entries {
                if query.matches(&entry) {
                    results.push(entry);
                    if results.len() >= query.limit {
                        return Ok(results);
                    }
                }
            }
        }

        Ok(results)
    }

    /// Full-text search across all sessions using a simple string pattern.
    pub fn search_text(&self, pattern: &str) -> Result<Vec<TranscriptEntry>, TranscriptError> {
        let query = TranscriptQuery {
            content_pattern: Some(pattern.to_string()),
            limit: 100,
            ..Default::default()
        };
        self.search(&query)
    }

    /// Fuzzy search: matches entries where the content contains any of the
    /// given words (case-insensitive).
    pub fn fuzzy_search(
        &self,
        terms: &[&str],
    ) -> Result<Vec<TranscriptEntry>, TranscriptError> {
        if terms.is_empty() {
            return Ok(Vec::new());
        }

        let terms_lower: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
        let all_entries = self.search(&TranscriptQuery {
            limit: 10_000,
            ..Default::default()
        })?;

        let results: Vec<TranscriptEntry> = all_entries
            .into_iter()
            .filter(|entry| {
                let content_lower = entry.content.to_lowercase();
                terms_lower.iter().all(|term| content_lower.contains(term))
            })
            .collect();

        Ok(results)
    }

    /// Compute statistics for a single session.
    pub fn session_stats(&self, session_id: &str) -> Result<SessionTranscriptStats, TranscriptError> {
        let entries = self.get_session(session_id)?;

        let mut stats = SessionTranscriptStats {
            session_id: session_id.to_string(),
            total_entries: entries.len(),
            user_entries: 0,
            assistant_entries: 0,
            tool_entries: 0,
            system_entries: 0,
            total_tool_calls: 0,
            first_entry: None,
            last_entry: None,
            total_chars: 0,
        };

        for entry in &entries {
            match entry.role {
                TranscriptRole::User => stats.user_entries += 1,
                TranscriptRole::Assistant => stats.assistant_entries += 1,
                TranscriptRole::Tool => stats.tool_entries += 1,
                TranscriptRole::System => stats.system_entries += 1,
            }
            stats.total_tool_calls += entry.tool_calls.len();
            stats.total_chars += entry.content.len();

            match (stats.first_entry, stats.last_entry) {
                (None, None) => {
                    stats.first_entry = Some(entry.timestamp);
                    stats.last_entry = Some(entry.timestamp);
                }
                _ => {
                    if entry.timestamp < stats.first_entry.expect("first_entry set in None case") {
                        stats.first_entry = Some(entry.timestamp);
                    }
                    if entry.timestamp > stats.last_entry.expect("last_entry set in None case") {
                        stats.last_entry = Some(entry.timestamp);
                    }
                }
            }
        }

        Ok(stats)
    }

    /// Compute global statistics across all sessions.
    pub fn stats(&self) -> Result<GlobalTranscriptStats, TranscriptError> {
        let sessions = self.list_sessions()?;
        let mut global = GlobalTranscriptStats {
            total_sessions: sessions.len(),
            total_entries: 0,
            total_tool_calls: 0,
            total_chars: 0,
            oldest_entry: None,
            newest_entry: None,
            session_stats: HashMap::new(),
        };

        for session_id in &sessions {
            let s = self.session_stats(session_id)?;
            global.total_entries += s.total_entries;
            global.total_tool_calls += s.total_tool_calls;
            global.total_chars += s.total_chars;

            if let Some(first) = s.first_entry {
                match global.oldest_entry {
                    None => global.oldest_entry = Some(first),
                    Some(oldest) if first < oldest => global.oldest_entry = Some(first),
                    _ => {}
                }
            }
            if let Some(last) = s.last_entry {
                match global.newest_entry {
                    None => global.newest_entry = Some(last),
                    Some(newest) if last > newest => global.newest_entry = Some(last),
                    _ => {}
                }
            }

            global
                .session_stats
                .insert(session_id.clone(), s);
        }

        Ok(global)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("shannon-test-transcripts")
            .join(Uuid::new_v4().to_string());
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn store() -> TranscriptStore {
        TranscriptStore::new_in_dir(temp_dir()).unwrap()
    }

    fn make_entry(role: TranscriptRole, content: &str, session_id: &str) -> TranscriptEntry {
        TranscriptEntry::new(role, content.to_string(), session_id.to_string())
    }

    // -----------------------------------------------------------------------
    // TranscriptEntry tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_entry_creation() {
        let entry = make_entry(TranscriptRole::User, "hello", "s1");
        assert_eq!(entry.role, TranscriptRole::User);
        assert_eq!(entry.content, "hello");
        assert_eq!(entry.session_id, "s1");
        assert!(entry.tool_calls.is_empty());
        assert!(entry.metadata.is_empty());
    }

    #[test]
    fn test_entry_with_timestamp() {
        let ts = Utc::now();
        let entry = TranscriptEntry::with_timestamp(
            TranscriptRole::Assistant,
            "hi".to_string(),
            "s1".to_string(),
            ts,
        );
        assert_eq!(entry.timestamp, ts);
    }

    #[test]
    fn test_entry_with_tool_call() {
        let entry = make_entry(TranscriptRole::Assistant, "reading file", "s1")
            .with_tool_call(ToolCallRecord {
                name: "file_read".into(),
                input: serde_json::json!({"path": "/tmp/test.txt"}),
                success: true,
                duration_ms: Some(42),
            });
        assert_eq!(entry.tool_calls.len(), 1);
        assert_eq!(entry.tool_calls[0].name, "file_read");
        assert!(entry.tool_calls[0].success);
    }

    #[test]
    fn test_entry_with_metadata() {
        let entry = make_entry(TranscriptRole::User, "hello", "s1")
            .with_metadata("token_count", serde_json::json!(50));
        assert_eq!(entry.metadata.get("token_count").unwrap(), 50);
    }

    #[test]
    fn test_role_display() {
        assert_eq!(TranscriptRole::User.to_string(), "user");
        assert_eq!(TranscriptRole::Assistant.to_string(), "assistant");
        assert_eq!(TranscriptRole::System.to_string(), "system");
        assert_eq!(TranscriptRole::Tool.to_string(), "tool");
    }

    #[test]
    fn test_role_from_str() {
        assert_eq!(TranscriptRole::from("user"), TranscriptRole::User);
        assert_eq!(TranscriptRole::from("assistant"), TranscriptRole::Assistant);
        assert_eq!(TranscriptRole::from("model"), TranscriptRole::Assistant);
        assert_eq!(TranscriptRole::from("system"), TranscriptRole::System);
        assert_eq!(TranscriptRole::from("tool"), TranscriptRole::Tool);
        assert_eq!(TranscriptRole::from("unknown"), TranscriptRole::User);
    }

    #[test]
    fn test_entry_serialization_roundtrip() {
        let entry = make_entry(TranscriptRole::User, "hello world", "s1")
            .with_metadata("key", serde_json::json!("value"));
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: TranscriptEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, entry.id);
        assert_eq!(parsed.role, entry.role);
        assert_eq!(parsed.content, entry.content);
        assert_eq!(parsed.session_id, entry.session_id);
    }

    // -----------------------------------------------------------------------
    // TranscriptQuery tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_query_defaults() {
        let q = TranscriptQuery::default();
        assert!(q.session_id.is_none());
        assert!(q.role.is_none());
        assert!(q.content_pattern.is_none());
        assert_eq!(q.limit, 100);
    }

    #[test]
    fn test_query_builder() {
        let q = TranscriptQuery::new()
            .session_id("s1")
            .role(TranscriptRole::User)
            .limit(10);
        assert_eq!(q.session_id.as_deref(), Some("s1"));
        assert_eq!(q.role, Some(TranscriptRole::User));
        assert_eq!(q.limit, 10);
    }

    #[test]
    fn test_query_matches() {
        let entry = make_entry(TranscriptRole::User, "Hello World", "s1");
        let q = TranscriptQuery::new()
            .session_id("s1")
            .role(TranscriptRole::User)
            .content_pattern("hello");
        assert!(q.matches(&entry));
    }

    #[test]
    fn test_query_matches_session_mismatch() {
        let entry = make_entry(TranscriptRole::User, "Hello World", "s1");
        let q = TranscriptQuery::new().session_id("s2");
        assert!(!q.matches(&entry));
    }

    #[test]
    fn test_query_matches_role_mismatch() {
        let entry = make_entry(TranscriptRole::User, "Hello World", "s1");
        let q = TranscriptQuery::new().role(TranscriptRole::Assistant);
        assert!(!q.matches(&entry));
    }

    #[test]
    fn test_query_matches_pattern_mismatch() {
        let entry = make_entry(TranscriptRole::User, "Hello World", "s1");
        let q = TranscriptQuery::new().content_pattern("goodbye");
        assert!(!q.matches(&entry));
    }

    #[test]
    fn test_query_date_filter() {
        let now = Utc::now();
        let entry = TranscriptEntry::with_timestamp(
            TranscriptRole::User,
            "test".into(),
            "s1".into(),
            now,
        );

        let q = TranscriptQuery::new().after(now - chrono::Duration::seconds(1));
        assert!(q.matches(&entry));

        let q = TranscriptQuery::new().before(now + chrono::Duration::seconds(1));
        assert!(q.matches(&entry));

        let q = TranscriptQuery::new().after(now + chrono::Duration::seconds(1));
        assert!(!q.matches(&entry));
    }

    // -----------------------------------------------------------------------
    // TranscriptStore tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_store_creation() {
        let dir = temp_dir();
        let s = TranscriptStore::new_in_dir(dir.clone()).unwrap();
        assert!(dir.exists());
        assert_eq!(s.transcripts_dir, dir);
    }

    #[test]
    fn test_append_and_get_session() {
        let s = store();
        let e1 = make_entry(TranscriptRole::User, "hello", "s1");
        let e2 = make_entry(TranscriptRole::Assistant, "hi there", "s1");
        s.append_entry(&e1).unwrap();
        s.append_entry(&e2).unwrap();

        let entries = s.get_session("s1").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].content, "hello");
        assert_eq!(entries[1].content, "hi there");
    }

    #[test]
    fn test_get_session_not_found() {
        let s = store();
        let result = s.get_session("nonexistent");
        assert!(result.is_err());
        match result.unwrap_err() {
            TranscriptError::SessionNotFound(id) => assert_eq!(id, "nonexistent"),
            other => panic!("Expected SessionNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn test_delete_session() {
        let s = store();
        s.append_entry(&make_entry(TranscriptRole::User, "hi", "s1"))
            .unwrap();
        assert!(s.session_path("s1").exists());

        s.delete_session("s1").unwrap();
        assert!(!s.session_path("s1").exists());
    }

    #[test]
    fn test_delete_session_not_found() {
        let s = store();
        let result = s.delete_session("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_sessions() {
        let s = store();
        s.append_entry(&make_entry(TranscriptRole::User, "a", "s1"))
            .unwrap();
        s.append_entry(&make_entry(TranscriptRole::User, "b", "s2"))
            .unwrap();

        let sessions = s.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.contains(&"s1".to_string()));
        assert!(sessions.contains(&"s2".to_string()));
    }

    #[test]
    fn test_list_sessions_empty() {
        let s = store();
        let sessions = s.list_sessions().unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_search_by_content() {
        let s = store();
        s.append_entry(&make_entry(TranscriptRole::User, "implement CSV parser", "s1"))
            .unwrap();
        s.append_entry(&make_entry(TranscriptRole::Assistant, "I'll build a parser", "s1"))
            .unwrap();
        s.append_entry(&make_entry(TranscriptRole::User, "unrelated message", "s2"))
            .unwrap();

        let results = s.search_text("CSV").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].session_id, "s1");
    }

    #[test]
    fn test_search_by_query() {
        let s = store();
        s.append_entry(&make_entry(TranscriptRole::User, "hello", "s1"))
            .unwrap();
        s.append_entry(&make_entry(TranscriptRole::Assistant, "world", "s1"))
            .unwrap();
        s.append_entry(&make_entry(TranscriptRole::User, "hello again", "s2"))
            .unwrap();

        let q = TranscriptQuery::new()
            .role(TranscriptRole::User)
            .content_pattern("hello")
            .limit(10);
        let results = s.search(&q).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_with_limit() {
        let s = store();
        for i in 0..20 {
            s.append_entry(&make_entry(
                TranscriptRole::User,
                &format!("message {i}"),
                "s1",
            ))
            .unwrap();
        }

        let results = s.search_text("message").unwrap();
        assert_eq!(results.len(), 20);

        let q = TranscriptQuery::new().content_pattern("message").limit(5);
        let limited = s.search(&q).unwrap();
        assert_eq!(limited.len(), 5);
    }

    #[test]
    fn test_fuzzy_search() {
        let s = store();
        s.append_entry(&make_entry(
            TranscriptRole::User,
            "implement a CSV parser in Rust",
            "s1",
        ))
        .unwrap();
        s.append_entry(&make_entry(
            TranscriptRole::User,
            "fix the bug in the parser",
            "s2",
        ))
        .unwrap();
        s.append_entry(&make_entry(
            TranscriptRole::User,
            "write documentation",
            "s3",
        ))
        .unwrap();

        let results = s.fuzzy_search(&["csv", "rust"]).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].session_id, "s1");
    }

    #[test]
    fn test_fuzzy_search_empty_terms() {
        let s = store();
        let results = s.fuzzy_search(&[]).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_fuzzy_search_no_results() {
        let s = store();
        s.append_entry(&make_entry(TranscriptRole::User, "hello world", "s1"))
            .unwrap();
        let results = s.fuzzy_search(&["nonexistent"]).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_session_stats() {
        let s = store();
        s.append_entry(&make_entry(TranscriptRole::User, "hello", "s1"))
            .unwrap();
        s.append_entry(&make_entry(TranscriptRole::Assistant, "hi", "s1"))
            .unwrap();
        s.append_entry(&make_entry(TranscriptRole::System, "sys", "s1"))
            .unwrap();

        let stats = s.session_stats("s1").unwrap();
        assert_eq!(stats.total_entries, 3);
        assert_eq!(stats.user_entries, 1);
        assert_eq!(stats.assistant_entries, 1);
        assert_eq!(stats.system_entries, 1);
        assert!(stats.first_entry.is_some());
        assert!(stats.last_entry.is_some());
    }

    #[test]
    fn test_session_stats_with_tool_calls() {
        let s = store();
        let entry = make_entry(TranscriptRole::Assistant, "reading file", "s1")
            .with_tool_call(ToolCallRecord {
                name: "file_read".into(),
                input: serde_json::json!({"path": "/tmp/test"}),
                success: true,
                duration_ms: None,
            });
        s.append_entry(&entry).unwrap();

        let stats = s.session_stats("s1").unwrap();
        assert_eq!(stats.total_tool_calls, 1);
    }

    #[test]
    fn test_global_stats() {
        let s = store();
        s.append_entry(&make_entry(TranscriptRole::User, "hello", "s1"))
            .unwrap();
        s.append_entry(&make_entry(TranscriptRole::User, "world", "s2"))
            .unwrap();

        let global = s.stats().unwrap();
        assert_eq!(global.total_sessions, 2);
        assert_eq!(global.total_entries, 2);
        assert_eq!(global.session_stats.len(), 2);
        assert!(global.oldest_entry.is_some());
        assert!(global.newest_entry.is_some());
    }

    #[test]
    fn test_global_stats_empty() {
        let s = store();
        let global = s.stats().unwrap();
        assert_eq!(global.total_sessions, 0);
        assert_eq!(global.total_entries, 0);
    }

    #[test]
    fn test_multiple_sessions_independent() {
        let s = store();
        s.append_entry(&make_entry(TranscriptRole::User, "a", "s1"))
            .unwrap();
        s.append_entry(&make_entry(TranscriptRole::User, "b", "s2"))
            .unwrap();

        let s1 = s.get_session("s1").unwrap();
        let s2 = s.get_session("s2").unwrap();
        assert_eq!(s1.len(), 1);
        assert_eq!(s2.len(), 1);
        assert_eq!(s1[0].content, "a");
        assert_eq!(s2[0].content, "b");
    }

    #[test]
    fn test_persistence_across_reloads() {
        let dir = temp_dir();
        {
            let s = TranscriptStore::new_in_dir(dir.clone()).unwrap();
            s.append_entry(&make_entry(TranscriptRole::User, "persistent", "s1"))
                .unwrap();
        }
        // Reload store from same directory.
        let s = TranscriptStore::new_in_dir(dir).unwrap();
        let entries = s.get_session("s1").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "persistent");
    }

    #[test]
    fn test_corrupted_lines_skipped() {
        let s = store();
        s.append_entry(&make_entry(TranscriptRole::User, "valid", "s1"))
            .unwrap();

        // Append a corrupted line.
        let path = s.session_path("s1");
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"this is not json\n")
            .unwrap();

        let entries = s.get_session("s1").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "valid");
    }

    #[test]
    fn test_empty_lines_skipped() {
        let s = store();
        s.append_entry(&make_entry(TranscriptRole::User, "valid", "s1"))
            .unwrap();

        let path = s.session_path("s1");
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"\n\n\n")
            .unwrap();

        let entries = s.get_session("s1").unwrap();
        assert_eq!(entries.len(), 1);
    }
}
