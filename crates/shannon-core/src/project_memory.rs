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
#[derive(Default)]
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
                .map_err(|e| ProjectMemoryError::InvalidFrontmatter(format!("YAML parsing error: {e}")))?;

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
            parts.push(format!("Model Override: {model}"));
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

    /// Load and merge memory from multiple sources following Claude Code's hierarchy.
    ///
    /// Searches for memory files in the following order (later entries override earlier ones):
    /// 1. Global `~/.claude/CLAUDE.md` (Claude Code compatible)
    /// 2. Global `~/.shannon/CLAUDE.md`
    /// 3. Global `~/.shannon/SHANNON.md`
    /// 4. Project `.claude/CLAUDE.md` (Claude Code compatible)
    /// 5. Project `CLAUDE.md` (at base_dir)
    /// 6. Project `CLAUDE.local.md` (gitignored, personal instructions)
    /// 7. Project `SHANNON.md`
    /// 8. Parent directory `CLAUDE.md`/`SHANNON.md` files (walking up to root)
    pub fn load_merged(&self) -> Result<MergedMemory, ProjectMemoryError> {
        let mut sources: Vec<MemorySource> = Vec::new();
        let mut merged_instructions: Vec<String> = Vec::new();
        let mut merged_metadata = ProjectMemoryMetadata::default();

        let mut try_add = |path: PathBuf| {
            if let Some(source) = try_load_source(&path) {
                if !source.config.instructions.is_empty() {
                    merged_instructions.push(source.config.instructions.clone());
                }
                merged_metadata = merge_metadata(merged_metadata.clone(), source.config.metadata.clone());
                sources.push(source);
            }
        };

        // Global paths (both .claude/ and .shannon/)
        if let Some(home) = dirs_home() {
            try_add(home.join(".claude").join("CLAUDE.md"));
            try_add(home.join(".shannon").join("CLAUDE.md"));
            try_add(home.join(".shannon").join("SHANNON.md"));
        }

        // Project paths
        try_add(self.base_dir.join(".claude").join("CLAUDE.md"));
        try_add(self.base_dir.join("CLAUDE.md"));
        try_add(self.base_dir.join("CLAUDE.local.md"));
        try_add(self.base_dir.join("SHANNON.md"));

        // Parent directories (walking up from base_dir, root-first order)
        let mut parents: Vec<PathBuf> = Vec::new();
        let mut cur = self.base_dir.parent().map(|p| p.to_path_buf());
        while let Some(p) = cur.take() {
            parents.push(p.clone());
            cur = p.parent().map(|q| q.to_path_buf());
        }
        parents.reverse(); // root-first so closer dirs come last (higher priority)
        for p in parents {
            try_add(p.join("CLAUDE.md"));
            try_add(p.join("SHANNON.md"));
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

/// Try to load and parse a memory file at the given path.
/// Returns None if the file doesn't exist or can't be parsed.
fn try_load_source(path: &Path) -> Option<MemorySource> {
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    let parent = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let manager = ProjectMemoryManager::new(parent);
    let config = manager.parse_memory_file_content(&content, path).ok()?;
    Some(MemorySource {
        path: path.to_path_buf(),
        config,
    })
}

/// Load MEMORY.md index file (first 200 lines).
/// Searches project root, .claude/, and .shannon/ directories.
/// Returns None if no MEMORY.md is found.
pub fn load_memory_index(dir: &Path) -> Option<String> {
    let paths = [
        dir.join("MEMORY.md"),
        dir.join(".claude").join("MEMORY.md"),
        dir.join(".shannon").join("MEMORY.md"),
    ];

    for path in &paths {
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                let lines: Vec<&str> = content.lines().take(200).collect();
                let truncated = lines.join("\n");
                if !truncated.trim().is_empty() {
                    return Some(format!("=== Memory Index (MEMORY.md) ===\n\n{truncated}"));
                }
            }
        }
    }
    None
}

/// Load all `.md` files from `.claude/rules/` directory.
///
/// Returns concatenated content of all rule files, sorted by filename for
/// deterministic ordering. Returns `None` if the directory doesn't exist
/// or contains no `.md` files.
pub fn load_rules(dir: &Path) -> Option<String> {
    let rules_dir = dir.join(".claude").join("rules");
    if !rules_dir.is_dir() {
        return None;
    }

    let mut entries: Vec<_> = std::fs::read_dir(&rules_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "md")
        })
        .collect();

    entries.sort_by_key(|e| e.file_name());

    let mut parts: Vec<String> = Vec::new();
    for entry in entries {
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            if !content.trim().is_empty() {
                if let Some(name) = entry.file_name().to_str() {
                    parts.push(format!("### Rule: {name}\n\n{content}"));
                }
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(format!("=== Project Rules (.claude/rules/) ===\n\n{}", parts.join("\n\n")))
    }
}

/// Resolve `@import` directives in content.
///
/// Supports patterns like `@README`, `@docs/guide.md`, `@CONTRIBUTING`.
/// Lines starting with `@` followed by a path-like string (no spaces, no `@`,
/// no `:`, not starting with `/`) are treated as imports.
///
/// If the imported file can't be found, the original line is kept as-is.
pub fn resolve_imports(content: &str, base_dir: &Path) -> String {
    let mut result = String::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(import_path) = trimmed.strip_prefix('@') {
            // Only treat as import if it looks like a file path
            if !import_path.contains(' ')
                && !import_path.contains('@')
                && !import_path.contains(':')
                && !import_path.starts_with('/')
            {
                let candidates = if import_path.contains('.') {
                    vec![import_path.to_string()]
                } else {
                    vec![
                        import_path.to_string(),
                        format!("{import_path}.md"),
                        format!("{import_path}.txt"),
                    ]
                };

                let mut resolved = false;
                for candidate in &candidates {
                    let full_path = base_dir.join(candidate);
                    if full_path.exists() {
                        if let Ok(imported) = std::fs::read_to_string(&full_path) {
                            result.push_str(&imported);
                            result.push('\n');
                            resolved = true;
                            break;
                        }
                    }
                }

                if !resolved {
                    result.push_str(line);
                    result.push('\n');
                }
                continue;
            }
        }
        result.push_str(line);
        result.push('\n');
    }

    result
}

/// Save a memory entry as a file in the project memory directory.
///
/// Writes to `~/.shannon/projects/<project_hash>/memory/<id>.md` for
/// Claude Code-compatible file-based auto-memory.
///
/// Returns the path where the file was saved, or an error.
pub fn save_memory_file(project_dir: &Path, id: &str, content: &str) -> std::io::Result<PathBuf> {
    let memory_dir = project_memory_dir(project_dir);
    std::fs::create_dir_all(&memory_dir)?;

    // Use first 8 chars of id as filename, sanitized
    let safe_id: String = id.chars()
        .take(8)
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let filename = format!("{safe_id}.md");
    let path = memory_dir.join(&filename);

    // Write with YAML frontmatter
    let file_content = format!(
        "---\nid: {id}\ndate: {}\n---\n\n{content}",
        chrono::Utc::now().to_rfc3339()
    );
    std::fs::write(&path, file_content)?;

    Ok(path)
}

/// Get the project memory directory path.
///
/// Returns `~/.shannon/projects/<project_hash>/memory/`.
/// The project hash is derived from the project directory path.
pub fn project_memory_dir(project_dir: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;

    let mut hasher = DefaultHasher::new();
    project_dir.hash(&mut hasher);
    let hash = format!("{:016x}", hasher.finish());

    dirs_home()
        .map(|h| h.join(".shannon").join("projects").join(&hash).join("memory"))
        .unwrap_or_else(|| PathBuf::from(".shannon/projects").join(&hash).join("memory"))
}

/// Attempt to resolve the user's home directory.
fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

// Backward compatibility aliases
pub type ClaudeMdConfig = ProjectMemoryConfig;
pub type ClaudeMdMetadata = ProjectMemoryMetadata;
pub type ClaudeMdManager = ProjectMemoryManager;
pub type ClaudeMdSearchResult = ProjectMemorySearchResult;
pub type ClaudeMdError = ProjectMemoryError;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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

    #[test]
    fn test_load_merged_finds_claude_paths() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        let claude_dir = tmp.join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(tmp.join("CLAUDE.md"), "Root CLAUDE.md instructions").unwrap();
        fs::write(claude_dir.join("CLAUDE.md"), "Hidden claude dir instructions").unwrap();
        fs::write(tmp.join("CLAUDE.local.md"), "Local gitignored instructions").unwrap();
        fs::write(tmp.join("SHANNON.md"), "Shannon project instructions").unwrap();

        let manager = ProjectMemoryManager::new(tmp.clone());
        let result = manager.load_merged().unwrap();

        assert!(!result.sources.is_empty(), "Should find at least some sources");
        assert!(result.instructions.contains("Root CLAUDE.md"), "Should contain root CLAUDE.md");
        assert!(result.instructions.contains("Hidden claude dir"), "Should contain .claude/CLAUDE.md");
        assert!(result.instructions.contains("Local gitignored"), "Should contain CLAUDE.local.md");
        assert!(result.instructions.contains("Shannon project"), "Should contain SHANNON.md");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_memory_index() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();

        // No MEMORY.md → returns None
        assert!(load_memory_index(&tmp).is_none());

        // With MEMORY.md
        let content: Vec<String> = (0..300).map(|i| format!("Line {i}")).collect();
        fs::write(tmp.join("MEMORY.md"), content.join("\n")).unwrap();

        let result = load_memory_index(&tmp);
        assert!(result.is_some(), "Should find MEMORY.md");
        let text = result.unwrap();
        assert!(text.contains("=== Memory Index"), "Should have header");
        // Should be truncated to ~200 lines
        assert!(!text.contains("Line 250"), "Should not contain line 250+");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_imports() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(tmp.join("docs")).unwrap();
        fs::write(tmp.join("README.md"), "# Readme content").unwrap();
        fs::write(tmp.join("docs").join("guide.md"), "# Guide content").unwrap();

        // Test @import resolution
        let content = "Header line\n@README\nMiddle line\n@docs/guide.md\nFooter";
        let result = resolve_imports(content, &tmp);

        assert!(result.contains("Header line"), "Should keep non-import lines");
        assert!(result.contains("Readme content"), "Should resolve @README");
        assert!(result.contains("Middle line"), "Should keep middle lines");
        assert!(result.contains("Guide content"), "Should resolve @docs/guide.md");
        assert!(result.contains("Footer"), "Should keep footer");
        assert!(!result.contains("@README"), "Should not contain @README after resolution");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_imports_unresolved() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();

        // @nonexistent should be kept as-is
        let content = "Line one\n@nonexistent_file_xyz\nLine two";
        let result = resolve_imports(content, &tmp);
        assert!(result.contains("@nonexistent_file_xyz"), "Unresolved imports kept as-is");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_imports_skips_non_paths() {
        let content = "Email: user@example.com\nMention: @someone\nPath: /absolute/path";
        let result = resolve_imports(content, Path::new("."));
        assert!(result.contains("user@example.com"), "Should not resolve emails");
        assert!(result.contains("@someone"), "Should not resolve @mentions");
    }

    #[test]
    fn test_try_load_source() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();

        // Nonexistent file
        assert!(try_load_source(&tmp.join("nonexistent.md")).is_none());

        // Valid file
        fs::write(tmp.join("test.md"), "Test content").unwrap();
        let result = try_load_source(&tmp.join("test.md"));
        assert!(result.is_some(), "Should load valid file");
        let source = result.unwrap();
        assert!(source.config.content.contains("Test content"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
