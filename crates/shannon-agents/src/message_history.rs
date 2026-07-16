//! Append-only JSONL history of all inter-agent messages.
//!
//! Mirrors the design of `scheduled_runs.rs`: append-only JSONL files
//! partitioned by team and date, with last-write-wins on `revision`
//! (currently always 0 — messages are immutable once recorded).
//!
//! ## Layout
//! ```text
//! ~/.shannon/agent-messages/
//! ├── <team_name>/
//! │   ├── 2026-06-13.jsonl
//! │   └── 2026-06-12.jsonl
//! ```
//!
//! ## Rolling
//! Lines older than `rolling_days` (default 30 — messages are higher-volume
//! than runs) are pruned via [`MessageHistoryStore::prune_old`].
//!
//! ## Why a separate store from inboxes?
//! `FilePersistence::deliver_message` writes to a per-agent inbox that is
//! cleared on `read_inbox`. That's a delivery queue, not a history log.
//! The desktop UI needs an append-only audit trail of every message,
//! independent of whether the recipient has consumed it.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};

/// Errors returned by the history store.
#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("invalid path component: {0}")]
    InvalidPath(String),
}

/// Kind of message content recorded in history (mirror of `MessageContent`
/// reduced to a stable string for cross-version readability).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Text,
    Structured,
    Protocol,
}

impl ContentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Structured => "structured",
            Self::Protocol => "protocol",
        }
    }
}

/// A single recorded message in history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRecord {
    /// Unique message ID (UUID v4 string).
    pub message_id: String,
    /// Team the message belongs to (`"<adhoc>"` when no team context).
    pub team: String,
    /// Sender agent name.
    pub from: String,
    /// Recipient agent name, or `"*"` for broadcast.
    pub to: String,
    /// Truncated content preview (max ~200 chars) for log readability.
    pub content_preview: String,
    /// Kind of original content (text/structured/protocol).
    pub content_kind: ContentKind,
    /// Priority label (`low`/`normal`/`high`/`critical`).
    pub priority: String,
    /// When the message was sent.
    pub timestamp: DateTime<Utc>,
    /// Revision (always 0 for now — reserved for future edits).
    #[serde(default)]
    pub revision: u32,
}

impl MessageRecord {
    /// Maximum length of `content_preview`. Longer content is truncated with `…`.
    pub const PREVIEW_MAX: usize = 200;

    /// Build a preview string, truncating with an ellipsis if too long.
    pub fn truncate_preview(s: &str) -> String {
        if s.chars().count() <= Self::PREVIEW_MAX {
            s.to_string()
        } else {
            let truncated: String = s.chars().take(Self::PREVIEW_MAX - 1).collect();
            format!("{truncated}…")
        }
    }
}

/// Append-only JSONL agent message history.
#[derive(Debug, Clone)]
pub struct MessageHistoryStore {
    base_dir: PathBuf,
    /// Rolling window in days. Entries older than this are pruned.
    pub rolling_days: u32,
}

impl MessageHistoryStore {
    /// Default store at `~/.shannon/agent-messages/`.
    pub fn new() -> Self {
        Self {
            base_dir: default_base_dir(),
            rolling_days: 30,
        }
    }

    /// Custom base directory (testing).
    pub fn with_base(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            rolling_days: 30,
        }
    }

    /// Return the base directory path.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Validate team name — no path separators, no traversal.
    fn sanitize_component(name: &str, label: &str) -> Result<(), HistoryError> {
        if name.is_empty() || name == "." || name == ".." {
            return Err(HistoryError::InvalidPath(format!(
                "{label} must not be empty or a directory alias"
            )));
        }
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(HistoryError::InvalidPath(format!(
                "{label} must not contain path separators or traversal"
            )));
        }
        Ok(())
    }

    /// Append a record. Returns the message_id.
    pub fn record(&self, msg: &MessageRecord) -> Result<String, HistoryError> {
        Self::sanitize_component(&msg.team, "team")?;
        let path = self.path_for(&msg.team, msg.timestamp);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        let line = serde_json::to_string(msg)?;
        writeln!(file, "{line}")?;
        Ok(msg.message_id.clone())
    }

    /// Find a single message by ID (returns the latest revision).
    pub fn find_by_id(&self, message_id: &str) -> Result<Option<MessageRecord>, HistoryError> {
        for msg in self.iter_all_teams()? {
            if msg.message_id == message_id {
                return Ok(Some(msg));
            }
        }
        Ok(None)
    }

    /// List messages for a team, most recent first.
    pub fn list_by_team(
        &self,
        team: &str,
        limit: usize,
    ) -> Result<Vec<MessageRecord>, HistoryError> {
        Self::sanitize_component(team, "team")?;
        let mut messages: Vec<_> = self.iter_team(team)?.into_iter().collect();
        messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        messages.truncate(limit);
        Ok(messages)
    }

    /// List messages where the given agent is sender or recipient, most recent first.
    pub fn list_by_agent(
        &self,
        team: &str,
        agent: &str,
        limit: usize,
    ) -> Result<Vec<MessageRecord>, HistoryError> {
        Self::sanitize_component(team, "team")?;
        Self::sanitize_component(agent, "agent")?;
        let mut messages: Vec<_> = self
            .iter_team(team)?
            .into_iter()
            .filter(|m| m.from == agent || m.to == agent || m.to == "*")
            .collect();
        messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        messages.truncate(limit);
        Ok(messages)
    }

    /// List messages in a time range for a team, oldest first.
    pub fn list_by_time_range(
        &self,
        team: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<MessageRecord>, HistoryError> {
        Self::sanitize_component(team, "team")?;
        let mut messages: Vec<_> = self
            .iter_team(team)?
            .into_iter()
            .filter(|m| m.timestamp >= start && m.timestamp < end)
            .collect();
        messages.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        Ok(messages)
    }

    /// Prune entries older than `rolling_days`. Returns count pruned.
    pub fn prune_old(&self) -> Result<usize, HistoryError> {
        let threshold = Utc::now() - chrono::Duration::days(self.rolling_days as i64);
        if !self.base_dir.exists() {
            return Ok(0);
        }
        let mut pruned = 0usize;
        for team_entry in fs::read_dir(&self.base_dir)? {
            let team_entry = team_entry?;
            if !team_entry.file_type()?.is_dir() {
                continue;
            }
            for day_entry in fs::read_dir(team_entry.path())? {
                let day_entry = day_entry?;
                let path = day_entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                pruned += self.prune_file(&path, threshold)?;
            }
        }
        Ok(pruned)
    }

    /// Prune a single file in-place, keeping only entries at or after `threshold`.
    fn prune_file(&self, path: &Path, threshold: DateTime<Utc>) -> Result<usize, HistoryError> {
        let file = match OpenOptions::new().read(true).open(path) {
            Ok(f) => f,
            Err(_) => return Ok(0),
        };
        let reader = BufReader::new(file);
        let mut latest_by_id: HashMap<String, MessageRecord> = HashMap::new();
        let mut pruned = 0usize;
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<MessageRecord>(&line) {
                Ok(msg) => {
                    if msg.timestamp < threshold {
                        pruned += 1;
                    } else {
                        let entry = latest_by_id
                            .entry(msg.message_id.clone())
                            .or_insert(msg.clone());
                        if msg.revision > entry.revision {
                            *entry = msg;
                        }
                    }
                }
                Err(_) => continue,
            }
        }
        if pruned == 0 {
            return Ok(0);
        }
        let kept: Vec<MessageRecord> = latest_by_id.into_values().collect();
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(path)?;
        for msg in &kept {
            let line = serde_json::to_string(msg)?;
            writeln!(file, "{line}")?;
        }
        Ok(pruned)
    }

    /// Iterate all messages for a single team, returning the latest revision per id.
    fn iter_team(&self, team: &str) -> Result<Vec<MessageRecord>, HistoryError> {
        let team_dir = self.base_dir.join(team);
        if !team_dir.exists() {
            return Ok(Vec::new());
        }
        let mut by_id: HashMap<String, MessageRecord> = HashMap::new();
        for day_entry in fs::read_dir(&team_dir)? {
            let day_entry = day_entry?;
            let path = day_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            self.collect_latest(&path, &mut by_id)?;
        }
        Ok(by_id.into_values().collect())
    }

    /// Iterate across all teams (slow — used only by `find_by_id`).
    fn iter_all_teams(&self) -> Result<Vec<MessageRecord>, HistoryError> {
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for team_entry in fs::read_dir(&self.base_dir)? {
            let team_entry = team_entry?;
            if !team_entry.file_type()?.is_dir() {
                continue;
            }
            let team_name = team_entry.file_name().to_string_lossy().into_owned();
            let mut team_msgs = self.iter_team(&team_name)?;
            out.append(&mut team_msgs);
        }
        Ok(out)
    }

    /// Read a file and track the latest revision per message_id.
    fn collect_latest(
        &self,
        path: &Path,
        latest: &mut HashMap<String, MessageRecord>,
    ) -> Result<(), HistoryError> {
        let file = match OpenOptions::new().read(true).open(path) {
            Ok(f) => f,
            Err(_) => return Ok(()),
        };
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(msg) = serde_json::from_str::<MessageRecord>(&line) {
                let entry = latest
                    .entry(msg.message_id.clone())
                    .or_insert_with(|| msg.clone());
                if msg.revision > entry.revision {
                    *entry = msg;
                }
            }
        }
        Ok(())
    }

    /// Compute the JSONL path for a team and timestamp.
    fn path_for(&self, team: &str, ts: DateTime<Utc>) -> PathBuf {
        self.base_dir.join(team).join(format!(
            "{:04}-{:02}-{:02}.jsonl",
            ts.year(),
            ts.month(),
            ts.day()
        ))
    }
}

impl Default for MessageHistoryStore {
    fn default() -> Self {
        Self::new()
    }
}

fn default_base_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".shannon")
        .join("agent-messages")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn sample_record(team: &str, from: &str, to: &str, content: &str) -> MessageRecord {
        MessageRecord {
            message_id: format!("msg-{}", rand_suffix()),
            team: team.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            content_preview: MessageRecord::truncate_preview(content),
            content_kind: ContentKind::Text,
            priority: "normal".to_string(),
            timestamp: Utc::now(),
            revision: 0,
        }
    }

    fn rand_suffix() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("{nanos}")
    }

    #[test]
    fn test_record_and_find() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MessageHistoryStore::with_base(tmp.path().to_path_buf());
        let rec = sample_record("alpha", "alice", "bob", "hi");
        let id = store.record(&rec).unwrap();
        assert_eq!(id, rec.message_id);

        let found = store.find_by_id(&id).unwrap();
        assert!(found.is_some());
        let m = found.unwrap();
        assert_eq!(m.from, "alice");
        assert_eq!(m.to, "bob");
    }

    #[test]
    fn test_list_by_team_sorts_recent_first() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MessageHistoryStore::with_base(tmp.path().to_path_buf());

        // Three records with explicit timestamps to test sort order.
        let mut old = sample_record("t", "a", "b", "old");
        old.timestamp = Utc::now() - chrono::Duration::hours(2);
        store.record(&old).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(5));
        let mut mid = sample_record("t", "a", "b", "mid");
        mid.timestamp = Utc::now() - chrono::Duration::hours(1);
        store.record(&mid).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(5));
        let mut recent = sample_record("t", "a", "b", "recent");
        recent.timestamp = Utc::now();
        store.record(&recent).unwrap();

        let list = store.list_by_team("t", 10).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].content_preview, "recent");
        assert_eq!(list[2].content_preview, "old");
    }

    #[test]
    fn test_list_by_team_respects_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MessageHistoryStore::with_base(tmp.path().to_path_buf());
        for i in 0..5 {
            let mut r = sample_record("t", "a", "b", &format!("m{i}"));
            r.timestamp = Utc::now() + chrono::Duration::seconds(i);
            store.record(&r).unwrap();
        }
        assert_eq!(store.list_by_team("t", 2).unwrap().len(), 2);
    }

    #[test]
    fn test_list_by_agent_includes_sender_recipient_and_broadcast() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MessageHistoryStore::with_base(tmp.path().to_path_buf());

        let r1 = sample_record("t", "alice", "bob", "to-bob");
        store.record(&r1).unwrap();
        let r2 = sample_record("t", "carol", "alice", "to-alice");
        store.record(&r2).unwrap();
        let mut r3 = sample_record("t", "dave", "*", "broadcast");
        r3.content_preview = "broadcast".into();
        store.record(&r3).unwrap();

        let alice_msgs = store.list_by_agent("t", "alice", 10).unwrap();
        // alice sent 1, received 1, and got the broadcast → 3
        assert_eq!(alice_msgs.len(), 3);

        let bob_msgs = store.list_by_agent("t", "bob", 10).unwrap();
        // bob received from alice + broadcast → 2
        assert_eq!(bob_msgs.len(), 2);
    }

    #[test]
    fn test_list_by_time_range() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MessageHistoryStore::with_base(tmp.path().to_path_buf());

        let now = Utc::now();
        let mut older = sample_record("t", "a", "b", "past");
        older.timestamp = now - chrono::Duration::days(2);
        store.record(&older).unwrap();

        let mut fresh = sample_record("t", "a", "b", "now");
        fresh.timestamp = now;
        store.record(&fresh).unwrap();

        let start = now - chrono::Duration::days(1);
        let end = now + chrono::Duration::seconds(2);
        let list = store.list_by_time_range("t", start, end).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].content_preview, "now");
    }

    #[test]
    fn test_prune_old_removes_expired() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MessageHistoryStore::with_base(tmp.path().to_path_buf());

        let mut old = sample_record("t", "a", "b", "old");
        old.timestamp = Utc::now() - chrono::Duration::days(60);
        store.record(&old).unwrap();

        let fresh = sample_record("t", "a", "b", "fresh");
        store.record(&fresh).unwrap();

        let pruned = store.prune_old().unwrap();
        assert!(pruned >= 1);
        assert!(store.find_by_id(&old.message_id).unwrap().is_none());
        assert!(store.find_by_id(&fresh.message_id).unwrap().is_some());
    }

    #[test]
    fn test_path_layout_team_date() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MessageHistoryStore::with_base(tmp.path().to_path_buf());
        let rec = sample_record("alpha", "a", "b", "x");
        store.record(&rec).unwrap();
        let now = Utc::now();
        let expected = tmp.path().join("alpha").join(format!(
            "{:04}-{:02}-{:02}.jsonl",
            now.year(),
            now.month(),
            now.day()
        ));
        assert!(expected.exists());
    }

    #[test]
    fn test_invalid_team_name_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MessageHistoryStore::with_base(tmp.path().to_path_buf());
        let rec = sample_record("../escape", "a", "b", "x");
        let err = store.record(&rec).unwrap_err();
        assert!(matches!(err, HistoryError::InvalidPath(_)));
    }

    #[test]
    fn test_truncate_preview_short_unchanged() {
        let s = "hello";
        assert_eq!(MessageRecord::truncate_preview(s), "hello");
    }

    #[test]
    fn test_truncate_preview_long_truncates_with_ellipsis() {
        let s: String = "x".repeat(MessageRecord::PREVIEW_MAX + 50);
        let out = MessageRecord::truncate_preview(&s);
        assert!(out.ends_with('…'));
        // chars() count: PREVIEW_MAX (truncated) - 1 + ellipsis = PREVIEW_MAX
        assert_eq!(out.chars().count(), MessageRecord::PREVIEW_MAX);
    }

    #[test]
    fn test_content_kind_serde() {
        let kinds = vec![
            ContentKind::Text,
            ContentKind::Structured,
            ContentKind::Protocol,
        ];
        let json = serde_json::to_string(&kinds).unwrap();
        assert!(json.contains("text"));
        assert!(json.contains("structured"));
        assert!(json.contains("protocol"));
        let de: Vec<ContentKind> = serde_json::from_str(&json).unwrap();
        assert_eq!(de, kinds);
    }

    #[test]
    fn test_record_returns_message_id() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MessageHistoryStore::with_base(tmp.path().to_path_buf());
        let rec = sample_record("t", "a", "b", "x");
        let id = store.record(&rec).unwrap();
        assert_eq!(id, rec.message_id);
    }

    #[test]
    fn test_find_missing_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MessageHistoryStore::with_base(tmp.path().to_path_buf());
        assert!(store.find_by_id("nope").unwrap().is_none());
    }

    #[test]
    fn test_empty_store_operations() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MessageHistoryStore::with_base(tmp.path().to_path_buf());
        assert!(store.list_by_team("any", 10).unwrap().is_empty());
        assert_eq!(store.prune_old().unwrap(), 0);
    }

    #[test]
    fn test_multiple_teams_isolated() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MessageHistoryStore::with_base(tmp.path().to_path_buf());

        let r1 = sample_record("alpha", "a", "b", "in-alpha");
        store.record(&r1).unwrap();
        let r2 = sample_record("beta", "a", "b", "in-beta");
        store.record(&r2).unwrap();

        let alpha = store.list_by_team("alpha", 10).unwrap();
        let beta = store.list_by_team("beta", 10).unwrap();
        assert_eq!(alpha.len(), 1);
        assert_eq!(alpha[0].content_preview, "in-alpha");
        assert_eq!(beta.len(), 1);
        assert_eq!(beta[0].content_preview, "in-beta");
    }

    #[test]
    fn test_priority_recorded() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MessageHistoryStore::with_base(tmp.path().to_path_buf());
        let mut rec = sample_record("t", "a", "b", "urgent");
        rec.priority = "critical".into();
        store.record(&rec).unwrap();
        let found = store.find_by_id(&rec.message_id).unwrap().unwrap();
        assert_eq!(found.priority, "critical");
    }
}
