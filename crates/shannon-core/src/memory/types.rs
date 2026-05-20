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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_entry_new_basic_fields() {
        let entry = MemoryEntry::new("my-project", MemoryCategory::Preference, "Use tabs not spaces");
        assert!(!entry.id.is_empty(), "id should be a non-empty UUID string");
        assert_eq!(entry.project, "my-project");
        assert_eq!(entry.category, MemoryCategory::Preference);
        assert_eq!(entry.content, "Use tabs not spaces");
        assert!(entry.tags.is_empty());
        assert!((entry.confidence - 1.0).abs() < f64::EPSILON);
        assert_eq!(entry.access_count, 0);
    }

    #[test]
    fn test_memory_entry_new_timestamps_are_recent() {
        let before = Utc::now();
        let entry = MemoryEntry::new("proj", MemoryCategory::Pattern, "content");
        let after = Utc::now();
        assert!(entry.created_at >= before);
        assert!(entry.created_at <= after);
        assert!(entry.accessed_at >= before);
        assert!(entry.accessed_at <= after);
    }

    #[test]
    fn test_with_confidence_valid() {
        let entry = MemoryEntry::with_confidence(
            "proj", MemoryCategory::Decision, "Use PostgreSQL", 0.85,
            vec!["database".to_string()],
        ).unwrap();
        assert!((entry.confidence - 0.85).abs() < f64::EPSILON);
        assert_eq!(entry.tags, vec!["database"]);
        assert_eq!(entry.category, MemoryCategory::Decision);
    }

    #[test]
    fn test_with_confidence_zero() {
        let entry = MemoryEntry::with_confidence("p", MemoryCategory::Error, "msg", 0.0, vec![]);
        assert!(entry.is_ok());
        assert!((entry.unwrap().confidence).abs() < f64::EPSILON);
    }

    #[test]
    fn test_with_confidence_one() {
        let entry = MemoryEntry::with_confidence("p", MemoryCategory::Error, "msg", 1.0, vec![]);
        assert!(entry.is_ok());
        assert!((entry.unwrap().confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_with_confidence_negative_rejected() {
        let result = MemoryEntry::with_confidence("p", MemoryCategory::Preference, "x", -0.1, vec![]);
        assert!(result.is_err());
        match result.unwrap_err() {
            MemoryError::InvalidConfidence(v) => assert!((v - (-0.1)).abs() < f64::EPSILON),
            other => panic!("expected InvalidConfidence, got {:?}", other),
        }
    }

    #[test]
    fn test_with_confidence_above_one_rejected() {
        let result = MemoryEntry::with_confidence("p", MemoryCategory::Preference, "x", 1.5, vec![]);
        assert!(result.is_err());
        match result.unwrap_err() {
            MemoryError::InvalidConfidence(v) => assert!((v - 1.5).abs() < f64::EPSILON),
            other => panic!("expected InvalidConfidence, got {:?}", other),
        }
    }

    #[test]
    fn test_touch_updates_access_count() {
        let mut entry = MemoryEntry::new("p", MemoryCategory::Context, "data");
        assert_eq!(entry.access_count, 0);
        entry.touch();
        assert_eq!(entry.access_count, 1);
        entry.touch();
        assert_eq!(entry.access_count, 2);
    }

    #[test]
    fn test_touch_updates_accessed_at() {
        let mut entry = MemoryEntry::new("p", MemoryCategory::Context, "data");
        let original = entry.accessed_at;
        entry.touch();
        assert!(entry.accessed_at >= original);
    }

    #[test]
    fn test_matches_query_content_substring() {
        let entry = MemoryEntry::new("p", MemoryCategory::Preference, "Always use tabs for indentation");
        assert!(entry.matches_query("tabs"));
        assert!(entry.matches_query("TABS"));
        assert!(entry.matches_query("always use"));
        assert!(!entry.matches_query("spaces"));
    }

    #[test]
    fn test_matches_query_tag_match() {
        let entry = MemoryEntry::with_confidence(
            "p", MemoryCategory::Decision, "Use PostgreSQL", 0.9,
            vec!["database".to_string(), "sql".to_string()],
        ).unwrap();
        assert!(entry.matches_query("database"));
        assert!(entry.matches_query("SQL"));
        assert!(!entry.matches_query("mongodb"));
    }

    #[test]
    fn test_memory_type_all_returns_all_variants() {
        let all = MemoryType::all();
        assert_eq!(all.len(), 4);
        assert!(all.contains(&MemoryType::UserPreference));
        assert!(all.contains(&MemoryType::ProjectConvention));
        assert!(all.contains(&MemoryType::TechnicalDecision));
        assert!(all.contains(&MemoryType::DebuggingInsight));
    }

    #[test]
    fn test_memory_type_from_string_roundtrip() {
        for mt in MemoryType::all() {
            assert_eq!(MemoryType::from_string(mt.as_str()), Some(mt.clone()));
        }
    }

    #[test]
    fn test_memory_type_from_string_unknown() {
        assert_eq!(MemoryType::from_string("NonExistent"), None);
    }

    #[test]
    fn test_memory_type_display() {
        assert_eq!(format!("{}", MemoryType::UserPreference), "UserPreference");
        assert_eq!(format!("{}", MemoryType::DebuggingInsight), "DebuggingInsight");
    }

    #[test]
    fn test_memory_type_descriptions_are_non_empty() {
        for mt in MemoryType::all() {
            assert!(!mt.description().is_empty());
        }
    }

    #[test]
    fn test_memory_type_scope_directories() {
        assert_eq!(MemoryType::UserPreference.scope_directory(), "user");
        assert_eq!(MemoryType::ProjectConvention.scope_directory(), "project");
        assert_eq!(MemoryType::TechnicalDecision.scope_directory(), "project");
        assert_eq!(MemoryType::DebuggingInsight.scope_directory(), "shared");
    }

    #[test]
    fn test_memory_type_to_category_conversion() {
        assert_eq!(MemoryCategory::from(MemoryType::UserPreference), MemoryCategory::Preference);
        assert_eq!(MemoryCategory::from(MemoryType::ProjectConvention), MemoryCategory::Pattern);
        assert_eq!(MemoryCategory::from(MemoryType::TechnicalDecision), MemoryCategory::Decision);
        assert_eq!(MemoryCategory::from(MemoryType::DebuggingInsight), MemoryCategory::Error);
    }

    #[test]
    fn test_memory_category_display() {
        assert_eq!(format!("{}", MemoryCategory::Preference), "preference");
        assert_eq!(format!("{}", MemoryCategory::Pattern), "pattern");
        assert_eq!(format!("{}", MemoryCategory::Decision), "decision");
        assert_eq!(format!("{}", MemoryCategory::Error), "error");
        assert_eq!(format!("{}", MemoryCategory::Context), "context");
    }

    #[test]
    fn test_session_memory_config_defaults() {
        let config = SessionMemoryConfig::default();
        assert!(config.auto_extract_enabled);
        assert_eq!(config.max_memories_per_category, 100);
        assert_eq!(config.memory_ttl, Duration::days(90));
        assert!(config.consolidation_enabled);
    }

    #[test]
    fn test_memory_entry_serialization_roundtrip() {
        let entry = MemoryEntry::with_confidence(
            "test-proj", MemoryCategory::Decision, "Use React for frontend", 0.92,
            vec!["frontend".to_string(), "react".to_string()],
        ).unwrap();
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: MemoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, entry.id);
        assert_eq!(deserialized.project, entry.project);
        assert_eq!(deserialized.category, entry.category);
        assert_eq!(deserialized.content, entry.content);
        assert_eq!(deserialized.tags, entry.tags);
        assert!((deserialized.confidence - entry.confidence).abs() < f64::EPSILON);
        assert_eq!(deserialized.access_count, entry.access_count);
    }

    #[test]
    fn test_session_memory_config_serialization_roundtrip() {
        let config = SessionMemoryConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: SessionMemoryConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.auto_extract_enabled, config.auto_extract_enabled);
        assert_eq!(deserialized.max_memories_per_category, config.max_memories_per_category);
        assert_eq!(deserialized.consolidation_enabled, config.consolidation_enabled);
    }
}
