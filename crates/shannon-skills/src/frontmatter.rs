//! Frontmatter parsing for skill definitions

use crate::error::{SkillError, SkillResult};
use serde::Deserialize;
use std::collections::HashMap;

/// Parsed frontmatter from a skill file
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct SkillFrontmatter {
    /// Display name (defaults to directory/file name)
    pub name: Option<String>,

    /// Description of what the skill does
    pub description: Option<String>,

    /// Alternative names for invoking the skill
    #[serde(rename = "alias")]
    pub aliases: Option<Vec<String>>,

    /// When to use this skill
    #[serde(rename = "when_to_use")]
    pub when_to_use: Option<String>,

    /// Hint for arguments
    #[serde(rename = "argument-hint")]
    pub argument_hint: Option<String>,

    /// Allowed tools for this skill
    #[serde(rename = "allowed-tools")]
    pub allowed_tools: Option<Vec<String>>,

    /// Model override for this skill
    pub model: Option<String>,

    /// Disable model invocation (prompt-only skill)
    #[serde(rename = "disable-model-invocation")]
    pub disable_model_invocation: Option<bool>,

    /// Whether user can invoke this skill directly
    #[serde(rename = "user-invocable")]
    pub user_invocable: Option<bool>,

    /// Hooks configuration
    pub hooks: Option<HooksConfig>,

    /// Execution context: 'inline' or 'fork'
    pub context: Option<ExecutionContext>,

    /// Agent assignment
    pub agent: Option<String>,

    /// File paths that trigger this skill
    pub paths: Option<Vec<String>>,

    /// Version of the skill
    pub version: Option<String>,

    /// Arguments schema
    pub arguments: Option<ArgumentConfig>,
}

/// Execution context for skills
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionContext {
    /// Execute in the same process
    Inline,
    /// Execute in a forked process
    Fork,
}

/// Hooks configuration
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct HooksConfig {
    /// Pre-sampling hooks
    #[serde(rename = "preSamplingHook")]
    pub pre_sampling: Option<Vec<String>>,

    /// Post-sampling hooks
    #[serde(rename = "postSamplingHook")]
    pub post_sampling: Option<Vec<String>>,
}

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            pre_sampling: None,
            post_sampling: None,
        }
    }
}

/// Argument configuration
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ArgumentConfig {
    /// Single argument name
    Single(String),
    /// Multiple argument names
    Multiple(Vec<String>),
}

impl ArgumentConfig {
    /// Get all argument names as a vector
    pub fn names(&self) -> Vec<String> {
        match self {
            ArgumentConfig::Single(name) => vec![name.clone()],
            ArgumentConfig::Multiple(names) => names.clone(),
        }
    }
}

/// Parsed skill content with frontmatter and body
#[derive(Debug, Clone)]
pub struct ParsedSkill {
    /// The frontmatter metadata
    pub frontmatter: SkillFrontmatter,
    /// The markdown content body
    pub body: String,
    /// The raw full content
    pub raw: String,
}

/// Parse frontmatter and body from skill markdown content
pub fn parse_skill_frontmatter(content: &str, source: &str) -> SkillResult<ParsedSkill> {
    let (frontmatter_str, body) = extract_frontmatter(content)?;

    let frontmatter: SkillFrontmatter = if frontmatter_str.trim().is_empty() {
        SkillFrontmatter::default()
    } else {
        serde_yaml::from_str(frontmatter_str)
            .map_err(|e| SkillError::FrontmatterParse {
                name: source.to_string(),
                message: e.to_string(),
            })?
    };

    Ok(ParsedSkill {
        frontmatter,
        body: body.trim().to_string(),
        raw: content.to_string(),
    })
}

/// Extract frontmatter (between --- markers) and body from content
fn extract_frontmatter(content: &str) -> SkillResult<(&str, &str)> {
    if !content.starts_with("---") {
        // No frontmatter, treat entire content as body
        return Ok(("", content));
    }

    let rest = &content[3..]; // Skip opening ---
    let end_idx = rest
        .find("\n---")
        .ok_or_else(|| SkillError::InvalidFormat("Missing closing --- in frontmatter".into()))?;

    let frontmatter = &rest[..end_idx];
    let body_start = end_idx + 4; // Skip \n---
    let body = &rest[body_start..];

    Ok((frontmatter, body))
}

/// Parse shell command configuration from frontmatter
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ShellConfig {
    /// Shell command to execute
    pub shell: Option<String>,
    /// Working directory
    #[serde(rename = "cwd")]
    pub working_dir: Option<String>,
    /// Environment variables
    pub env: Option<HashMap<String, String>>,
}

/// Parse effort level from frontmatter
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EffortLevel {
    Minimal,
    Low,
    Medium,
    High,
    Maximum,
}

impl Default for EffortLevel {
    fn default() -> Self {
        Self::Medium
    }
}

impl std::str::FromStr for EffortLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "minimal" => Ok(Self::Minimal),
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "maximum" => Ok(Self::Maximum),
            _ => Err(format!("Invalid effort level: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter() {
        let content = r#"---
name: test-skill
description: A test skill
---
This is the body."#;

        let parsed = parse_skill_frontmatter(content, "test").unwrap();
        assert_eq!(parsed.frontmatter.name, Some("test-skill".to_string()));
        assert_eq!(parsed.frontmatter.description, Some("A test skill".to_string()));
        assert_eq!(parsed.body, "This is the body.");
    }

    #[test]
    fn test_no_frontmatter() {
        let content = "Just the body, no frontmatter";
        let parsed = parse_skill_frontmatter(content, "test").unwrap();
        assert!(parsed.frontmatter.name.is_none());
        assert_eq!(parsed.body, content);
    }

    #[test]
    fn test_extract_frontmatter() {
        let content = r#"---
key: value
---
body content"#;

        let (fm, body) = extract_frontmatter(content).unwrap();
        // Based on the actual implementation:
        // - rest skips the opening "---"
        // - find("\n---") finds the position of the closing delimiter
        // - body starts after the closing "\n---" (4 chars)
        // The actual result has \n before body content
        assert_eq!(fm, "\nkey: value");
        assert_eq!(body, "\nbody content");
    }
}
