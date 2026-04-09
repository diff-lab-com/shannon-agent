use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::MemoryError;

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

/// A more granular memory type for session memory management.
///
/// This extends [`MemoryCategory`] with scope information (user vs project vs
/// shared) and is used by the session memory consolidation system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MemoryType {
    /// User preferences, workflow choices.
    UserPreference,
    /// Project-specific patterns, coding standards.
    ProjectConvention,
    /// Architectural decisions and rationale.
    TechnicalDecision,
    /// Solutions to problems encountered.
    DebuggingInsight,
}

impl MemoryType {
    /// Return all memory type variants.
    pub fn all() -> Vec<MemoryType> {
        vec![
            MemoryType::UserPreference,
            MemoryType::ProjectConvention,
            MemoryType::TechnicalDecision,
            MemoryType::DebuggingInsight,
        ]
    }

    /// Parse a string into a `MemoryType`.
    pub fn from_string(s: &str) -> Option<MemoryType> {
        match s {
            "UserPreference" => Some(MemoryType::UserPreference),
            "ProjectConvention" => Some(MemoryType::ProjectConvention),
            "TechnicalDecision" => Some(MemoryType::TechnicalDecision),
            "DebuggingInsight" => Some(MemoryType::DebuggingInsight),
            _ => None,
        }
    }

    /// String representation of this memory type.
    pub fn as_str(&self) -> &str {
        match self {
            MemoryType::UserPreference => "UserPreference",
            MemoryType::ProjectConvention => "ProjectConvention",
            MemoryType::TechnicalDecision => "TechnicalDecision",
            MemoryType::DebuggingInsight => "DebuggingInsight",
        }
    }

    /// Human-readable description of this memory type.
    pub fn description(&self) -> &str {
        match self {
            MemoryType::UserPreference => "User preferences and workflow choices",
            MemoryType::ProjectConvention => "Project-specific patterns and coding standards",
            MemoryType::TechnicalDecision => "Architectural decisions and rationale",
            MemoryType::DebuggingInsight => "Solutions to problems encountered during debugging",
        }
    }

    /// The scope directory name for storing this type of memory.
    ///
    /// - `"user"` -- stored per-user, shared across projects
    /// - `"project"` -- stored per-project
    /// - `"shared"` -- stored per-project but intended for team sharing
    pub fn scope_directory(&self) -> &str {
        match self {
            MemoryType::UserPreference => "user",
            MemoryType::ProjectConvention => "project",
            MemoryType::TechnicalDecision => "project",
            MemoryType::DebuggingInsight => "shared",
        }
    }
}

impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<MemoryType> for MemoryCategory {
    fn from(mt: MemoryType) -> Self {
        match mt {
            MemoryType::UserPreference => MemoryCategory::Preference,
            MemoryType::ProjectConvention => MemoryCategory::Pattern,
            MemoryType::TechnicalDecision => MemoryCategory::Decision,
            MemoryType::DebuggingInsight => MemoryCategory::Error,
        }
    }
}

// ============================================================================
// Session Memory Configuration
// ============================================================================

/// Configuration for session memory management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMemoryConfig {
    /// Whether automatic extraction from conversations is enabled.
    pub auto_extract_enabled: bool,
    /// Maximum number of memories to retain per category.
    pub max_memories_per_category: usize,
    /// Time-to-live for auto-extracted memories. Memories older than this
    /// are eligible for cleanup.
    pub memory_ttl: Duration,
    /// Whether memory consolidation (dedup + merge) is enabled.
    pub consolidation_enabled: bool,
}

impl Default for SessionMemoryConfig {
    fn default() -> Self {
        Self {
            auto_extract_enabled: true,
            max_memories_per_category: 100,
            memory_ttl: Duration::days(90),
            consolidation_enabled: true,
        }
    }
}

// ============================================================================
// Memory Entry
// ============================================================================

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
