//! # CLAUDE.md Configuration System
//!
//! Handles discovery, parsing, and injection of CLAUDE.md configuration files
//! into AI system prompts.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during CLAUDE.md processing
#[derive(Error, Debug)]
pub enum ClaudeMdError {
    #[error("Failed to read CLAUDE.md file: {0}")]
    ReadError(std::io::Error),

    #[error("Failed to parse CLAUDE.md: {0}")]
    ParseError(String),

    #[error("Invalid frontmatter: {0}")]
    InvalidFrontmatter(String),

    #[error("CLAUDE.md not found in path: {0}")]
    NotFound(String),
}

/// Parsed CLAU.md configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeMdConfig {
    /// Frontmatter metadata
    pub metadata: ClaudeMdMetadata,
    /// Main content (instructions, guidelines, etc.)
    pub content: String,
    /// All context including instructions, ignore patterns, etc.
    pub instructions: String,
}

/// Frontmatter metadata from CLAUDE.md
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeMdMetadata {
    /// Priority for this configuration (higher = more important)
    pub priority: i32,
    /// Whether to disable auto memory
    pub disable_auto_memory: bool,
    /// Custom model override
    pub model: Option<String>,
    /// Temperature override
    pub temperature: Option<f32>,
    /// Max tokens override
    pub max_tokens: Option<u32>,
    /// Specific instructions for different contexts
    pub instructions_for: Option<serde_json::Value>,
    /// Tool permissions
    pub tool_permissions: Option<serde_json::Value>,
}

impl Default for ClaudeMdMetadata {
    fn default() -> Self {
        Self {
            priority: 0,
            disable_auto_memory: false,
            model: None,
            temperature: None,
            max_tokens: None,
            instructions_for: None,
            tool_permissions: None,
        }
    }
}

/// Result of CLAUDE.md search
#[derive(Debug, Clone)]
pub struct ClaudeMdSearchResult {
    /// Path to the CLAUDE.md file found
    pub path: PathBuf,
    /// Parsed configuration
    pub config: ClaudeMdConfig,
}

/// CLAUDE.md manager for discovery and parsing
pub struct ClaudeMdManager {
    /// Base directory to search from
    base_dir: PathBuf,
}

impl ClaudeMdManager {
    /// Create a new CLAUDE.md manager
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Create from current directory
    pub fn from_current_dir() -> Result<Self, std::io::Error> {
        Ok(Self {
            base_dir: std::env::current_dir()?,
        })
    }

    /// Find and parse the nearest CLAUDE.md file
    /// Searches from base_dir upward through parent directories
    pub fn find_nearest(&self) -> Result<Option<ClaudeMdSearchResult>, ClaudeMdError> {
        self.find_nearest_from(&self.base_dir)
    }

    /// Find and parse the nearest CLAUDE.md file from a specific directory
    pub fn find_nearest_from(&self, start_dir: &Path) -> Result<Option<ClaudeMdSearchResult>, ClaudeMdError> {
        let mut current_dir = start_dir.to_path_buf();

        // Search upward through parent directories
        loop {
            let claude_md_path = current_dir.join("CLAUDE.md");

            if claude_md_path.exists() {
                // Found CLAUDE.md, parse it
                let config = self.parse_claude_md(&claude_md_path)?;
                return Ok(Some(ClaudeMdSearchResult {
                    path: claude_md_path,
                    config,
                }));
            }

            // Move to parent directory
            match current_dir.parent() {
                Some(parent) => {
                    // Prevent infinite loop at filesystem root
                    if parent == current_dir {
                        // Reached root, no CLAUDE.md found
                        return Ok(None);
                    }
                    current_dir = parent.to_path_buf();
                }
                None => {
                    // No parent directory, we're at root
                    return Ok(None);
                }
            }
        }
    }

    /// Find all CLAUDE.md files in current directory and subdirectories
    pub fn find_all(&self) -> Result<Vec<ClaudeMdSearchResult>, ClaudeMdError> {
        let mut results = Vec::new();
        self.find_all_recursive(&self.base_dir, &mut results)?;
        Ok(results)
    }

    /// Recursively search for CLAUDE.md files
    fn find_all_recursive(
        &self,
        dir: &Path,
        results: &mut Vec<ClaudeMdSearchResult>,
    ) -> Result<(), ClaudeMdError> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                // Skip directories we can't read
                return Ok(());
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();

            if path.is_dir() {
                // Skip hidden directories and common skip directories
                let file_name = match path.file_name() {
                    Some(name) => name.to_string_lossy().to_string(),
                    None => continue,
                };

                if file_name.starts_with('.')
                    || file_name == "node_modules"
                    || file_name == "target"
                    || file_name == ".git"
                {
                    continue;
                }

                self.find_all_recursive(&path, results)?;
            } else if path.file_name() == Some(std::ffi::OsStr::new("CLAUDE.md")) {
                // Found CLAUDE.md, parse it
                let config = self.parse_claude_md(&path)?;
                results.push(ClaudeMdSearchResult {
                    path,
                    config,
                });
            }
        }

        Ok(())
    }

    /// Parse a CLAUDE.md file
    fn parse_claude_md(&self, path: &Path) -> Result<ClaudeMdConfig, ClaudeMdError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ClaudeMdError::ReadError(e))?;

        self.parse_claude_md_content(&content, path)
    }

    /// Parse CLAUDE.md content
    pub fn parse_claude_md_content(&self, content: &str, path: &Path) -> Result<ClaudeMdConfig, ClaudeMdError> {
        // Check for YAML frontmatter (--- delimited)
        let (metadata, body) = if content.starts_with("---") {
            // Has YAML frontmatter
            let parts: Vec<&str> = content.splitn(3, "---").collect();
            if parts.len() < 2 {
                return Err(ClaudeMdError::InvalidFrontmatter(
                    "Frontmatter not properly closed with ---".to_string()
                ));
            }

            let frontmatter = parts.get(1).unwrap_or(&"");
            let main_content = parts[2..].join("---").trim().to_string();

            // Parse YAML frontmatter
            let metadata: ClaudeMdMetadata = serde_yaml::from_str(frontmatter)
                .map_err(|e| ClaudeMdError::InvalidFrontmatter(format!("YAML parsing error: {}", e)))?;

            (metadata, main_content)
        } else {
            // No frontmatter, use defaults
            (ClaudeMdMetadata::default(), content.to_string())
        };

        // Extract instructions (everything that's not comments)
        let instructions = self.extract_instructions(&body);

        Ok(ClaudeMdConfig {
            metadata,
            content: body,
            instructions,
        })
    }

    /// Extract instructions from content, filtering out comments
    fn extract_instructions(&self, content: &str) -> String {
        content
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                // Skip empty lines and comment-only lines
                let is_comment = trimmed.starts_with("//") || trimmed.starts_with('#');
                !trimmed.is_empty() && !is_comment
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Format CLAUDE.md config for injection into system prompt
    pub fn format_for_prompt(&self, config: &ClaudeMdConfig) -> String {
        let mut parts = Vec::new();

        // Add priority header
        parts.push(format!("=== CLAUDE.md Instructions (priority: {}) ===", config.metadata.priority));

        // Add model overrides if present
        if let Some(ref model) = config.metadata.model {
            parts.push(format!("Model Override: {}", model));
        }
        if config.metadata.disable_auto_memory {
            parts.push("Auto Memory: Disabled".to_string());
        }

        // Add main instructions
        if !config.instructions.is_empty() {
            parts.push(format!("\n{}", config.instructions));
        }

        parts.join("\n")
    }

    /// Discover and load CLAUDE.md with automatic search
    pub fn discover_and_load(&self) -> Result<Vec<ClaudeMdConfig>, ClaudeMdError> {
        // First try to find the nearest CLAUDE.md (searching upward)
        if let Some(result) = self.find_nearest()? {
            return Ok(vec![result.config]);
        }

        // If no nearest found, search for all in current directory tree
        let results = self.find_all()?;
        if results.is_empty() {
            return Ok(Vec::new());
        }

        // Sort by priority (descending)
        let mut configs: Vec<_> = results.into_iter().map(|r| r.config).collect();
        configs.sort_by(|a, b| b.metadata.priority.cmp(&a.metadata.priority));

        Ok(configs)
    }

    /// Build system prompt incorporating CLAUDE.md instructions
    pub fn build_system_prompt(&self, configs: &[ClaudeMdConfig]) -> String {
        if configs.is_empty() {
            return String::new();
        }

        let mut parts = Vec::new();

        parts.push("=== Project-Specific Instructions (from CLAUDE.md) ===".to_string());

        for config in configs {
            parts.push(self.format_for_prompt(config));
            parts.push("\n---\n".to_string());
        }

        parts.join("\n")
    }
}

impl Default for ClaudeMdManager {
    fn default() -> Self {
        Self::from_current_dir().unwrap_or_else(|_| Self {
            base_dir: PathBuf::from(".")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_claude_md_without_frontmatter() {
        let manager = ClaudeMdManager::new(PathBuf::from("."));
        let content = "This is a test instruction.\nAnother line.";
        let config = manager.parse_claude_md_content(content, Path::new("test.md")).unwrap();

        assert_eq!(config.metadata.priority, 0);
        assert_eq!(config.content, content);
        assert!(!config.instructions.is_empty());
    }

    #[test]
    fn test_parse_claude_md_with_frontmatter() {
        let manager = ClaudeMdManager::new(PathBuf::from("."));
        let content = r#"---
priority: 10
model: claude-3-opus
disable_auto_memory: false
---
This is a test instruction with frontmatter."#;

        let config = manager.parse_claude_md_content(content, Path::new("test.md")).unwrap();

        assert_eq!(config.metadata.priority, 10);
        assert_eq!(config.metadata.model.as_deref(), Some("claude-3-opus"));
        assert!(config.content.contains("This is a test instruction"));
    }

    #[test]
    fn test_extract_instructions() {
        let manager = ClaudeMdManager::new(PathBuf::from("."));
        let content = r#"// This is a comment
Real instruction here
# Another comment
Another instruction"#;

        let instructions = manager.extract_instructions(content);
        assert!(instructions.contains("Real instruction"));
        assert!(instructions.contains("Another instruction"));
        assert!(!instructions.contains("// This is a comment"));
    }

    #[test]
    fn test_format_for_prompt() {
        let manager = ClaudeMdManager::new(PathBuf::from("."));
        let config = ClaudeMdConfig {
            metadata: ClaudeMdMetadata {
                priority: 5,
                model: Some("claude-3-5-sonnet".to_string()),
                disable_auto_memory: true,
                ..Default::default()
            },
            content: "Test content".to_string(),
            instructions: "Test content".to_string(),
        };

        let prompt = manager.format_for_prompt(&config);
        assert!(prompt.contains("priority: 5"));
        assert!(prompt.contains("Model Override: claude-3-5-sonnet"));
        assert!(prompt.contains("Auto Memory: Disabled"));
        assert!(prompt.contains("Test content"));
    }
}
