//! # Project Memory Configuration System
//!
//! Handles discovery, parsing, and injection of project memory configuration files
//! (CLAUDE.md and SHANNON.md) into AI system prompts.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during project memory file processing
#[derive(Error, Debug)]
pub enum ProjectMemoryError {
    #[error("Failed to read memory file: {0}")]
    ReadError(std::io::Error),

    #[error("Failed to parse memory file: {0}")]
    ParseError(String),

    #[error("Invalid frontmatter: {0}")]
    InvalidFrontmatter(String),

    #[error("Memory file not found in path: {0}")]
    NotFound(String),
}

/// Parsed project memory configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryConfig {
    /// Frontmatter metadata
    pub metadata: ProjectMemoryMetadata,
    /// Main content (instructions, guidelines, etc.)
    pub content: String,
    /// All context including instructions, ignore patterns, etc.
    pub instructions: String,
}

/// Frontmatter metadata from a project memory file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryMetadata {
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

impl Default for ProjectMemoryMetadata {
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

/// Result of a project memory file search
#[derive(Debug, Clone)]
pub struct ProjectMemorySearchResult {
    /// Path to the memory file found
    pub path: PathBuf,
    /// Parsed configuration
    pub config: ProjectMemoryConfig,
}

/// A source of project memory, used in merged loading
#[derive(Debug, Clone)]
pub struct MemorySource {
    /// Path to the memory file
    pub path: PathBuf,
    /// Parsed configuration from this source
    pub config: ProjectMemoryConfig,
}

/// Merged memory from multiple sources
#[derive(Debug, Clone)]
pub struct MergedMemory {
    /// Combined instructions from all sources (later sources override earlier ones)
    pub instructions: String,
    /// Merged metadata (later sources override earlier ones)
    pub metadata: ProjectMemoryMetadata,
    /// All sources that contributed to this merged memory
    pub sources: Vec<MemorySource>,
}

/// Project memory manager for discovery and parsing
pub struct ProjectMemoryManager {
    /// Base directory to search from
    base_dir: PathBuf,
}

/// Supported memory file names
const MEMORY_FILE_NAMES: &[&str] = &["CLAUDE.md", "SHANNON.md"];

impl ProjectMemoryManager {
    /// Create a new project memory manager
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Create from current directory
    pub fn from_current_dir() -> Result<Self, std::io::Error> {
        Ok(Self {
            base_dir: std::env::current_dir()?,
        })
    }

    /// Find and parse the nearest project memory file
    /// Searches from base_dir upward through parent directories.
    /// Checks for both CLAUDE.md and SHANNON.md, returning the first found.
    pub fn find_nearest(&self) -> Result<Option<ProjectMemorySearchResult>, ProjectMemoryError> {
        self.find_nearest_from(&self.base_dir)
    }

    /// Find and parse the nearest project memory file from a specific directory.
    /// Searches for both CLAUDE.md and SHANNON.md, returning the first found.
    pub fn find_nearest_from(&self, start_dir: &Path) -> Result<Option<ProjectMemorySearchResult>, ProjectMemoryError> {
        let mut current_dir = start_dir.to_path_buf();

        // Search upward through parent directories
        loop {
            for &file_name in MEMORY_FILE_NAMES {
                let memory_path = current_dir.join(file_name);

                if memory_path.exists() {
                    // Found a memory file, parse it
                    let config = self.parse_memory_file(&memory_path)?;
                    return Ok(Some(ProjectMemorySearchResult {
                        path: memory_path,
                        config,
                    }));
                }
            }

            // Move to parent directory
            match current_dir.parent() {
                Some(parent) => {
                    // Prevent infinite loop at filesystem root
                    if parent == current_dir {
                        // Reached root, no memory file found
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

    /// Find all project memory files in current directory and subdirectories.
    /// Finds both CLAUDE.md and SHANNON.md files.
    pub fn find_all(&self) -> Result<Vec<ProjectMemorySearchResult>, ProjectMemoryError> {
        let mut results = Vec::new();
        self.find_all_recursive(&self.base_dir, &mut results)?;
        Ok(results)
    }

    /// Recursively search for project memory files (CLAUDE.md and SHANNON.md)
    fn find_all_recursive(
        &self,
        dir: &Path,
        results: &mut Vec<ProjectMemorySearchResult>,
    ) -> Result<(), ProjectMemoryError> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => {
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
            } else if MEMORY_FILE_NAMES.iter().any(|&name| {
                path.file_name() == Some(std::ffi::OsStr::new(name))
            }) {
                // Found a memory file, parse it
                let config = self.parse_memory_file(&path)?;
                results.push(ProjectMemorySearchResult {
                    path,
                    config,
                });
            }
        }

        Ok(())
    }

    /// Parse a project memory file
    fn parse_memory_file(&self, path: &Path) -> Result<ProjectMemoryConfig, ProjectMemoryError> {
        let content = std::fs::read_to_string(path)
            .map_err(ProjectMemoryError::ReadError)?;

        self.parse_memory_file_content(&content, path)
    }

    /// Parse project memory file content
    pub fn parse_memory_file_content(&self, content: &str, _path: &Path) -> Result<ProjectMemoryConfig, ProjectMemoryError> {
        // Check for YAML frontmatter (--- delimited)
        let (metadata, body) = if content.starts_with("---") {
            // Has YAML frontmatter
            let parts: Vec<&str> = content.splitn(3, "---").collect();
            if parts.len() < 2 {
                return Err(ProjectMemoryError::InvalidFrontmatter(
                    "Frontmatter not properly closed with ---".to_string()
                ));
            }

            let frontmatter = parts.get(1).unwrap_or(&"");
            let main_content = parts[2..].join("---").trim().to_string();

            // Parse YAML frontmatter
            let metadata: ProjectMemoryMetadata = serde_yaml::from_str(frontmatter)
                .map_err(|e| ProjectMemoryError::InvalidFrontmatter(format!("YAML parsing error: {}", e)))?;

            (metadata, main_content)
        } else {
            // No frontmatter, use defaults
            (ProjectMemoryMetadata::default(), content.to_string())
        };

        // Extract instructions (everything that's not comments)
        let instructions = self.extract_instructions(&body);

        Ok(ProjectMemoryConfig {
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

    /// Format memory config for injection into system prompt
    pub fn format_for_prompt(&self, config: &ProjectMemoryConfig) -> String {
        let mut parts = Vec::new();

        // Add priority header
        parts.push(format!("=== Project Memory Instructions (priority: {}) ===", config.metadata.priority));

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

    /// Discover and load project memory with automatic search
    pub fn discover_and_load(&self) -> Result<Vec<ProjectMemoryConfig>, ProjectMemoryError> {
        // First try to find the nearest memory file (searching upward)
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

    /// Build system prompt incorporating project memory instructions
    pub fn build_system_prompt(&self, configs: &[ProjectMemoryConfig]) -> String {
        if configs.is_empty() {
            return String::new();
        }

        let mut parts = Vec::new();

        parts.push("=== Project-Specific Instructions (from project memory) ===".to_string());

        for config in configs {
            parts.push(self.format_for_prompt(config));
            parts.push("\n---\n".to_string());
        }

        parts.join("\n")
    }

    /// Load and merge memory from multiple sources in priority order.
    ///
    /// Searches for memory files in the following order (later entries override earlier ones):
    /// 1. Global `~/.shannon/CLAUDE.md`
    /// 2. Global `~/.shannon/SHANNON.md`
    /// 3. Project `CLAUDE.md` (found at base_dir)
    /// 4. Project `SHANNON.md` (found at base_dir)
    pub fn load_merged(&self) -> Result<MergedMemory, ProjectMemoryError> {
        let mut sources: Vec<MemorySource> = Vec::new();
        let mut merged_instructions: Vec<String> = Vec::new();
        let mut merged_metadata = ProjectMemoryMetadata::default();

        // 1. Global CLAUDE.md
        if let Some(home) = dirs_home() {
            let global_claude = home.join(".shannon").join("CLAUDE.md");
            if global_claude.exists() {
                if let Ok(config) = self.parse_memory_file(&global_claude) {
                    if !config.instructions.is_empty() {
                        merged_instructions.push(config.instructions.clone());
                    }
                    merged_metadata = merge_metadata(merged_metadata, config.metadata.clone());
                    sources.push(MemorySource {
                        path: global_claude,
                        config,
                    });
                }
            }

            // 2. Global SHANNON.md
            let global_shannon = home.join(".shannon").join("SHANNON.md");
            if global_shannon.exists() {
                if let Ok(config) = self.parse_memory_file(&global_shannon) {
                    if !config.instructions.is_empty() {
                        merged_instructions.push(config.instructions.clone());
                    }
                    merged_metadata = merge_metadata(merged_metadata, config.metadata.clone());
                    sources.push(MemorySource {
                        path: global_shannon,
                        config,
                    });
                }
            }
        }

        // 3. Project CLAUDE.md
        let project_claude = self.base_dir.join("CLAUDE.md");
        if project_claude.exists() {
            if let Ok(config) = self.parse_memory_file(&project_claude) {
                if !config.instructions.is_empty() {
                    merged_instructions.push(config.instructions.clone());
                }
                merged_metadata = merge_metadata(merged_metadata, config.metadata.clone());
                sources.push(MemorySource {
                    path: project_claude,
                    config,
                });
            }
        }

        // 4. Project SHANNON.md
        let project_shannon = self.base_dir.join("SHANNON.md");
        if project_shannon.exists() {
            if let Ok(config) = self.parse_memory_file(&project_shannon) {
                if !config.instructions.is_empty() {
                    merged_instructions.push(config.instructions.clone());
                }
                merged_metadata = merge_metadata(merged_metadata, config.metadata.clone());
                sources.push(MemorySource {
                    path: project_shannon,
                    config,
                });
            }
        }

        Ok(MergedMemory {
            instructions: merged_instructions.join("\n\n"),
            metadata: merged_metadata,
            sources,
        })
    }
}

impl Default for ProjectMemoryManager {
    fn default() -> Self {
        Self::from_current_dir().unwrap_or_else(|_| Self {
            base_dir: PathBuf::from(".")
        })
    }
}

/// Merge two metadata structs, with `later` values overriding `base` values
/// when they are non-default/non-None.
fn merge_metadata(base: ProjectMemoryMetadata, later: ProjectMemoryMetadata) -> ProjectMemoryMetadata {
    ProjectMemoryMetadata {
        priority: later.priority,
        disable_auto_memory: later.disable_auto_memory,
        model: later.model.or(base.model),
        temperature: later.temperature.or(base.temperature),
        max_tokens: later.max_tokens.or(base.max_tokens),
        instructions_for: later.instructions_for.or(base.instructions_for),
        tool_permissions: later.tool_permissions.or(base.tool_permissions),
    }
}

/// Attempt to resolve the user's home directory.
fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_memory_file_without_frontmatter() {
        let manager = ProjectMemoryManager::new(PathBuf::from("."));
        let content = "This is a test instruction.\nAnother line.";
        let config = manager.parse_memory_file_content(content, Path::new("test.md")).unwrap();

        assert_eq!(config.metadata.priority, 0);
        assert_eq!(config.content, content);
        assert!(!config.instructions.is_empty());
    }

    #[test]
    fn test_parse_memory_file_with_frontmatter() {
        let manager = ProjectMemoryManager::new(PathBuf::from("."));
        let content = r#"---
priority: 10
model: test-model-v1
disable_auto_memory: false
---
This is a test instruction with frontmatter."#;

        let config = manager.parse_memory_file_content(content, Path::new("test.md")).unwrap();

        assert_eq!(config.metadata.priority, 10);
        assert_eq!(config.metadata.model.as_deref(), Some("test-model-v1"));
        assert!(config.content.contains("This is a test instruction"));
    }

    #[test]
    fn test_extract_instructions() {
        let manager = ProjectMemoryManager::new(PathBuf::from("."));
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
        let manager = ProjectMemoryManager::new(PathBuf::from("."));
        let config = ProjectMemoryConfig {
            metadata: ProjectMemoryMetadata {
                priority: 5,
                model: Some("test-model".to_string()),
                disable_auto_memory: true,
                ..Default::default()
            },
            content: "Test content".to_string(),
            instructions: "Test content".to_string(),
        };

        let prompt = manager.format_for_prompt(&config);
        assert!(prompt.contains("priority: 5"));
        assert!(prompt.contains("Model Override: test-model"));
        assert!(prompt.contains("Auto Memory: Disabled"));
        assert!(prompt.contains("Test content"));
    }

    #[test]
    fn test_backward_compat_aliases() {
        // Verify that the backward-compatible type aliases compile and work
        let _config: ClaudeMdConfig = ProjectMemoryConfig {
            metadata: ClaudeMdMetadata::default(),
            content: String::new(),
            instructions: String::new(),
        };
        let _metadata: ClaudeMdMetadata = ProjectMemoryMetadata::default();
        let _manager: ClaudeMdManager = ProjectMemoryManager::new(PathBuf::from("."));
        let _result: ClaudeMdSearchResult = ProjectMemorySearchResult {
            path: PathBuf::new(),
            config: ProjectMemoryConfig {
                metadata: ProjectMemoryMetadata::default(),
                content: String::new(),
                instructions: String::new(),
            },
        };
        let _error: ClaudeMdError = ProjectMemoryError::NotFound("test".to_string());
    }

    #[test]
    fn test_merge_metadata() {
        let base = ProjectMemoryMetadata {
            priority: 1,
            model: Some("base-model".to_string()),
            temperature: Some(0.5),
            disable_auto_memory: false,
            max_tokens: None,
            instructions_for: None,
            tool_permissions: None,
        };
        let later = ProjectMemoryMetadata {
            priority: 5,
            model: Some("later-model".to_string()),
            temperature: None,
            disable_auto_memory: true,
            max_tokens: Some(4096),
            instructions_for: Some(serde_json::json!({"key": "value"})),
            tool_permissions: None,
        };

        let merged = merge_metadata(base, later);
        assert_eq!(merged.priority, 5);
        assert_eq!(merged.model.as_deref(), Some("later-model"));
        assert_eq!(merged.temperature, Some(0.5)); // inherited from base
        assert!(merged.disable_auto_memory);
        assert_eq!(merged.max_tokens, Some(4096));
    }
}

// Backward compatibility aliases
pub type ClaudeMdConfig = ProjectMemoryConfig;
pub type ClaudeMdMetadata = ProjectMemoryMetadata;
pub type ClaudeMdManager = ProjectMemoryManager;
pub type ClaudeMdSearchResult = ProjectMemorySearchResult;
pub type ClaudeMdError = ProjectMemoryError;
