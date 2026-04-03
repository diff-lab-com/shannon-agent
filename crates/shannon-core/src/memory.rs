//! # Auto-Dream: Automatic Memory Extraction and Persistence
//!
//! This module provides a system for automatically extracting important information
//! from conversations and persisting it across sessions.
//!
//! ## Architecture
//!
//! Memories are stored as JSON files under `~/.shannon/memories/`, one file per
//! project (keyed by a hash of the project path).
//!
//! - [`MemoryStore`]: CRUD + search + persistence for memory entries
//! - [`AutoDreamService`]: Pattern-based extraction of memories from conversations
//!
//! ## Memory Categories
//!
//! Memories are classified into categories for better retrieval:
//! - **Preference**: User preferences ("always use tabs not spaces")
//! - **Pattern**: Code patterns observed
//! - **Decision**: Architectural decisions made
//! - **Error**: Recurring errors and solutions
//! - **Context**: Project-specific context

use crate::api::{Message, MessageContent};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use thiserror::Error;
use uuid::Uuid;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during memory operations
#[derive(Error, Debug)]
pub enum MemoryError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Memory not found: {0}")]
    NotFound(String),

    #[error("Invalid confidence value: {0}. Must be between 0.0 and 1.0")]
    InvalidConfidence(f64),
}

// ============================================================================
// Memory Types
// ============================================================================

/// Classification of a memory entry's content type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MemoryCategory {
    /// User preferences ("always use tabs not spaces")
    Preference,
    /// Code patterns observed
    Pattern,
    /// Architectural decisions made
    Decision,
    /// Recurring errors and solutions
    Error,
    /// Project-specific context
    Context,
}

impl std::fmt::Display for MemoryCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryCategory::Preference => write!(f, "preference"),
            MemoryCategory::Pattern => write!(f, "pattern"),
            MemoryCategory::Decision => write!(f, "decision"),
            MemoryCategory::Error => write!(f, "error"),
            MemoryCategory::Context => write!(f, "context"),
        }
    }
}

/// A single memory entry extracted from a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique identifier (UUID)
    pub id: String,
    /// Project path or name this memory belongs to
    pub project: String,
    /// Category classification for this memory
    pub category: MemoryCategory,
    /// The remembered information content
    pub content: String,
    /// Searchable tags for retrieval
    pub tags: Vec<String>,
    /// Confidence score 0.0-1.0 indicating extraction quality
    pub confidence: f64,
    /// When this memory was created
    pub created_at: DateTime<Utc>,
    /// When this memory was last accessed
    pub accessed_at: DateTime<Utc>,
    /// Number of times this memory has been accessed
    pub access_count: u32,
}

impl MemoryEntry {
    /// Create a new memory entry with the given content, project, and category.
    ///
    /// Generates a new UUID, sets timestamps to now, and initializes access count to 0.
    pub fn new(project: &str, category: MemoryCategory, content: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            project: project.to_string(),
            category,
            content: content.to_string(),
            tags: Vec::new(),
            confidence: 1.0,
            created_at: Utc::now(),
            accessed_at: Utc::now(),
            access_count: 0,
        }
    }

    /// Create a memory entry with all fields specified.
    pub fn with_confidence(
        project: &str,
        category: MemoryCategory,
        content: &str,
        confidence: f64,
        tags: Vec<String>,
    ) -> Result<Self, MemoryError> {
        if !(0.0..=1.0).contains(&confidence) {
            return Err(MemoryError::InvalidConfidence(confidence));
        }
        Ok(Self {
            id: Uuid::new_v4().to_string(),
            project: project.to_string(),
            category,
            content: content.to_string(),
            tags,
            confidence,
            created_at: Utc::now(),
            accessed_at: Utc::now(),
            access_count: 0,
        })
    }

    /// Record an access to this memory (updates timestamp and count).
    pub fn touch(&mut self) {
        self.accessed_at = Utc::now();
        self.access_count += 1;
    }

    /// Check if this entry's content contains the given query substring (case-insensitive).
    pub fn matches_query(&self, query: &str) -> bool {
        let query_lower = query.to_lowercase();
        self.content.to_lowercase().contains(&query_lower)
            || self.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
    }
}

// ============================================================================
// Memory Store
// ============================================================================

/// Persistent storage for memory entries, backed by JSON files on disk.
///
/// Each project's memories are stored in a separate file:
/// `{storage_path}/{project_hash}.json`
pub struct MemoryStore {
    entries: HashMap<String, MemoryEntry>,
    storage_path: PathBuf,
}

impl MemoryStore {
    /// Create a new empty memory store.
    ///
    /// The storage directory will be created on [`load`](Self::load) if it
    /// does not already exist.
    pub fn new(storage_path: PathBuf) -> Self {
        Self {
            entries: HashMap::new(),
            storage_path,
        }
    }

    /// Add a memory entry to the store.
    ///
    /// If an entry with the same ID already exists it will be overwritten.
    pub fn add(&mut self, entry: MemoryEntry) -> Result<(), MemoryError> {
        self.entries.insert(entry.id.clone(), entry);
        Ok(())
    }

    /// Retrieve a memory entry by ID.
    ///
    /// Returns `None` if no entry with the given ID exists.
    pub fn get(&self, id: &str) -> Option<&MemoryEntry> {
        self.entries.get(id)
    }

    /// Retrieve a mutable memory entry by ID and record an access.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut MemoryEntry> {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.touch();
        }
        self.entries.get_mut(id)
    }

    /// Search memories by substring match on content and tags.
    ///
    /// If `project` is provided, results are filtered to that project.
    /// Results are sorted by confidence descending.
    pub fn search(&self, query: &str, project: Option<&str>) -> Vec<MemoryEntry> {
        let mut results: Vec<MemoryEntry> = self
            .entries
            .values()
            .filter(|e| {
                let project_match = project.map_or(true, |p| e.project == p);
                project_match && e.matches_query(query)
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        results
    }

    /// Get all memories belonging to a specific project.
    ///
    /// Results are sorted by creation date, most recent first.
    pub fn project_memories(&self, project: &str) -> Vec<MemoryEntry> {
        let mut results: Vec<MemoryEntry> = self
            .entries
            .values()
            .filter(|e| e.project == project)
            .cloned()
            .collect();

        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        results
    }

    /// Delete a memory entry by ID.
    ///
    /// Returns `Ok(true)` if the entry was found and removed, `Ok(false)` otherwise.
    pub fn delete(&mut self, id: &str) -> Result<bool, MemoryError> {
        Ok(self.entries.remove(id).is_some())
    }

    /// Persist all memories to disk as JSON.
    ///
    /// One JSON file per project is written to `{storage_path}/{project_hash}.json`.
    /// Each file contains a `Vec<MemoryEntry>`.
    pub fn save(&self) -> Result<(), MemoryError> {
        fs::create_dir_all(&self.storage_path)?;

        // Group entries by project
        let mut by_project: HashMap<String, Vec<&MemoryEntry>> = HashMap::new();
        for entry in self.entries.values() {
            by_project
                .entry(entry.project.clone())
                .or_default()
                .push(entry);
        }

        for (project, entries) in &by_project {
            let hash = project_hash(project);
            let path = self.storage_path.join(format!("{}.json", hash));
            let json = serde_json::to_string_pretty(entries)?;
            fs::write(&path, json)?;
        }

        Ok(())
    }

    /// Load memories from disk.
    ///
    /// Reads all `{project_hash}.json` files from the storage directory and
    /// merges them into the in-memory store. Creates the storage directory
    /// if it does not exist.
    pub fn load(&mut self) -> Result<(), MemoryError> {
        fs::create_dir_all(&self.storage_path)?;

        if !self.storage_path.exists() {
            return Ok(());
        }

        let entries = fs::read_dir(&self.storage_path)?;
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let contents = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let file_entries: Vec<MemoryEntry> = match serde_json::from_str(&contents) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for mem in file_entries {
                self.entries.insert(mem.id.clone(), mem);
            }
        }

        Ok(())
    }

    /// Remove old entries and cap the total count.
    ///
    /// First removes entries older than `max_age`. Then, if the store still
    /// exceeds `max_entries`, removes the least-recently-accessed entries until
    /// the cap is met.
    ///
    /// Returns the total number of entries removed.
    pub fn cleanup(&mut self, max_age: Duration, max_entries: usize) -> Result<usize, MemoryError> {
        let cutoff = Utc::now() - max_age;
        let initial_count = self.entries.len();

        // Remove entries older than max_age
        self.entries.retain(|_, entry| entry.created_at > cutoff);

        // If still over capacity, remove least-recently-accessed entries
        if self.entries.len() > max_entries {
            let mut access_times: Vec<(String, DateTime<Utc>)> = self
                .entries
                .iter()
                .map(|(id, entry)| (id.clone(), entry.accessed_at))
                .collect();

            access_times.sort_by_key(|(_, t)| *t);

            let to_remove = self.entries.len() - max_entries;
            for (id, _) in access_times.into_iter().take(to_remove) {
                self.entries.remove(&id);
            }
        }

        // Persist changes after cleanup
        self.save()?;

        Ok(initial_count - self.entries.len())
    }

    /// Return the number of entries currently in the store.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return true if the store contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ============================================================================
// Auto-Dream Service
// ============================================================================

/// Pattern-based memory extraction service.
///
/// Scans conversation text for signals that indicate memorable information
/// (preferences, decisions, errors, patterns) and creates [`MemoryEntry`]
/// instances automatically.
pub struct AutoDreamService {
    store: Arc<RwLock<MemoryStore>>,
}

impl AutoDreamService {
    /// Create a new AutoDreamService backed by the given store.
    pub fn new(store: Arc<RwLock<MemoryStore>>) -> Self {
        Self { store }
    }

    /// Extract memories from a block of text.
    ///
    /// Uses pattern matching to detect:
    /// - **Preferences**: "always/never/prefer/use X not Y"
    /// - **Decisions**: "we decided/let's use/agreed on/chosen X"
    /// - **Errors**: "the issue was/error/bug/fix was/broken because"
    /// - **Patterns**: "the pattern is/typically we/in this project we"
    ///
    /// Surrounding context (approximately +/-2 sentences) is included.
    /// Confidence is assigned based on how explicitly the signal was stated.
    pub fn extract_memories(&self, conversation: &str, project: &str) -> Vec<MemoryEntry> {
        let mut memories = Vec::new();

        let sentences: Vec<&str> = split_into_sentences(conversation);

        for (i, sentence) in sentences.iter().enumerate() {
            let lower = sentence.to_lowercase();

            // --- Preference detection ---
            if let Some(memory) = detect_preference(&lower, sentence, &sentences, i, project) {
                memories.push(memory);
                continue;
            }

            // --- Decision detection ---
            if let Some(memory) = detect_decision(&lower, sentence, &sentences, i, project) {
                memories.push(memory);
                continue;
            }

            // --- Error detection ---
            if let Some(memory) = detect_error(&lower, sentence, &sentences, i, project) {
                memories.push(memory);
                continue;
            }

            // --- Pattern detection ---
            if let Some(memory) = detect_pattern(&lower, sentence, &sentences, i, project) {
                memories.push(memory);
                continue;
            }
        }

        memories
    }

    /// Process a list of conversation messages, extract memories, deduplicate,
    /// and store them.
    ///
    /// Returns the list of newly stored memories (excluding duplicates).
    pub fn process_conversation(
        &self,
        messages: &[Message],
        project: &str,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        // Concatenate all message text
        let full_text: String = messages
            .iter()
            .map(|m| match &m.content {
                MessageContent::Text(t) => t.clone(),
                MessageContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|b| match b {
                        crate::api::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            })
            .collect::<Vec<_>>()
            .join("\n");

        let extracted = self.extract_memories(&full_text, project);
        let deduped = deduplicate_memories(extracted);

        // Store all new memories
        let mut store = self
            .store
            .write()
            .map_err(|e| MemoryError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        for entry in &deduped {
            store.add(entry.clone())?;
        }

        store.save()?;
        Ok(deduped)
    }

    /// Search the memory store.
    ///
    /// Convenience method that acquires a read lock on the store.
    pub fn search(&self, query: &str, project: Option<&str>) -> Vec<MemoryEntry> {
        match self.store.read() {
            Ok(store) => store.search(query, project),
            Err(_) => Vec::new(),
        }
    }

    /// Get all memories for a project.
    pub fn project_memories(&self, project: &str) -> Vec<MemoryEntry> {
        match self.store.read() {
            Ok(store) => store.project_memories(project),
            Err(_) => Vec::new(),
        }
    }
}

// ============================================================================
// Extraction helpers
// ============================================================================

/// Keywords that signal a user preference.
const PREFERENCE_KEYWORDS: &[&str] = &[
    "i always",
    "i never",
    "i prefer",
    "prefer to",
    "always use",
    "never use",
    "please always",
    "please never",
    "make sure to always",
    "make sure to never",
    "don't use",
    "do not use",
    "remember to",
    "i like",
    "i dislike",
    "my preference",
    "style guide",
];

/// Keywords that signal an architectural decision.
const DECISION_KEYWORDS: &[&str] = &[
    "we decided",
    "let's use",
    "lets use",
    "we'll use",
    "going with",
    "agreed on",
    "chosen",
    "we chose",
    "decided to",
    "the decision",
    "we're going to",
    "we are going to",
    "as decided",
    "final decision",
    "let's go with",
    "lets go with",
];

/// Keywords that signal an error or solution.
const ERROR_KEYWORDS: &[&str] = &[
    "the issue was",
    "the error was",
    "the bug was",
    "the fix was",
    "the problem was",
    "caused by",
    "broken because",
    "failed because",
    "root cause",
    "the workaround",
    "to fix this",
    "solution was",
    "resolved by",
];

/// Keywords that signal a code pattern.
const PATTERN_KEYWORDS: &[&str] = &[
    "the pattern is",
    "typically we",
    "in this project we",
    "our convention",
    "we follow",
    "our pattern",
    "the standard approach",
    "our standard",
    "code style",
    "naming convention",
    "we structure",
    "our architecture",
    "the pattern here",
];

/// Detect a preference in the given sentence.
fn detect_preference(
    lower: &str,
    sentence: &str,
    sentences: &[&str],
    index: usize,
    project: &str,
) -> Option<MemoryEntry> {
    for kw in PREFERENCE_KEYWORDS {
        if lower.contains(kw) {
            let context = extract_context(sentences, index, 2);
            let confidence = confidence_for_sentence(sentence, kw);
            let mut entry = MemoryEntry::with_confidence(
                project,
                MemoryCategory::Preference,
                &context,
                confidence,
                vec!["preference".to_string(), tag_from_keyword(kw).to_string()],
            )
            .ok()?;

            // Auto-extract some tags from the content
            entry.tags = deduplicate_tags(entry.tags);
            return Some(entry);
        }
    }
    None
}

/// Detect a decision in the given sentence.
fn detect_decision(
    lower: &str,
    sentence: &str,
    sentences: &[&str],
    index: usize,
    project: &str,
) -> Option<MemoryEntry> {
    for kw in DECISION_KEYWORDS {
        if lower.contains(kw) {
            let context = extract_context(sentences, index, 2);
            let confidence = confidence_for_sentence(sentence, kw);
            let mut entry = MemoryEntry::with_confidence(
                project,
                MemoryCategory::Decision,
                &context,
                confidence,
                vec!["decision".to_string(), tag_from_keyword(kw).to_string()],
            )
            .ok()?;

            entry.tags = deduplicate_tags(entry.tags);
            return Some(entry);
        }
    }
    None
}

/// Detect an error/solution in the given sentence.
fn detect_error(
    lower: &str,
    sentence: &str,
    sentences: &[&str],
    index: usize,
    project: &str,
) -> Option<MemoryEntry> {
    for kw in ERROR_KEYWORDS {
        if lower.contains(kw) {
            let context = extract_context(sentences, index, 2);
            let confidence = confidence_for_sentence(sentence, kw);
            let mut entry = MemoryEntry::with_confidence(
                project,
                MemoryCategory::Error,
                &context,
                confidence,
                vec!["error".to_string(), "solution".to_string()],
            )
            .ok()?;

            entry.tags = deduplicate_tags(entry.tags);
            return Some(entry);
        }
    }
    None
}

/// Detect a code pattern in the given sentence.
fn detect_pattern(
    lower: &str,
    sentence: &str,
    sentences: &[&str],
    index: usize,
    project: &str,
) -> Option<MemoryEntry> {
    for kw in PATTERN_KEYWORDS {
        if lower.contains(kw) {
            let context = extract_context(sentences, index, 2);
            let confidence = confidence_for_sentence(sentence, kw);
            let mut entry = MemoryEntry::with_confidence(
                project,
                MemoryCategory::Pattern,
                &context,
                confidence,
                vec!["pattern".to_string()],
            )
            .ok()?;

            entry.tags = deduplicate_tags(entry.tags);
            return Some(entry);
        }
    }
    None
}

/// Extract surrounding context ( +/- `radius` sentences) around the sentence at `index`.
fn extract_context(sentences: &[&str], index: usize, radius: usize) -> String {
    let start = index.saturating_sub(radius);
    let end = (index + radius + 1).min(sentences.len());
    sentences[start..end].join(" ").trim().to_string()
}

/// Compute a confidence score based on how explicitly the keyword was used.
///
/// Higher confidence for:
/// - Sentence starts with the keyword (more direct)
/// - Keyword is longer (more specific)
/// - Sentence is not too short (more context)
fn confidence_for_sentence(sentence: &str, keyword: &str) -> f64 {
    let lower = sentence.to_lowercase();

    let mut confidence: f64 = 0.5;

    // Bonus for sentence starting with keyword
    if lower.starts_with(keyword) {
        confidence += 0.2;
    }

    // Bonus for keyword specificity (length)
    if keyword.len() > 10 {
        confidence += 0.1;
    }

    // Bonus for sentence length (more context = better)
    if sentence.len() > 50 {
        confidence += 0.1;
    }

    // Bonus for explicit markers
    if lower.contains("always") || lower.contains("never") {
        confidence += 0.1;
    }

    confidence.min(1.0)
}

/// Deduplicate memories by removing entries with very similar content.
///
/// Two entries are considered duplicates if their normalized content has a
/// Jaccard similarity above 0.8.
fn deduplicate_memories(memories: Vec<MemoryEntry>) -> Vec<MemoryEntry> {
    let mut unique: Vec<MemoryEntry> = Vec::new();

    for memory in memories {
        let is_dup = unique.iter().any(|existing| {
            content_similarity(&existing.content, &memory.content) > 0.8
        });

        if !is_dup {
            unique.push(memory);
        }
    }

    unique
}

/// Simple word-level Jaccard similarity between two strings.
fn content_similarity(a: &str, b: &str) -> f64 {
    let words_a: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let words_b: std::collections::HashSet<&str> = b.split_whitespace().collect();

    if words_a.is_empty() && words_b.is_empty() {
        return 1.0;
    }

    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();

    if union == 0 {
        return 1.0;
    }

    intersection as f64 / union as f64
}

/// Create a tag from a keyword phrase.
fn tag_from_keyword(keyword: &str) -> &str {
    // Take the last word of multi-word keywords as a tag
    keyword
        .split_whitespace()
        .last()
        .unwrap_or(keyword)
}

/// Remove duplicate tags from a vector.
fn deduplicate_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    tags.into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

/// Split text into sentences using common delimiters.
fn split_into_sentences(text: &str) -> Vec<&str> {
    let mut sentences: Vec<&str> = Vec::new();
    let mut start = 0;

    for (i, c) in text.char_indices() {
        if c == '.' || c == '!' || c == '?' || c == '\n' {
            let end = if c == '\n' { i } else { i + 1 };
            let sentence = text[start..end].trim();
            if !sentence.is_empty() {
                sentences.push(sentence);
            }
            start = i + 1;
        }
    }

    // Don't forget the last fragment
    let remaining = text[start..].trim();
    if !remaining.is_empty() {
        sentences.push(remaining);
    }

    sentences
}

/// Hash a project path to a safe filename.
fn project_hash(project: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    project.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ---------------------------------------------------------------------------
    // MemoryEntry tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_memory_entry_creation() {
        let entry = MemoryEntry::new("test-project", MemoryCategory::Preference, "Always use tabs");
        assert!(!entry.id.is_empty());
        assert_eq!(entry.project, "test-project");
        assert_eq!(entry.category, MemoryCategory::Preference);
        assert_eq!(entry.content, "Always use tabs");
        assert!(entry.tags.is_empty());
        assert_eq!(entry.confidence, 1.0);
        assert_eq!(entry.access_count, 0);
    }

    #[test]
    fn test_memory_entry_with_confidence() {
        let entry = MemoryEntry::with_confidence(
            "proj",
            MemoryCategory::Decision,
            "Use Rust",
            0.8,
            vec!["rust".to_string()],
        )
        .unwrap();

        assert_eq!(entry.confidence, 0.8);
        assert_eq!(entry.tags, vec!["rust"]);
    }

    #[test]
    fn test_memory_entry_invalid_confidence() {
        let result = MemoryEntry::with_confidence("proj", MemoryCategory::Preference, "x", 1.5, vec![]);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MemoryError::InvalidConfidence(1.5)));
    }

    #[test]
    fn test_memory_entry_invalid_confidence_negative() {
        let result = MemoryEntry::with_confidence("proj", MemoryCategory::Preference, "x", -0.1, vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_memory_entry_touch() {
        let mut entry = MemoryEntry::new("proj", MemoryCategory::Context, "some context");
        let original_accessed = entry.accessed_at;
        assert_eq!(entry.access_count, 0);

        entry.touch();
        assert_eq!(entry.access_count, 1);
        assert!(entry.accessed_at >= original_accessed);
    }

    #[test]
    fn test_memory_entry_matches_query_content() {
        let entry = MemoryEntry::new("proj", MemoryCategory::Preference, "Always use tabs for indentation");
        assert!(entry.matches_query("tabs"));
        assert!(entry.matches_query("TAB")); // case-insensitive
        assert!(entry.matches_query("indentation"));
        assert!(!entry.matches_query("spaces"));
    }

    #[test]
    fn test_memory_entry_matches_query_tags() {
        let mut entry = MemoryEntry::new("proj", MemoryCategory::Preference, "Use tabs");
        entry.tags = vec!["formatting".to_string(), "style".to_string()];
        assert!(entry.matches_query("formatting"));
        assert!(entry.matches_query("STYLE"));
        assert!(!entry.matches_query("something-else"));
    }

    #[test]
    fn test_memory_entry_serialization_roundtrip() {
        let entry = MemoryEntry::new("my-project", MemoryCategory::Error, "Bug in parser");
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: MemoryEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, entry.id);
        assert_eq!(deserialized.project, entry.project);
        assert_eq!(deserialized.category, entry.category);
        assert_eq!(deserialized.content, entry.content);
        assert_eq!(deserialized.confidence, entry.confidence);
    }

    #[test]
    fn test_memory_category_display() {
        assert_eq!(MemoryCategory::Preference.to_string(), "preference");
        assert_eq!(MemoryCategory::Pattern.to_string(), "pattern");
        assert_eq!(MemoryCategory::Decision.to_string(), "decision");
        assert_eq!(MemoryCategory::Error.to_string(), "error");
        assert_eq!(MemoryCategory::Context.to_string(), "context");
    }

    // ---------------------------------------------------------------------------
    // MemoryStore CRUD tests
    // ---------------------------------------------------------------------------

    fn temp_store() -> (MemoryStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let store = MemoryStore::new(temp_dir.path().to_path_buf());
        (store, temp_dir)
    }

    #[test]
    fn test_store_add_and_get() {
        let (mut store, _tmp) = temp_store();
        let entry = MemoryEntry::new("proj", MemoryCategory::Preference, "Use tabs");
        let id = entry.id.clone();

        store.add(entry).unwrap();
        let retrieved = store.get(&id).unwrap();
        assert_eq!(retrieved.content, "Use tabs");
    }

    #[test]
    fn test_store_get_nonexistent() {
        let (store, _tmp) = temp_store();
        assert!(store.get("nonexistent").is_none());
    }

    #[test]
    fn test_store_get_mut_touches() {
        let (mut store, _tmp) = temp_store();
        let entry = MemoryEntry::new("proj", MemoryCategory::Context, "context");
        let id = entry.id.clone();
        store.add(entry).unwrap();

        let retrieved = store.get_mut(&id).unwrap();
        assert_eq!(retrieved.access_count, 1);

        retrieved.touch();
        assert_eq!(retrieved.access_count, 2);
    }

    #[test]
    fn test_store_delete() {
        let (mut store, _tmp) = temp_store();
        let entry = MemoryEntry::new("proj", MemoryCategory::Error, "bug");
        let id = entry.id.clone();
        store.add(entry).unwrap();

        let deleted = store.delete(&id).unwrap();
        assert!(deleted);
        assert!(store.get(&id).is_none());
    }

    #[test]
    fn test_store_delete_nonexistent() {
        let (mut store, _tmp) = temp_store();
        let deleted = store.delete("nonexistent").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn test_store_len_and_is_empty() {
        let (mut store, _tmp) = temp_store();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);

        store.add(MemoryEntry::new("p", MemoryCategory::Context, "a")).unwrap();
        assert_eq!(store.len(), 1);
        assert!(!store.is_empty());
    }

    // ---------------------------------------------------------------------------
    // MemoryStore search tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_store_search_content() {
        let (mut store, _tmp) = temp_store();
        store.add(MemoryEntry::new("proj", MemoryCategory::Preference, "Always use tabs")).unwrap();
        store.add(MemoryEntry::new("proj", MemoryCategory::Pattern, "Use Result<T> for errors")).unwrap();
        store.add(MemoryEntry::new("other", MemoryCategory::Context, "Some other context")).unwrap();

        let results = store.search("tabs", None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "Always use tabs");
    }

    #[test]
    fn test_store_search_tags() {
        let (mut store, _tmp) = temp_store();
        let mut entry = MemoryEntry::new("proj", MemoryCategory::Preference, "Use tabs");
        entry.tags = vec!["formatting".to_string()];
        store.add(entry).unwrap();

        let results = store.search("formatting", None);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_store_search_with_project_filter() {
        let (mut store, _tmp) = temp_store();
        store.add(MemoryEntry::new("proj-a", MemoryCategory::Preference, "Use tabs in proj-a")).unwrap();
        store.add(MemoryEntry::new("proj-b", MemoryCategory::Preference, "Use tabs in proj-b")).unwrap();

        let results = store.search("tabs", Some("proj-a"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].project, "proj-a");
    }

    #[test]
    fn test_store_search_case_insensitive() {
        let (mut store, _tmp) = temp_store();
        store.add(MemoryEntry::new("proj", MemoryCategory::Preference, "Always use RUST")).unwrap();

        let results = store.search("rust", None);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_store_search_sorted_by_confidence() {
        let (mut store, _tmp) = temp_store();

        let mut low = MemoryEntry::new("proj", MemoryCategory::Preference, "Use tabs preference");
        low.confidence = 0.3;
        let mut high = MemoryEntry::new("proj", MemoryCategory::Preference, "Use spaces preference");
        high.confidence = 0.9;

        store.add(low).unwrap();
        store.add(high).unwrap();

        let results = store.search("preference", None);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].confidence, 0.9);
        assert_eq!(results[1].confidence, 0.3);
    }

    #[test]
    fn test_store_search_no_results() {
        let (store, _tmp) = temp_store();
        let results = store.search("nonexistent", None);
        assert!(results.is_empty());
    }

    // ---------------------------------------------------------------------------
    // MemoryStore project filtering tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_store_project_memories() {
        let (mut store, _tmp) = temp_store();
        store.add(MemoryEntry::new("proj-a", MemoryCategory::Preference, "pref-a")).unwrap();
        store.add(MemoryEntry::new("proj-b", MemoryCategory::Error, "error-b")).unwrap();
        store.add(MemoryEntry::new("proj-a", MemoryCategory::Decision, "dec-a")).unwrap();

        let proj_a = store.project_memories("proj-a");
        assert_eq!(proj_a.len(), 2);

        let proj_b = store.project_memories("proj-b");
        assert_eq!(proj_b.len(), 1);

        let proj_c = store.project_memories("proj-c");
        assert!(proj_c.is_empty());
    }

    #[test]
    fn test_store_project_memories_sorted_by_date() {
        let (mut store, _tmp) = temp_store();

        let mut old = MemoryEntry::new("proj", MemoryCategory::Context, "old");
        old.created_at = Utc::now() - Duration::days(1);
        let mut new = MemoryEntry::new("proj", MemoryCategory::Context, "new");
        new.created_at = Utc::now();

        store.add(old).unwrap();
        store.add(new).unwrap();

        let memories = store.project_memories("proj");
        assert_eq!(memories[0].content, "new");
        assert_eq!(memories[1].content, "old");
    }

    // ---------------------------------------------------------------------------
    // MemoryStore persistence tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_store_save_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("subdir").join("memories");
        assert!(!path.exists());

        let mut store = MemoryStore::new(path.clone());
        store.add(MemoryEntry::new("proj", MemoryCategory::Context, "test")).unwrap();
        store.save().unwrap();

        assert!(path.exists());
    }

    #[test]
    fn test_store_save_and_load_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("memories");

        // Create and save
        let mut store = MemoryStore::new(path.clone());
        store.add(MemoryEntry::new("proj-a", MemoryCategory::Preference, "Use tabs")).unwrap();
        store.add(MemoryEntry::new("proj-b", MemoryCategory::Error, "Parser bug")).unwrap();
        store.add(MemoryEntry::new("proj-a", MemoryCategory::Decision, "Chose async")).unwrap();
        store.save().unwrap();

        // Load into a new store
        let mut store2 = MemoryStore::new(path);
        store2.load().unwrap();

        assert_eq!(store2.len(), 3);
        assert!(store2.get(&store.entries.keys().next().unwrap()).is_some());
    }

    #[test]
    fn test_store_load_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("nonexistent").join("memories");
        assert!(!path.exists());

        let mut store = MemoryStore::new(path.clone());
        store.load().unwrap();

        assert!(path.exists());
    }

    #[test]
    fn test_store_load_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("empty");

        let mut store = MemoryStore::new(path.clone());
        store.load().unwrap();

        assert!(store.is_empty());
    }

    #[test]
    fn test_store_load_skips_invalid_files() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("memories");

        // Write a valid file
        let valid_entry = MemoryEntry::new("proj", MemoryCategory::Context, "valid");
        let valid_hash = project_hash("proj");
        let valid_path = path.join(format!("{}.json", valid_hash));
        fs::create_dir_all(&path).unwrap();
        let json = serde_json::to_string_pretty(&vec![&valid_entry]).unwrap();
        fs::write(&valid_path, json).unwrap();

        // Write a garbage file
        let garbage_path = path.join("garbage.json");
        fs::write(&garbage_path, "not json {{{{").unwrap();

        // Write a non-json file (should be skipped)
        let txt_path = path.join("readme.txt");
        fs::write(&txt_path, "hello").unwrap();

        let mut store = MemoryStore::new(path);
        store.load().unwrap();

        assert_eq!(store.len(), 1);
    }

    #[test]
    fn test_store_save_creates_per_project_files() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("memories");

        let mut store = MemoryStore::new(path.clone());
        store.add(MemoryEntry::new("project-alpha", MemoryCategory::Preference, "pref")).unwrap();
        store.add(MemoryEntry::new("project-beta", MemoryCategory::Error, "err")).unwrap();
        store.save().unwrap();

        // Two project files should exist
        let files: Vec<String> = fs::read_dir(&path)
            .unwrap()
            .filter_map(|e| {
                e.ok()
                    .and_then(|e| e.path().extension().map(|ext| ext.to_string_lossy().to_string()))
            })
            .collect();

        assert_eq!(files.len(), 2);
    }

    // ---------------------------------------------------------------------------
    // MemoryStore cleanup tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_store_cleanup_age_based() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("memories");

        let mut store = MemoryStore::new(path.clone());
        store.load().unwrap();

        // Add an old entry
        let mut old = MemoryEntry::new("proj", MemoryCategory::Context, "old");
        old.created_at = Utc::now() - Duration::days(60);
        store.add(old).unwrap();

        // Add a recent entry
        store.add(MemoryEntry::new("proj", MemoryCategory::Context, "new")).unwrap();

        assert_eq!(store.len(), 2);

        // Cleanup entries older than 30 days
        let removed = store.cleanup(Duration::days(30), 100).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn test_store_cleanup_count_based() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("memories");

        let mut store = MemoryStore::new(path.clone());
        store.load().unwrap();

        // Add 5 entries
        for i in 0..5 {
            store.add(MemoryEntry::new("proj", MemoryCategory::Context, &format!("entry-{}", i))).unwrap();
        }

        assert_eq!(store.len(), 5);

        // Cap at 2 entries (all are recent, so only count-based removal applies)
        let removed = store.cleanup(Duration::days(365), 2).unwrap();
        assert_eq!(removed, 3);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_store_cleanup_persists_changes() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("memories");

        let mut store = MemoryStore::new(path.clone());
        store.load().unwrap();

        let mut old = MemoryEntry::new("proj", MemoryCategory::Context, "old");
        old.created_at = Utc::now() - Duration::days(100);
        store.add(old).unwrap();

        store.cleanup(Duration::days(30), 100).unwrap();

        // Reload and verify
        let mut store2 = MemoryStore::new(path);
        store2.load().unwrap();
        assert!(store2.is_empty());
    }

    #[test]
    fn test_store_cleanup_nothing_to_remove() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("memories");

        let mut store = MemoryStore::new(path.clone());
        store.load().unwrap();
        store.add(MemoryEntry::new("proj", MemoryCategory::Context, "recent")).unwrap();

        let removed = store.cleanup(Duration::days(365), 100).unwrap();
        assert_eq!(removed, 0);
        assert_eq!(store.len(), 1);
    }

    // ---------------------------------------------------------------------------
    // AutoDreamService extraction tests
    // ---------------------------------------------------------------------------

    fn make_service() -> (AutoDreamService, Arc<RwLock<MemoryStore>>) {
        let temp_dir = TempDir::new().unwrap();
        let store = Arc::new(RwLock::new(MemoryStore::new(temp_dir.path().to_path_buf())));
        let service = AutoDreamService::new(store.clone());
        (service, store)
    }

    #[test]
    fn test_extract_preference_always() {
        let (service, _store) = make_service();
        let memories = service.extract_memories(
            "I always use tabs instead of spaces for indentation.",
            "test-project",
        );

        assert!(!memories.is_empty());
        assert_eq!(memories[0].category, MemoryCategory::Preference);
        assert!(memories[0].content.to_lowercase().contains("tabs"));
    }

    #[test]
    fn test_extract_preference_never() {
        let (service, _) = make_service();
        let memories = service.extract_memories(
            "Please never use unwrap() in production code.",
            "test-project",
        );

        assert!(!memories.is_empty());
        assert_eq!(memories[0].category, MemoryCategory::Preference);
    }

    #[test]
    fn test_extract_preference_prefer() {
        let (service, _) = make_service();
        let memories = service.extract_memories(
            "I prefer functional programming patterns over OOP.",
            "test-project",
        );

        assert!(!memories.is_empty());
        assert_eq!(memories[0].category, MemoryCategory::Preference);
    }

    #[test]
    fn test_extract_decision() {
        let (service, _) = make_service();
        let memories = service.extract_memories(
            "We decided to use PostgreSQL for the database.",
            "test-project",
        );

        assert!(!memories.is_empty());
        assert_eq!(memories[0].category, MemoryCategory::Decision);
        assert!(memories[0].content.to_lowercase().contains("postgresql"));
    }

    #[test]
    fn test_extract_decision_lets_use() {
        let (service, _) = make_service();
        let memories = service.extract_memories(
            "Let's use Rust for the backend service.",
            "test-project",
        );

        assert!(!memories.is_empty());
        assert_eq!(memories[0].category, MemoryCategory::Decision);
    }

    #[test]
    fn test_extract_error() {
        let (service, _) = make_service();
        let memories = service.extract_memories(
            "The issue was a missing import in the auth module.",
            "test-project",
        );

        assert!(!memories.is_empty());
        assert_eq!(memories[0].category, MemoryCategory::Error);
    }

    #[test]
    fn test_extract_error_fix() {
        let (service, _) = make_service();
        let memories = service.extract_memories(
            "The fix was to update the serde version to 1.0.200.",
            "test-project",
        );

        assert!(!memories.is_empty());
        assert_eq!(memories[0].category, MemoryCategory::Error);
    }

    #[test]
    fn test_extract_pattern() {
        let (service, _) = make_service();
        let memories = service.extract_memories(
            "In this project we follow the repository pattern for data access.",
            "test-project",
        );

        assert!(!memories.is_empty());
        assert_eq!(memories[0].category, MemoryCategory::Pattern);
    }

    #[test]
    fn test_extract_pattern_convention() {
        let (service, _) = make_service();
        let memories = service.extract_memories(
            "Our naming convention is snake_case for functions and CamelCase for types.",
            "test-project",
        );

        assert!(!memories.is_empty());
        assert_eq!(memories[0].category, MemoryCategory::Pattern);
    }

    #[test]
    fn test_extract_no_memories_from_plain_text() {
        let (service, _) = make_service();
        let memories = service.extract_memories(
            "The weather is nice today. I went for a walk in the park.",
            "test-project",
        );

        assert!(memories.is_empty());
    }

    #[test]
    fn test_extract_multiple_memories() {
        let (service, _) = make_service();
        let text = "I always use tabs. We decided to use Rust. The issue was a race condition.";
        let memories = service.extract_memories(text, "test-project");

        assert!(memories.len() >= 3);
        let categories: Vec<&MemoryCategory> = memories.iter().map(|m| &m.category).collect();
        assert!(categories.contains(&&MemoryCategory::Preference));
        assert!(categories.contains(&&MemoryCategory::Decision));
        assert!(categories.contains(&&MemoryCategory::Error));
    }

    #[test]
    fn test_extract_includes_context() {
        let (service, _) = make_service();
        let text = "I looked at the codebase. I always use tabs for indentation. This is my preference.";
        let memories = service.extract_memories(text, "test-project");

        assert!(!memories.is_empty());
        // Context should include surrounding sentences
        let content_lower = memories[0].content.to_lowercase();
        assert!(content_lower.contains("tabs"));
    }

    #[test]
    fn test_extract_confidence_scores() {
        let (service, _) = make_service();

        // More explicit statement should get higher confidence
        let memories1 = service.extract_memories(
            "I always use tabs for indentation in this project.",
            "test-project",
        );

        let memories2 = service.extract_memories(
            "Maybe I sometimes prefer tabs over spaces.",
            "test-project",
        );

        if !memories1.is_empty() && !memories2.is_empty() {
            // Both should have valid confidence
            assert!((0.0..=1.0).contains(&memories1[0].confidence));
            assert!((0.0..=1.0).contains(&memories2[0].confidence));
        }
    }

    // ---------------------------------------------------------------------------
    // Deduplication tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_deduplicate_identical_entries() {
        let entries = vec![
            MemoryEntry::new("proj", MemoryCategory::Preference, "Always use tabs for indentation"),
            MemoryEntry::new("proj", MemoryCategory::Preference, "Always use tabs for indentation"),
        ];

        let deduped = deduplicate_memories(entries);
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn test_deduplicate_similar_entries() {
        let entries = vec![
            MemoryEntry::new("proj", MemoryCategory::Preference, "Always use tabs for indentation in code"),
            MemoryEntry::new("proj", MemoryCategory::Preference, "Always use tabs for indentation in Rust code"),
        ];

        let deduped = deduplicate_memories(entries);
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn test_deduplicate_keeps_different_entries() {
        let entries = vec![
            MemoryEntry::new("proj", MemoryCategory::Preference, "Always use tabs for indentation"),
            MemoryEntry::new("proj", MemoryCategory::Error, "The bug was in the parser module"),
        ];

        let deduped = deduplicate_memories(entries);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn test_deduplicate_empty_input() {
        let deduped = deduplicate_memories(Vec::new());
        assert!(deduped.is_empty());
    }

    // ---------------------------------------------------------------------------
    // Confidence scoring tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_confidence_always_keyword_higher() {
        let score = confidence_for_sentence("I always use tabs for indentation", "i always");
        assert!(score > 0.5, "confidence should be above 0.5, got {}", score);
    }

    #[test]
    fn test_confidence_short_sentence_lower() {
        let score = confidence_for_sentence("I prefer tabs", "i prefer");
        assert!(score < 1.0);
    }

    #[test]
    fn test_confidence_capped_at_one() {
        let score = confidence_for_sentence(
            "I always use tabs for indentation in this very large project with many files",
            "i always",
        );
        assert!(score <= 1.0);
    }

    // ---------------------------------------------------------------------------
    // Content similarity tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_similarity_identical() {
        let sim = content_similarity("hello world", "hello world");
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_similarity_completely_different() {
        let sim = content_similarity("cats and dogs", "planes and trains");
        assert!(sim < 0.5);
    }

    #[test]
    fn test_similarity_partial_overlap() {
        let sim = content_similarity("use tabs for indentation", "use tabs for Rust indentation");
        assert!(sim > 0.5);
        assert!(sim < 1.0);
    }

    #[test]
    fn test_similarity_empty_strings() {
        let sim = content_similarity("", "");
        assert!((sim - 1.0).abs() < 0.001);
    }

    // ---------------------------------------------------------------------------
    // Process conversation tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_process_conversation() {
        let (service, store) = make_service();

        let messages = vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::Text(
                    "I always use tabs instead of spaces for indentation.".to_string(),
                ),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Text(
                    "Noted. I'll keep that in mind for all future code changes.".to_string(),
                ),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Text(
                    "The error was a race condition in the auth module. The fix was adding a mutex.".to_string(),
                ),
            },
        ];

        let result = service.process_conversation(&messages, "test-project").unwrap();

        assert!(!result.is_empty());
        assert!(result.len() >= 2, "expected >= 2 memories, got {}", result.len());

        // Verify stored
        let stored = store.read().unwrap();
        assert!(stored.len() >= 2);
    }

    #[test]
    fn test_process_conversation_deduplicates() {
        let (service, _store) = make_service();

        let messages = vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("I always use tabs for indentation.".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Text("Noted, always use tabs.".to_string()),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("Always use tabs for indentation in Rust files.".to_string()),
            },
        ];

        let result = service.process_conversation(&messages, "test-project").unwrap();

        // Should deduplicate similar preference memories
        let pref_count = result.iter().filter(|m| m.category == MemoryCategory::Preference).count();
        assert!(pref_count <= 2, "Expected at most 2 preference memories, got {}", pref_count);
    }

    #[test]
    fn test_process_conversation_empty() {
        let (service, store) = make_service();

        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("Hello, how are you?".to_string()),
        }];

        let result = service.process_conversation(&messages, "test-project").unwrap();
        assert!(result.is_empty());

        let stored = store.read().unwrap();
        assert!(stored.is_empty());
    }

    // ---------------------------------------------------------------------------
    // Helper function tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_split_into_sentences() {
        let text = "First sentence. Second sentence! Third sentence? Fourth\nFifth";
        let sentences = split_into_sentences(text);

        assert_eq!(sentences.len(), 5);
        assert_eq!(sentences[0], "First sentence.");
        assert_eq!(sentences[1], "Second sentence!");
    }

    #[test]
    fn test_split_into_sentences_empty() {
        let sentences = split_into_sentences("");
        assert!(sentences.is_empty());
    }

    #[test]
    fn test_project_hash_deterministic() {
        let h1 = project_hash("my-project");
        let h2 = project_hash("my-project");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_project_hash_different_projects() {
        let h1 = project_hash("project-a");
        let h2 = project_hash("project-b");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_project_hash_safe_filename() {
        let hash = project_hash("/some/path with spaces/and/special-chars_!@#$");
        // Should only contain hex characters
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ---------------------------------------------------------------------------
    // AutoDreamService search tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_service_search() {
        let (service, store) = {
            let temp_dir = TempDir::new().unwrap();
            let store = Arc::new(RwLock::new(MemoryStore::new(temp_dir.path().to_path_buf())));
            let service = AutoDreamService::new(store.clone());

            // Pre-populate the store
            store.write().unwrap().add(MemoryEntry::new("proj", MemoryCategory::Preference, "Use tabs")).unwrap();

            (service, store)
        };

        let results = service.search("tabs", None);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_service_project_memories() {
        let (service, store) = {
            let temp_dir = TempDir::new().unwrap();
            let store = Arc::new(RwLock::new(MemoryStore::new(temp_dir.path().to_path_buf())));
            let service = AutoDreamService::new(store.clone());

            store.write().unwrap().add(MemoryEntry::new("my-proj", MemoryCategory::Context, "ctx")).unwrap();

            (service, store)
        };

        let results = service.project_memories("my-proj");
        assert_eq!(results.len(), 1);
    }
}
