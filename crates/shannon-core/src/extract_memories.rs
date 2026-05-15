//! # Extract Memories
//!
//! Automatic memory extraction from conversations, modeled after Claude Code's
//! `src/services/extractMemories/extractMemories.ts`.
//!
//! ## Architecture
//!
//! The extractor monitors conversation messages and periodically invokes an
//! LLM-based extraction pipeline that identifies noteworthy information
//! (preferences, conventions, decisions, debugging insights) and persists it
//! as structured memory files.
//!
//! - [`MemoryExtractor`]: Top-level orchestrator for extraction lifecycle
//! - [`ExtractionConfig`]: Configuration for extraction behavior
//! - [`ExtractionResult`]: Outcome of an extraction pass
//! - [`ExtractionCategory`]: Typed memory categories for the extraction prompt

use crate::memory::{MemoryCategory, MemoryEntry, MemoryError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use shannon_types::recover_lock;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during memory extraction.
#[derive(Error, Debug)]
pub enum ExtractionError {
    #[error("I/O error during extraction: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error during extraction: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Extraction already in progress")]
    AlreadyInProgress,

    #[error("Memory write detected since last extraction cursor")]
    MemoryWriteDetected,

    #[error("Extraction disabled")]
    Disabled,

    #[error("Not enough messages for extraction (have {have}, need {need})")]
    InsufficientMessages { have: usize, need: usize },

    #[error("Failed to parse extraction output: {0}")]
    ParseError(String),

    #[error("Memory store error: {0}")]
    MemoryStore(#[from] MemoryError),
}

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the memory extraction pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionConfig {
    /// Whether automatic extraction is enabled.
    pub enabled: bool,
    /// Directory where extracted memory files are stored.
    pub memory_dir: PathBuf,
    /// Minimum number of new messages between extraction runs.
    pub min_messages_between_extractions: usize,
    /// Maximum number of conversation turns to include in the extraction prompt.
    pub max_turns: usize,
    /// When `true`, only extract user-level memories (no team/shared memory).
    pub auto_only: bool,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            memory_dir: dirs::home_dir()
                .unwrap_or_default()
                .join(".shannon")
                .join("memories"),
            min_messages_between_extractions: 10,
            max_turns: 50,
            auto_only: true,
        }
    }
}

// ============================================================================
// Result Types
// ============================================================================

/// Outcome of a memory extraction pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    /// Paths to memory files that were saved.
    pub memories_saved: Vec<PathBuf>,
    /// Number of messages processed during extraction.
    pub messages_processed: usize,
    /// Wall-clock duration of the extraction in milliseconds.
    pub duration_ms: u64,
    /// If extraction was skipped, the reason why.
    pub skipped_reason: Option<String>,
}

impl ExtractionResult {
    /// Create a result indicating extraction was skipped.
    pub fn skipped(reason: &str) -> Self {
        Self {
            memories_saved: Vec::new(),
            messages_processed: 0,
            duration_ms: 0,
            skipped_reason: Some(reason.to_string()),
        }
    }

    /// Create a result indicating successful extraction.
    pub fn success(memories_saved: Vec<PathBuf>, messages_processed: usize, duration_ms: u64) -> Self {
        Self {
            memories_saved,
            messages_processed,
            duration_ms,
            skipped_reason: None,
        }
    }

    /// Whether the extraction actually ran (was not skipped).
    pub fn was_extracted(&self) -> bool {
        self.skipped_reason.is_none()
    }
}

// ============================================================================
// Extraction Category
// ============================================================================

/// A category of memory that the extraction prompt asks the LLM to identify.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ExtractionCategory {
    /// Machine-readable name.
    pub name: String,
    /// Human-readable description included in the prompt.
    pub description: String,
    /// Example phrases that signal this category.
    pub examples: Vec<String>,
}

impl ExtractionCategory {
    /// Create a new extraction category.
    pub fn new(name: &str, description: &str, examples: Vec<&str>) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            examples: examples.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// Returns the default set of extraction categories.
pub fn default_categories() -> Vec<ExtractionCategory> {
    vec![
        ExtractionCategory::new(
            "UserPreference",
            "User preferences, workflow choices, tool/editor preferences, personal conventions",
            vec![
                "I always use tabs not spaces",
                "I prefer vim over emacs",
                "My preferred testing framework is pytest",
            ],
        ),
        ExtractionCategory::new(
            "ProjectConvention",
            "Project-specific patterns, coding standards, architectural conventions",
            vec![
                "In this project we follow the repository pattern",
                "Our convention is to prefix test files with test_",
                "This project uses semantic versioning",
            ],
        ),
        ExtractionCategory::new(
            "TechnicalDecision",
            "Architectural decisions and their rationale",
            vec![
                "We decided to use PostgreSQL for the main database",
                "Going with event-driven architecture because of scalability needs",
                "The decision to use Rust was driven by performance requirements",
            ],
        ),
        ExtractionCategory::new(
            "DebuggingInsight",
            "Solutions to problems encountered, debugging knowledge, error patterns",
            vec![
                "The issue was caused by a race condition in the auth module",
                "The fix was to add a mutex around the shared state",
                "This error happens when the database connection pool is exhausted",
            ],
        ),
    ]
}

// ============================================================================
// Message Summary
// ============================================================================

/// A lightweight summary of a conversation message used for extraction decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSummary {
    /// Unique message identifier.
    pub id: String,
    /// The role of the sender ("user", "assistant", "system").
    pub role: String,
    /// Text content of the message.
    pub content: String,
    /// Timestamp when the message was created.
    pub timestamp: DateTime<Utc>,
    /// Optional cursor/offset for tracking extraction progress.
    pub cursor: Option<String>,
}

impl MessageSummary {
    /// Create a new message summary.
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: role.to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            cursor: None,
        }
    }

    /// Create a message summary with an explicit cursor value.
    pub fn with_cursor(role: &str, content: &str, cursor: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: role.to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            cursor: Some(cursor.to_string()),
        }
    }
}

// ============================================================================
// Extracted Memory (intermediate format from LLM output)
// ============================================================================

/// A memory as extracted from the LLM's structured output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedMemory {
    /// The category of this memory.
    pub category: String,
    /// The content/information to remember.
    pub content: String,
    /// Optional tags for retrieval.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Confidence score 0.0-1.0.
    pub confidence: f64,
}

// ============================================================================
// Memory Extractor
// ============================================================================

/// Top-level orchestrator for automatic memory extraction from conversations.
///
/// Monitors the message stream and periodically triggers extraction when
/// enough new messages have accumulated and no extraction is currently running.
pub struct MemoryExtractor {
    /// Directory where memory files are persisted.
    memory_dir: PathBuf,
    /// Whether extraction is enabled.
    enabled: bool,
    /// Minimum number of new messages between extraction runs.
    min_messages_between_extractions: usize,
    /// Maximum number of conversation turns to include in the extraction prompt.
    max_turns: usize,
    /// Cursor of the last successful extraction (message index / ID).
    last_extraction_cursor: Option<String>,
    /// Guard to prevent concurrent extraction.
    in_progress: Arc<Mutex<bool>>,
}

impl MemoryExtractor {
    /// Create a new `MemoryExtractor` from the given configuration.
    pub fn new(config: ExtractionConfig) -> Self {
        let enabled = config.enabled;
        let memory_dir = config.memory_dir.clone();
        let min_messages = config.min_messages_between_extractions;
        let max_turns = config.max_turns;

        // Ensure the memory directory exists.
        if let Err(e) = fs::create_dir_all(&memory_dir) {
            tracing::warn!("Failed to create memory directory {:?}: {e}", memory_dir);
        }

        Self {
            memory_dir,
            enabled,
            min_messages_between_extractions: min_messages,
            max_turns,
            last_extraction_cursor: None,
            in_progress: Arc::new(Mutex::new(false)),
        }
    }

    /// Check whether extraction should be triggered for the given messages.
    ///
    /// Returns `true` when all of the following are met:
    /// 1. Extraction is enabled.
    /// 2. No extraction is currently in progress.
    /// 3. Enough new messages have accumulated since the last extraction.
    /// 4. No memory-write tool use has been detected since the last cursor.
    pub fn should_extract(&self, messages: &[MessageSummary]) -> bool {
        // 1. Enabled check
        if !self.enabled {
            return false;
        }

        // 2. In-progress guard
        {
            let guard = recover_lock(self.in_progress.lock());
            if *guard {
                return false;
            }
        }

        // 3. Sufficient new messages
        let visible = self.count_visible_messages(messages, self.last_extraction_cursor.as_deref());
        if visible < self.min_messages_between_extractions {
            return false;
        }

        // 4. Memory-write detection -- if the main agent wrote memories
        //    since the last extraction, skip to avoid double-extracting.
        if self.has_memory_writes_since(messages, self.last_extraction_cursor.as_deref()) {
            return false;
        }

        true
    }

    /// Run the extraction pipeline on the given messages.
    ///
    /// This method:
    /// 1. Acquires the in-progress lock.
    /// 2. Builds an extraction prompt from recent messages.
    /// 3. Parses the LLM response into structured memories.
    /// 4. Persists each memory as a file and adds it to the store.
    /// 5. Updates the extraction cursor.
    ///
    /// If extraction should not run (per [`should_extract`]), returns a
    /// `skipped` result.
    pub fn extract(
        &self,
        messages: &[MessageSummary],
        existing_memories: &[MemoryEntry],
    ) -> Result<ExtractionResult, ExtractionError> {
        // Check preconditions
        if !self.enabled {
            return Ok(ExtractionResult::skipped("extraction is disabled"));
        }

        // Acquire in-progress guard
        {
            let mut guard = self
                .in_progress
                .lock()
                .map_err(|_| ExtractionError::AlreadyInProgress)?;
            if *guard {
                return Ok(ExtractionResult::skipped("extraction already in progress"));
            }
            *guard = true;
        }

        let start = std::time::Instant::now();

        // Count visible messages
        let visible_count =
            self.count_visible_messages(messages, self.last_extraction_cursor.as_deref());

        let result = if visible_count < self.min_messages_between_extractions {
            Ok(ExtractionResult::skipped(&format!(
                "not enough messages (have {}, need {})",
                visible_count, self.min_messages_between_extractions
            )))
        } else {
            self.do_extract(messages, existing_memories, visible_count, &start)
        };

        // Release the in-progress guard
        {
            let mut guard = self
                .in_progress
                .lock()
                .map_err(|_| ExtractionError::AlreadyInProgress)?;
            *guard = false;
        }

        result
    }

    /// Internal extraction logic. Called by [`extract`] after the in-progress
    /// guard has been acquired.
    fn do_extract(
        &self,
        messages: &[MessageSummary],
        existing_memories: &[MemoryEntry],
        visible_count: usize,
        start: &std::time::Instant,
    ) -> Result<ExtractionResult, ExtractionError> {
        // Build prompt (for documentation / future LLM integration)
        let _prompt =
            self.build_extraction_prompt(visible_count, existing_memories);

        // Get recent messages for context
        let recent_messages = self.get_recent_messages(messages, self.max_turns);

        // Pattern-based extraction from message content
        let extracted = self.pattern_extract_from_messages(&recent_messages);

        if extracted.is_empty() {
            // Update cursor even when nothing extracted
            if let Some(last) = messages.last() {
                self.set_cursor(&last.id);
            }
            return Ok(ExtractionResult::skipped("no memories extracted from recent messages"));
        }

        // Convert extracted memories to MemoryEntry and persist
        let mut saved_paths = Vec::new();
        for mem in &extracted {
            let category = self.resolve_category(&mem.category);
            let entry = MemoryEntry::with_confidence(
                "extracted",
                category,
                &mem.content,
                mem.confidence.clamp(0.0, 1.0),
                mem.tags.clone(),
            )?;

            let path = self.save_memory_entry(&entry)?;
            saved_paths.push(path);
        }

        // Update cursor to the last processed message
        if let Some(last) = messages.last() {
            self.set_cursor(&last.id);
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ExtractionResult::success(
            saved_paths,
            visible_count,
            duration_ms,
        ))
    }

    /// Build the extraction prompt that would be sent to the LLM.
    ///
    /// Includes category definitions, existing memories (for deduplication),
    /// and instructions for structured output.
    pub fn build_extraction_prompt(
        &self,
        message_count: usize,
        existing_memories: &[MemoryEntry],
    ) -> String {
        let categories = default_categories();

        let mut prompt = String::new();

        prompt.push_str("You are a memory extraction assistant. Analyze the recent conversation ");
        prompt.push_str("and extract important information that should be remembered for future sessions.\n\n");

        prompt.push_str(&format!("## Messages to analyze: {message_count}\n\n"));

        prompt.push_str("## Memory Categories\n\n");
        for cat in &categories {
            prompt.push_str(&format!("### {}\n{}\nExamples:\n", cat.name, cat.description));
            for ex in &cat.examples {
                prompt.push_str(&format!("- \"{ex}\"\n"));
            }
            prompt.push('\n');
        }

        prompt.push_str("## Existing Memories (for deduplication)\n\n");
        if existing_memories.is_empty() {
            prompt.push_str("(No existing memories)\n\n");
        } else {
            for mem in existing_memories {
                prompt.push_str(&format!("- [{}] {}\n", mem.category, mem.content));
            }
            prompt.push('\n');
        }

        prompt.push_str("## Instructions\n\n");
        prompt.push_str("1. Extract ONLY information that is likely to be useful in future conversations.\n");
        prompt.push_str("2. Do NOT extract information that is already in the existing memories list.\n");
        prompt.push_str("3. Be concise -- each memory should be a single sentence or short paragraph.\n");
        prompt.push_str("4. Assign a confidence score between 0.5 and 1.0.\n");
        prompt.push_str("5. Output as a JSON array of objects with fields: category, content, tags, confidence.\n");

        prompt
    }

    /// Detect whether the main agent has written memories since the given cursor.
    ///
    /// This prevents double-extraction when the agent has already explicitly
    /// saved memories via a memory-write tool.
    pub fn has_memory_writes_since(
        &self,
        messages: &[MessageSummary],
        cursor: Option<&str>,
    ) -> bool {
        let start_idx = if let Some(c) = cursor {
            messages
                .iter()
                .position(|m| m.cursor.as_deref() == Some(c))
                .map(|i| i + 1)
                .unwrap_or(0)
        } else {
            0
        };

        for msg in &messages[start_idx..] {
            let lower = msg.content.to_lowercase();
            // Detect tool calls that write memories
            if lower.contains("write_memory")
                || lower.contains("save_memory")
                || lower.contains("memory written")
                || lower.contains("stored memory")
            {
                return true;
            }
        }
        false
    }

    /// Count the number of visible messages since the given cursor.
    ///
    /// A "visible" message is one that is from the user or assistant (not system).
    pub fn count_visible_messages(&self, messages: &[MessageSummary], since: Option<&str>) -> usize {
        let start_idx = if let Some(cursor) = since {
            messages
                .iter()
                .position(|m| m.id == cursor || m.cursor.as_deref() == Some(cursor))
                .map(|i| i + 1)
                .unwrap_or(0)
        } else {
            0
        };

        messages[start_idx..]
            .iter()
            .filter(|m| m.role == "user" || m.role == "assistant")
            .count()
    }

    /// Get the N most recent messages from the list.
    fn get_recent_messages(&self, messages: &[MessageSummary], n: usize) -> Vec<MessageSummary> {
        let start = if messages.len() > n {
            messages.len() - n
        } else {
            0
        };
        messages[start..].to_vec()
    }

    /// Resolve a category string to a [`MemoryCategory`].
    fn resolve_category(&self, name: &str) -> MemoryCategory {
        match name {
            "UserPreference" => MemoryCategory::Preference,
            "ProjectConvention" => MemoryCategory::Pattern,
            "TechnicalDecision" => MemoryCategory::Decision,
            "DebuggingInsight" => MemoryCategory::Error,
            _ => MemoryCategory::Context,
        }
    }

    /// Save a memory entry to a file in the memory directory.
    fn save_memory_entry(&self, entry: &MemoryEntry) -> Result<PathBuf, ExtractionError> {
        fs::create_dir_all(&self.memory_dir)?;
        let filename = format!("{}_{}.json", entry.category, entry.id);
        let path = self.memory_dir.join(&filename);
        let json = serde_json::to_string_pretty(entry)?;
        fs::write(&path, json)?;
        Ok(path)
    }

    /// Set the extraction cursor to the given value.
    fn set_cursor(&self, cursor: &str) {
        // We use a simple approach: store in a well-known file.
        let cursor_path = self.memory_dir.join(".last_extraction_cursor");
        if let Err(e) = fs::write(&cursor_path, cursor) {
            tracing::warn!("Failed to save memory extraction cursor: {e}");
        }
    }

    /// Pattern-based extraction from messages (used instead of LLM for testing
    /// and when no API key is available).
    fn pattern_extract_from_messages(&self, messages: &[MessageSummary]) -> Vec<ExtractedMemory> {
        let mut results = Vec::new();

        for msg in messages {
            let lower = msg.content.to_lowercase();

            // Check UserPreference patterns
            for kw in &[
                "i always",
                "i never",
                "i prefer",
                "please always",
                "please never",
                "don't use",
                "do not use",
                "my preference",
            ] {
                if lower.contains(kw) {
                    results.push(ExtractedMemory {
                        category: "UserPreference".to_string(),
                        content: msg.content.clone(),
                        tags: vec!["preference".to_string()],
                        confidence: 0.7,
                    });
                    break;
                }
            }

            // Check ProjectConvention patterns
            for kw in &[
                "in this project we",
                "our convention",
                "our pattern",
                "the standard approach",
                "naming convention",
            ] {
                if lower.contains(kw) {
                    results.push(ExtractedMemory {
                        category: "ProjectConvention".to_string(),
                        content: msg.content.clone(),
                        tags: vec!["convention".to_string()],
                        confidence: 0.7,
                    });
                    break;
                }
            }

            // Check TechnicalDecision patterns
            for kw in &[
                "we decided",
                "let's use",
                "going with",
                "the decision",
                "decided to",
                "we chose",
            ] {
                if lower.contains(kw) {
                    results.push(ExtractedMemory {
                        category: "TechnicalDecision".to_string(),
                        content: msg.content.clone(),
                        tags: vec!["decision".to_string()],
                        confidence: 0.7,
                    });
                    break;
                }
            }

            // Check DebuggingInsight patterns
            for kw in &[
                "the issue was",
                "the error was",
                "the bug was",
                "the fix was",
                "the problem was",
                "root cause",
                "the workaround",
                "to fix this",
                "solution was",
            ] {
                if lower.contains(kw) {
                    results.push(ExtractedMemory {
                        category: "DebuggingInsight".to_string(),
                        content: msg.content.clone(),
                        tags: vec!["debugging".to_string()],
                        confidence: 0.7,
                    });
                    break;
                }
            }
        }

        // Deduplicate by content similarity
        deduplicate_extracted(results)
    }

    /// Get the current extraction cursor.
    pub fn last_cursor(&self) -> Option<String> {
        let cursor_path = self.memory_dir.join(".last_extraction_cursor");
        fs::read_to_string(&cursor_path).ok()
    }

    /// Check whether an extraction is currently running.
    pub fn is_in_progress(&self) -> bool {
        self.in_progress
            .lock()
            .map(|g| *g)
            .unwrap_or(false)
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Deduplicate extracted memories by content similarity (Jaccard > 0.8).
fn deduplicate_extracted(memories: Vec<ExtractedMemory>) -> Vec<ExtractedMemory> {
    let mut unique: Vec<ExtractedMemory> = Vec::new();

    for mem in memories {
        let is_dup = unique.iter().any(|existing| {
            jaccard_similarity(&existing.content, &mem.content) > 0.8
        });
        if !is_dup {
            unique.push(mem);
        }
    }

    unique
}

/// Simple word-level Jaccard similarity.
fn jaccard_similarity(a: &str, b: &str) -> f64 {
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

// ============================================================================
// LLM-Based Memory Extractor
// ============================================================================

/// AI-powered memory extractor that uses the configured LLM to extract
/// structured memories from conversations. Falls back to pattern-based
/// extraction on any API error.
///
/// Uses a dedicated tokio runtime to bridge sync calling code with the
/// async `LlmClient`, avoiding nested-runtime panics.
pub struct LlmMemoryExtractor {
    client: crate::api::LlmClient,
    inner: MemoryExtractor,
}

impl LlmMemoryExtractor {
    /// Create a new LLM-backed extractor wrapping the given client and config.
    pub fn new(client: crate::api::LlmClient, config: ExtractionConfig) -> Self {
        let inner = MemoryExtractor::new(config);
        Self { client, inner }
    }

    /// Check whether extraction should be triggered (delegates to inner).
    pub fn should_extract(&self, messages: &[MessageSummary]) -> bool {
        self.inner.should_extract(messages)
    }

    /// Run LLM-powered extraction, falling back to pattern-based on error.
    ///
    /// First sends the conversation to the LLM with structured extraction
    /// instructions. If the LLM call fails or returns unparseable output,
    /// falls back to keyword pattern matching.
    pub fn extract(
        &self,
        messages: &[MessageSummary],
        existing_memories: &[MemoryEntry],
    ) -> Result<ExtractionResult, ExtractionError> {
        if !self.inner.enabled {
            return Ok(ExtractionResult::skipped("extraction is disabled"));
        }

        // Check in-progress guard
        {
            let guard = recover_lock(self.inner.in_progress.lock());
            if *guard {
                return Ok(ExtractionResult::skipped("extraction already in progress"));
            }
        }

        let visible_count = self.inner.count_visible_messages(
            messages,
            self.inner.last_extraction_cursor.as_deref(),
        );

        if visible_count < self.inner.min_messages_between_extractions {
            return Ok(ExtractionResult::skipped(&format!(
                "not enough messages (have {}, need {})",
                visible_count, self.inner.min_messages_between_extractions
            )));
        }

        // Set in-progress
        {
            let mut guard = self.inner.in_progress.lock().map_err(|_| ExtractionError::AlreadyInProgress)?;
            *guard = true;
        }

        let start = std::time::Instant::now();
        let result = self.do_llm_extract(messages, existing_memories, visible_count, &start);

        // Release in-progress
        {
            let mut guard = self.inner.in_progress.lock().map_err(|_| ExtractionError::AlreadyInProgress)?;
            *guard = false;
        }

        result
    }

    fn do_llm_extract(
        &self,
        messages: &[MessageSummary],
        existing_memories: &[MemoryEntry],
        visible_count: usize,
        start: &std::time::Instant,
    ) -> Result<ExtractionResult, ExtractionError> {
        // Try LLM extraction first
        let extracted = match self.call_llm_extract(messages, existing_memories) {
            Ok(memories) => memories,
            Err(_) => {
                // Fallback to pattern-based extraction
                let recent = self.inner.get_recent_messages(messages, self.inner.max_turns);
                self.inner.pattern_extract_from_messages(&recent)
            }
        };

        if extracted.is_empty() {
            if let Some(last) = messages.last() {
                self.inner.set_cursor(&last.id);
            }
            return Ok(ExtractionResult::skipped("no memories extracted from recent messages"));
        }

        let mut saved_paths = Vec::new();
        for mem in &extracted {
            let category = self.inner.resolve_category(&mem.category);
            let entry = MemoryEntry::with_confidence(
                "extracted",
                category,
                &mem.content,
                mem.confidence.clamp(0.0, 1.0),
                mem.tags.clone(),
            )?;

            let path = self.inner.save_memory_entry(&entry)?;
            saved_paths.push(path);
        }

        if let Some(last) = messages.last() {
            self.inner.set_cursor(&last.id);
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(ExtractionResult::success(saved_paths, visible_count, duration_ms))
    }

    /// Call the LLM to extract memories from recent messages.
    fn call_llm_extract(
        &self,
        messages: &[MessageSummary],
        existing_memories: &[MemoryEntry],
    ) -> Result<Vec<ExtractedMemory>, ExtractionError> {
        let visible_count = self.inner.count_visible_messages(
            messages,
            self.inner.last_extraction_cursor.as_deref(),
        );

        let prompt = self.inner.build_extraction_prompt(visible_count, existing_memories);

        // Build the conversation messages for the LLM
        let recent = self.inner.get_recent_messages(messages, self.inner.max_turns);
        let mut conversation_text = String::new();
        for msg in &recent {
            conversation_text.push_str(&format!("[{}] {}\n", msg.role, msg.content));
        }

        let system_msg = crate::api::Message {
            role: "system".to_string(),
            content: crate::api::MessageContent::Text(prompt),
        };
        let user_msg = crate::api::Message {
            role: "user".to_string(),
            content: crate::api::MessageContent::Text(format!(
                "Extract memories from this conversation:\n\n{conversation_text}"
            )),
        };

        // Use a dedicated runtime to avoid nested runtime panics
        let client = self.client.clone();
        let rt = tokio::runtime::Runtime::new().map_err(|e| {
            ExtractionError::ParseError(format!("failed to create tokio runtime: {e}"))
        })?;

        let blocks = rt.block_on(async {
            client
                .send_message(vec![system_msg, user_msg], None, None)
                .await
                .map_err(|e| ExtractionError::ParseError(format!("LLM call failed: {e}")))
        })?;

        // Extract text from response blocks
        let response_text: String = blocks
            .iter()
            .filter_map(|b| match b {
                crate::api::ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<&str>>()
            .join("");

        parse_llm_extraction_output(&response_text)
    }

    /// Access the inner pattern-based extractor.
    pub fn inner(&self) -> &MemoryExtractor {
        &self.inner
    }
}

/// Parse the LLM's JSON array output into extracted memories.
///
/// Tries to find a JSON array in the response text (may be wrapped in
/// markdown code blocks). Falls back to empty vec on parse failure.
fn parse_llm_extraction_output(text: &str) -> Result<Vec<ExtractedMemory>, ExtractionError> {
    // Strip markdown code fences if present
    let cleaned = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    // Try to find a JSON array in the text
    let json_start = cleaned.find('[');
    let json_end = cleaned.rfind(']');

    let json_str = match (json_start, json_end) {
        (Some(s), Some(e)) if e > s => &cleaned[s..=e],
        _ => return Err(ExtractionError::ParseError("no JSON array found in LLM output".into())),
    };

    let parsed: Vec<ExtractedMemory> = serde_json::from_str(json_str).map_err(|e| {
        ExtractionError::ParseError(format!("failed to parse LLM extraction output: {e}"))
    })?;

    Ok(parsed)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ---------------------------------------------------------------------------
    // ExtractionConfig tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_extraction_config_default() {
        let config = ExtractionConfig::default();
        assert!(config.enabled);
        assert_eq!(config.min_messages_between_extractions, 10);
        assert_eq!(config.max_turns, 50);
        assert!(config.auto_only);
    }

    #[test]
    fn test_extraction_config_serialization() {
        let config = ExtractionConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ExtractionConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.min_messages_between_extractions, config.min_messages_between_extractions);
        assert_eq!(deserialized.max_turns, config.max_turns);
    }

    // ---------------------------------------------------------------------------
    // ExtractionResult tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_extraction_result_skipped() {
        let result = ExtractionResult::skipped("not enough messages");
        assert!(!result.was_extracted());
        assert_eq!(result.skipped_reason, Some("not enough messages".to_string()));
        assert!(result.memories_saved.is_empty());
    }

    #[test]
    fn test_extraction_result_success() {
        let result = ExtractionResult::success(
            vec![PathBuf::from("/tmp/mem.json")],
            15,
            42,
        );
        assert!(result.was_extracted());
        assert!(result.skipped_reason.is_none());
        assert_eq!(result.memories_saved.len(), 1);
        assert_eq!(result.messages_processed, 15);
        assert_eq!(result.duration_ms, 42);
    }

    #[test]
    fn test_extraction_result_serialization() {
        let result = ExtractionResult::skipped("disabled");
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: ExtractionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.skipped_reason, Some("disabled".to_string()));
    }

    // ---------------------------------------------------------------------------
    // ExtractionCategory tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_default_categories() {
        let cats = default_categories();
        assert_eq!(cats.len(), 4);
        let names: Vec<&str> = cats.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"UserPreference"));
        assert!(names.contains(&"ProjectConvention"));
        assert!(names.contains(&"TechnicalDecision"));
        assert!(names.contains(&"DebuggingInsight"));
    }

    #[test]
    fn test_extraction_category_examples() {
        let cat = ExtractionCategory::new(
            "Test",
            "A test category",
            vec!["example 1", "example 2"],
        );
        assert_eq!(cat.examples.len(), 2);
        assert_eq!(cat.examples[0], "example 1");
    }

    // ---------------------------------------------------------------------------
    // MessageSummary tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_message_summary_new() {
        let msg = MessageSummary::new("user", "Hello world");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello world");
        assert!(msg.cursor.is_none());
        assert!(!msg.id.is_empty());
    }

    #[test]
    fn test_message_summary_with_cursor() {
        let msg = MessageSummary::with_cursor("user", "Hello", "cursor-123");
        assert_eq!(msg.cursor, Some("cursor-123".to_string()));
    }

    // ---------------------------------------------------------------------------
    // MemoryExtractor tests
    // ---------------------------------------------------------------------------

    fn make_extractor(temp_dir: &TempDir, min_messages: usize) -> MemoryExtractor {
        let config = ExtractionConfig {
            enabled: true,
            memory_dir: temp_dir.path().to_path_buf(),
            min_messages_between_extractions: min_messages,
            max_turns: 50,
            auto_only: true,
        };
        MemoryExtractor::new(config)
    }

    fn make_extractor_disabled(temp_dir: &TempDir) -> MemoryExtractor {
        let config = ExtractionConfig {
            enabled: false,
            memory_dir: temp_dir.path().to_path_buf(),
            min_messages_between_extractions: 5,
            max_turns: 50,
            auto_only: true,
        };
        MemoryExtractor::new(config)
    }

    #[test]
    fn test_should_extract_disabled() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor_disabled(&temp_dir);
        let messages = vec![MessageSummary::new("user", "I always use tabs")];
        assert!(!extractor.should_extract(&messages));
    }

    #[test]
    fn test_should_extract_insufficient_messages() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 10);
        let messages = (0..5)
            .map(|_| MessageSummary::new("user", "hello"))
            .collect::<Vec<_>>();
        assert!(!extractor.should_extract(&messages));
    }

    #[test]
    fn test_should_extract_sufficient_messages() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 3);
        let messages = (0..5)
            .map(|_| MessageSummary::new("user", "hello"))
            .collect::<Vec<_>>();
        assert!(extractor.should_extract(&messages));
    }

    #[test]
    fn test_should_extract_memory_write_detected() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 3);

        let mut messages: Vec<MessageSummary> = (0..5)
            .map(|_| MessageSummary::new("user", "hello"))
            .collect();

        // Add a message that looks like a memory write
        messages.push(MessageSummary::new("assistant", "Memory written successfully."));

        // should_extract should return false because memory write detected
        assert!(!extractor.should_extract(&messages));
    }

    #[test]
    fn test_should_extract_system_messages_excluded() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 3);

        // System messages should not count toward the threshold
        let messages = vec![
            MessageSummary::new("system", "You are helpful."),
            MessageSummary::new("system", "Be concise."),
            MessageSummary::new("user", "Hello"),
        ];

        // Only 1 visible message (user), need 3
        assert!(!extractor.should_extract(&messages));
    }

    #[test]
    fn test_count_visible_messages_no_cursor() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 3);

        let messages = vec![
            MessageSummary::new("user", "a"),
            MessageSummary::new("assistant", "b"),
            MessageSummary::new("system", "c"), // should be excluded
        ];

        assert_eq!(extractor.count_visible_messages(&messages, None), 2);
    }

    #[test]
    fn test_count_visible_messages_with_cursor() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 3);

        let msg0 = MessageSummary::new("user", "a");
        let msg1 = MessageSummary::new("user", "b");
        let msg2 = MessageSummary::new("user", "c");
        let msg3 = MessageSummary::new("user", "d");

        let cursor_id = msg1.id.clone();
        let messages = vec![msg0, msg1, msg2, msg3];

        // Messages after cursor (index 1) = msg2, msg3 = 2
        assert_eq!(
            extractor.count_visible_messages(&messages, Some(&cursor_id)),
            2
        );
    }

    #[test]
    fn test_has_memory_writes_since() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 3);

        let msg0 = MessageSummary::new("user", "hello");
        let msg1 = MessageSummary::new("assistant", "write_memory called");
        let msg2 = MessageSummary::new("user", "world");

        let cursor = msg0.id.clone();
        let messages = vec![msg0, msg1, msg2];

        assert!(extractor.has_memory_writes_since(&messages, Some(&cursor)));
    }

    #[test]
    fn test_has_memory_writes_since_no_writes() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 3);

        let msg0 = MessageSummary::new("user", "hello");
        let msg1 = MessageSummary::new("assistant", "regular response");
        let msg2 = MessageSummary::new("user", "world");

        let cursor = msg0.id.clone();
        let messages = vec![msg0, msg1, msg2];

        assert!(!extractor.has_memory_writes_since(&messages, Some(&cursor)));
    }

    #[test]
    fn test_build_extraction_prompt() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 3);

        let prompt = extractor.build_extraction_prompt(20, &[]);

        assert!(prompt.contains("20"));
        assert!(prompt.contains("UserPreference"));
        assert!(prompt.contains("ProjectConvention"));
        assert!(prompt.contains("TechnicalDecision"));
        assert!(prompt.contains("DebuggingInsight"));
        assert!(prompt.contains("JSON array"));
    }

    #[test]
    fn test_build_extraction_prompt_with_existing_memories() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 3);

        let existing = vec![
            MemoryEntry::new("proj", MemoryCategory::Preference, "Use tabs"),
        ];

        let prompt = extractor.build_extraction_prompt(10, &existing);
        assert!(prompt.contains("Use tabs"));
        assert!(prompt.contains("deduplication"));
    }

    #[test]
    fn test_extract_disabled() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor_disabled(&temp_dir);

        let messages = vec![MessageSummary::new("user", "I always use tabs")];
        let result = extractor.extract(&messages, &[]).unwrap();

        assert!(!result.was_extracted());
        assert!(result.skipped_reason.unwrap().contains("disabled"));
    }

    #[test]
    fn test_extract_insufficient_messages() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 10);

        let messages = (0..3)
            .map(|_| MessageSummary::new("user", "hello"))
            .collect::<Vec<_>>();

        let result = extractor.extract(&messages, &[]).unwrap();
        assert!(!result.was_extracted());
    }

    #[test]
    fn test_extract_success() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 3);

        let messages = vec![
            MessageSummary::new("user", "I always use tabs for indentation."),
            MessageSummary::new("assistant", "Noted."),
            MessageSummary::new("user", "We decided to use Rust."),
            MessageSummary::new("assistant", "Good choice."),
            MessageSummary::new("user", "The issue was a race condition."),
        ];

        let result = extractor.extract(&messages, &[]).unwrap();
        assert!(result.was_extracted());
        assert!(result.messages_processed > 0);
        assert!(!result.memories_saved.is_empty());
    }

    #[test]
    fn test_extract_persists_files() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 3);

        let messages = vec![
            MessageSummary::new("user", "I always use tabs for indentation."),
            MessageSummary::new("assistant", "Noted."),
            MessageSummary::new("user", "We decided to use Rust."),
            MessageSummary::new("assistant", "Good choice."),
            MessageSummary::new("user", "The issue was a race condition."),
        ];

        let result = extractor.extract(&messages, &[]).unwrap();
        assert!(result.was_extracted());

        // Verify files exist on disk
        for path in &result.memories_saved {
            assert!(path.exists(), "memory file should exist: {path:?}");
        }
    }

    #[test]
    fn test_extract_updates_cursor() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 3);

        let messages = vec![
            MessageSummary::new("user", "I always use tabs for indentation."),
            MessageSummary::new("assistant", "Noted."),
            MessageSummary::new("user", "We decided to use Rust."),
            MessageSummary::new("assistant", "Good choice."),
            MessageSummary::new("user", "The issue was a race condition."),
        ];

        extractor.extract(&messages, &[]).unwrap();
        let cursor = extractor.last_cursor();
        assert!(cursor.is_some());
    }

    #[test]
    fn test_resolve_category() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 3);

        assert_eq!(
            extractor.resolve_category("UserPreference"),
            MemoryCategory::Preference
        );
        assert_eq!(
            extractor.resolve_category("ProjectConvention"),
            MemoryCategory::Pattern
        );
        assert_eq!(
            extractor.resolve_category("TechnicalDecision"),
            MemoryCategory::Decision
        );
        assert_eq!(
            extractor.resolve_category("DebuggingInsight"),
            MemoryCategory::Error
        );
        assert_eq!(
            extractor.resolve_category("Unknown"),
            MemoryCategory::Context
        );
    }

    #[test]
    fn test_is_in_progress_false_initially() {
        let temp_dir = TempDir::new().unwrap();
        let extractor = make_extractor(&temp_dir, 3);
        assert!(!extractor.is_in_progress());
    }

    // ---------------------------------------------------------------------------
    // Jaccard similarity tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_jaccard_identical() {
        assert!((jaccard_similarity("hello world", "hello world") - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_jaccard_different() {
        assert!(jaccard_similarity("cats and dogs", "planes and trains") < 0.5);
    }

    #[test]
    fn test_jaccard_empty() {
        assert!((jaccard_similarity("", "") - 1.0).abs() < 0.001);
    }

    // ---------------------------------------------------------------------------
    // Deduplication tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_deduplicate_extracted_identical() {
        let memories = vec![
            ExtractedMemory {
                category: "UserPreference".to_string(),
                content: "Always use tabs for indentation".to_string(),
                tags: vec!["preference".to_string()],
                confidence: 0.7,
            },
            ExtractedMemory {
                category: "UserPreference".to_string(),
                content: "Always use tabs for indentation".to_string(),
                tags: vec!["preference".to_string()],
                confidence: 0.8,
            },
        ];
        let deduped = deduplicate_extracted(memories);
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn test_deduplicate_extracted_different() {
        let memories = vec![
            ExtractedMemory {
                category: "UserPreference".to_string(),
                content: "Always use tabs".to_string(),
                tags: vec![],
                confidence: 0.7,
            },
            ExtractedMemory {
                category: "TechnicalDecision".to_string(),
                content: "Use PostgreSQL for database".to_string(),
                tags: vec![],
                confidence: 0.8,
            },
        ];
        let deduped = deduplicate_extracted(memories);
        assert_eq!(deduped.len(), 2);
    }
}
