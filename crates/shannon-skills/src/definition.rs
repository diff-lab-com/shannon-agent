//! Core skill definition and types

use crate::frontmatter::{ExecutionContext, HooksConfig};
use shannon_types::Timestamp;
use std::path::PathBuf;

/// Unique identifier for a skill
pub type SkillId = String;

/// Lightweight metadata for a skill, always held in memory.
///
/// Contains only the information extracted from YAML frontmatter, without
/// the potentially large markdown body. Used for LLM context injection and
/// skill listing where the full content is not needed.
#[derive(Debug, Clone)]
pub struct SkillMetadata {
    /// Unique identifier
    pub id: SkillId,
    /// Display name
    pub name: String,
    /// Short description
    pub description: String,
    /// Aliases for invocation
    pub aliases: Vec<String>,
    /// When to use this skill (trigger pattern)
    pub when_to_use: Option<String>,
    /// Argument hint
    pub argument_hint: Option<String>,
    /// Allowed tools
    pub allowed_tools: Vec<String>,
    /// Can users invoke directly?
    pub user_invocable: bool,
    /// Is this skill hidden?
    pub is_hidden: bool,
    /// Where this skill was loaded from
    pub source: SkillSource,
    /// Path to the skill file (needed for full loading on demand)
    pub file_path: Option<PathBuf>,
}

impl SkillMetadata {
    /// Estimate the number of tokens this metadata would consume when
    /// formatted for LLM injection.
    ///
    /// Uses a rough heuristic of 1 token per 4 characters.
    pub fn estimated_tokens(&self) -> usize {
        let mut len = self.name.len() + self.description.len() + 2; // ": "
        if let Some(ref hint) = self.argument_hint {
            len += hint.len() + 4; // " []"
        }
        // Divide by ~4 chars per token, minimum 1
        (len / 4).max(1)
    }
}

impl From<&Skill> for SkillMetadata {
    fn from(skill: &Skill) -> Self {
        Self {
            id: skill.id.clone(),
            name: skill.name.clone(),
            description: skill.description.clone(),
            aliases: skill.aliases.clone(),
            when_to_use: skill.when_to_use.clone(),
            argument_hint: skill.argument_hint.clone(),
            allowed_tools: skill.allowed_tools.clone(),
            user_invocable: skill.user_invocable,
            is_hidden: skill.is_hidden,
            source: skill.source.clone(),
            file_path: skill.file_path.clone(),
        }
    }
}

/// Full skill content, loaded on demand when a skill is invoked.
///
/// Contains the complete [`Skill`] including the markdown body.
/// Loaded from disk only when needed and cached for subsequent access.
#[derive(Debug, Clone)]
pub struct SkillFull {
    /// The underlying complete skill
    pub skill: Skill,
}

impl SkillFull {
    /// Create a new `SkillFull` wrapping a complete [`Skill`].
    pub fn new(skill: Skill) -> Self {
        Self { skill }
    }

    /// Get a reference to the markdown body content.
    pub fn content(&self) -> &str {
        &self.skill.content
    }

    /// Get a reference to the underlying skill.
    pub fn as_skill(&self) -> &Skill {
        &self.skill
    }
}

impl From<Skill> for SkillFull {
    fn from(skill: Skill) -> Self {
        Self::new(skill)
    }
}

/// Where a skill was loaded from
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSource {
    /// Bundled with the application
    Bundled,
    /// User's local skills directory
    User,
    /// Project-specific skills
    Project,
    /// Managed/policy skills
    Managed,
    /// From an MCP server
    Mcp,
    /// Legacy commands directory
    CommandsDeprecated,
    /// Plugin provided
    Plugin,
}

/// Complete skill definition
#[derive(Debug, Clone)]
pub struct Skill {
    /// Unique identifier
    pub id: SkillId,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// Aliases for invocation
    pub aliases: Vec<String>,
    /// When to use this skill
    pub when_to_use: Option<String>,
    /// Argument hint
    pub argument_hint: Option<String>,
    /// Allowed tools
    pub allowed_tools: Vec<String>,
    /// Model override
    pub model: Option<String>,
    /// Disable model invocation
    pub disable_model_invocation: bool,
    /// Can users invoke directly?
    pub user_invocable: bool,
    /// Hooks configuration
    pub hooks: Option<HooksConfig>,
    /// Execution context
    pub context: Option<ExecutionContext>,
    /// Assigned agent
    pub agent: Option<String>,
    /// File paths that trigger this skill
    pub paths: Option<Vec<String>>,
    /// Version
    pub version: Option<String>,
    /// Where this skill was loaded from
    pub source: SkillSource,
    /// Base directory for skill files
    pub skill_root: Option<PathBuf>,
    /// Path to the skill file
    pub file_path: Option<PathBuf>,
    /// The markdown content
    pub content: String,
    /// Content length in characters
    pub content_length: usize,
    /// Is this skill hidden?
    pub is_hidden: bool,
    /// Creation timestamp
    pub created_at: Timestamp,
    /// Last modified timestamp
    pub updated_at: Option<Timestamp>,
}

impl Skill {
    /// Create a new skill definition
    pub fn new(
        id: SkillId,
        name: String,
        description: String,
        content: String,
    ) -> Self {
        let content_length = content.len();
        let now = chrono::Utc::now();
        Self {
            id,
            name: name.clone(),
            description,
            aliases: Vec::new(),
            when_to_use: None,
            argument_hint: None,
            allowed_tools: Vec::new(),
            model: None,
            disable_model_invocation: false,
            user_invocable: true,
            hooks: None,
            context: None,
            agent: None,
            paths: None,
            version: None,
            source: SkillSource::User,
            skill_root: None,
            file_path: None,
            content,
            content_length,
            is_hidden: false,
            created_at: now,
            updated_at: None,
        }
    }

    /// Get the user-facing name for this skill
    pub fn user_facing_name(&self) -> &str {
        &self.name
    }

    /// Check if this skill should be invoked for the given path
    pub fn matches_path(&self, path: &str) -> bool {
        self.paths.as_ref().is_some_and(|patterns| {
            patterns.iter().any(|pattern| {
                // Simple glob matching - ignore library would handle this properly
                if pattern == "**" {
                    return true;
                }
                if pattern.ends_with("/**") {
                    let prefix = &pattern[..pattern.len() - 3];
                    return path.starts_with(prefix);
                }
                path == pattern || path.starts_with(&format!("{pattern}/"))
            })
        })
    }

    /// Check if this skill can be invoked by users
    pub fn is_user_invocable(&self) -> bool {
        self.user_invocable
    }

    /// Check if this skill is conditional (requires path match)
    pub fn is_conditional(&self) -> bool {
        self.paths.as_ref().is_some_and(|p| !p.is_empty())
    }
}

/// Context for executing a skill
#[derive(Debug, Clone)]
pub struct SkillContext {
    /// Arguments passed to the skill
    pub arguments: Vec<String>,
    /// Current working directory
    pub cwd: std::path::PathBuf,
    /// Session ID
    pub session_id: String,
    /// Tool permission context
    pub permissions: SkillPermissions,
}

/// Permissions for skill execution
#[derive(Debug, Clone)]
pub struct SkillPermissions {
    /// Always allowed tools
    pub allowed_tools: Vec<String>,
    /// Whether shell commands are allowed
    pub allow_shell: bool,
    /// Whether file operations are allowed
    pub allow_file_ops: bool,
}

impl Default for SkillPermissions {
    fn default() -> Self {
        Self {
            allowed_tools: Vec::new(),
            allow_shell: true,
            allow_file_ops: true,
        }
    }
}

/// Result of executing a skill
#[derive(Debug, Clone)]
pub struct SkillResult {
    /// The skill that was executed
    pub skill_id: SkillId,
    /// Generated prompt content
    pub prompt_content: String,
    /// Whether model invocation should be skipped
    pub skip_model_invocation: bool,
    /// Any additional metadata
    pub metadata: SkillResultMetadata,
}

/// Metadata about skill execution
#[derive(Debug, Clone)]
pub struct SkillResultMetadata {
    /// Execution timestamp
    pub executed_at: Timestamp,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
    /// Whether shell commands were executed
    pub had_shell_commands: bool,
}

impl SkillResult {
    /// Create a new skill result
    pub fn new(skill_id: SkillId, prompt_content: String) -> Self {
        Self {
            skill_id,
            prompt_content,
            skip_model_invocation: false,
            metadata: SkillResultMetadata {
                executed_at: chrono::Utc::now(),
                duration_ms: 0,
                had_shell_commands: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_creation() {
        let skill = Skill::new(
            "test-skill".to_string(),
            "Test Skill".to_string(),
            "A test skill".to_string(),
            "Content".to_string(),
        );
        assert_eq!(skill.id, "test-skill");
        assert_eq!(skill.name, "Test Skill");
        assert!(skill.is_user_invocable());
    }

    #[test]
    fn test_path_matching() {
        let mut skill = Skill::new(
            "test".to_string(),
            "Test".to_string(),
            "A test".to_string(),
            "Content".to_string(),
        );
        skill.paths = Some(vec!["src".to_string()]); // Simple path match

        assert!(skill.matches_path("src/main.rs"));
        assert!(skill.matches_path("src/lib.rs"));
        assert!(!skill.matches_path("tests/test.rs"));
    }

    #[test]
    fn test_skill_metadata_from_skill() {
        let mut skill = Skill::new(
            "commit".to_string(),
            "commit".to_string(),
            "Generate git commits with conventional messages".to_string(),
            "# Commit skill body\n\nUse this to commit.".to_string(),
        );
        skill.aliases = vec!["ci".to_string()];
        skill.allowed_tools = vec!["bash".to_string()];
        skill.argument_hint = Some("<message>".to_string());

        let meta = SkillMetadata::from(&skill);
        assert_eq!(meta.id, "commit");
        assert_eq!(meta.name, "commit");
        assert_eq!(meta.description, "Generate git commits with conventional messages");
        assert_eq!(meta.aliases, vec!["ci".to_string()]);
        assert_eq!(meta.allowed_tools, vec!["bash".to_string()]);
        assert_eq!(meta.argument_hint, Some("<message>".to_string()));
        assert!(meta.user_invocable);
    }

    #[test]
    fn test_skill_metadata_estimated_tokens() {
        let skill = Skill::new(
            "commit".to_string(),
            "commit".to_string(),
            "Generate git commits".to_string(),
            "Body content".to_string(),
        );
        let meta = SkillMetadata::from(&skill);
        let tokens = meta.estimated_tokens();
        // Should be non-zero and reasonable
        assert!(tokens > 0);
        assert!(tokens < 100); // Short name + description shouldn't be many tokens
    }

    #[test]
    fn test_skill_metadata_estimated_tokens_minimum() {
        let skill = Skill::new(
            "a".to_string(),
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
        );
        let meta = SkillMetadata::from(&skill);
        // Even tiny metadata should estimate at least 1 token
        assert!(meta.estimated_tokens() >= 1);
    }

    #[test]
    fn test_skill_full_new() {
        let skill = Skill::new(
            "test-skill".to_string(),
            "Test".to_string(),
            "A test".to_string(),
            "Body content here".to_string(),
        );
        let full = SkillFull::new(skill);
        assert_eq!(full.content(), "Body content here");
        assert_eq!(full.as_skill().name, "Test");
    }

    #[test]
    fn test_skill_full_from_skill() {
        let skill = Skill::new(
            "review".to_string(),
            "review".to_string(),
            "Review code".to_string(),
            "# Review\n\nCheck code quality.".to_string(),
        );
        let full: SkillFull = skill.into();
        assert_eq!(full.as_skill().id, "review");
    }
}
