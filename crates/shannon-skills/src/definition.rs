//! Core skill definition and types

use crate::frontmatter::{ExecutionContext, HooksConfig};
use shannon_types::Timestamp;
use std::path::PathBuf;

/// Unique identifier for a skill
pub type SkillId = String;

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
    #[allow(dead_code)]
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
        self.paths.as_ref().map_or(false, |patterns| {
            patterns.iter().any(|pattern| {
                // Simple glob matching - ignore library would handle this properly
                if pattern == "**" {
                    return true;
                }
                if pattern.ends_with("/**") {
                    let prefix = &pattern[..pattern.len() - 3];
                    return path.starts_with(prefix);
                }
                path == pattern || path.starts_with(&format!("{}/", pattern))
            })
        })
    }

    /// Check if this skill can be invoked by users
    pub fn is_user_invocable(&self) -> bool {
        self.user_invocable
    }

    /// Check if this skill is conditional (requires path match)
    pub fn is_conditional(&self) -> bool {
        self.paths.as_ref().map_or(false, |p| !p.is_empty())
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
}
