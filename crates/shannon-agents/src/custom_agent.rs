//! Custom agent definitions loaded from `.claude/agents/*.md`.
//!
//! This module provides a richer agent definition format compared to
//! [`crate::agent_defs::AgentDefinition`], supporting YAML frontmatter
//! with tool restrictions, directory allowlists, turn limits, and
//! system prompt suffixes.
//!
//! ## File Format
//!
//! Files follow a YAML frontmatter + markdown body format:
//!
//! ```markdown
//! ---
//! name: my-agent
//! description: A custom agent for specific tasks
//! model: sonnet
//! tools:
//!   - read
//!   - write
//!   - bash
//! allowed_directories:
//!   - src/
//!   - tests/
//! max_turns: 10
//! system_prompt_suffix: |
//!   Always use TypeScript strict mode.
//! ---
//!
//! Additional instructions that form the body of the agent's system prompt.
//! ```

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Known valid tool names for validation.
const KNOWN_TOOLS: &[&str] = &[
    "read",
    "write",
    "bash",
    "grep",
    "glob",
    "edit",
    "web_search",
    "web_fetch",
    "notebook_read",
    "notebook_edit",
    "task",
    "skill",
];

/// Known valid model aliases.
const KNOWN_MODELS: &[&str] = &["sonnet", "opus", "haiku"];

/// Errors that can occur loading custom agent definitions.
#[derive(Debug, Error)]
pub enum CustomAgentError {
    /// I/O error reading a file.
    #[error("IO error reading {0}: {1}")]
    Io(PathBuf, std::io::Error),

    /// YAML frontmatter parse error.
    #[error("YAML parse error in {0}: {1}")]
    Yaml(PathBuf, String),

    /// Validation error (missing required fields, invalid values).
    #[error("Validation error in {0}: {1}")]
    Validation(PathBuf, String),

    /// Agent not found by name.
    #[error("Agent '{0}' not found")]
    NotFound(String),
}

/// A user-defined agent loaded from `.claude/agents/*.md`.
#[derive(Debug, Clone)]
pub struct CustomAgentDef {
    /// Short name for the agent type (e.g. "my-agent").
    pub name: String,
    /// Human-readable description of what this agent does.
    pub description: String,
    /// LLM model alias to use (e.g. "sonnet", "opus", "haiku").
    pub model: Option<String>,
    /// Tools this agent is allowed to use. `None` means all tools.
    pub allowed_tools: Option<Vec<String>>,
    /// Directories this agent is allowed to access. `None` means all directories.
    pub allowed_directories: Option<Vec<String>>,
    /// Maximum conversation turns for this agent.
    pub max_turns: Option<u32>,
    /// Text appended to the standard system prompt.
    pub system_prompt_suffix: Option<String>,
    /// The markdown body after the frontmatter — the main system prompt content.
    pub body_instructions: String,
    /// The file this definition was loaded from.
    pub source_path: PathBuf,
}

/// Serde-intermediate for parsing YAML frontmatter.
#[derive(Debug, Clone, Deserialize)]
struct FrontMatter {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "tools")]
    allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    allowed_directories: Option<Vec<String>>,
    #[serde(default)]
    max_turns: Option<u32>,
    #[serde(default)]
    system_prompt_suffix: Option<String>,
}

/// Loader for custom agent definitions from disk.
#[derive(Debug, Clone)]
pub struct CustomAgentLoader {
    /// Directories to scan, in priority order (later overrides earlier).
    search_paths: Vec<PathBuf>,
}

impl CustomAgentLoader {
    /// Create a new loader with default search paths.
    ///
    /// Order (later overrides earlier):
    /// 1. `~/.claude/agents/` (user-global)
    /// 2. `.claude/agents/` (project-local)
    pub fn new() -> Self {
        let mut search_paths = Vec::new();

        // User-global
        if let Some(home) = dirs::home_dir() {
            search_paths.push(home.join(".claude").join("agents"));
        }

        // Project-local (higher priority)
        search_paths.push(PathBuf::from(".claude").join("agents"));

        Self { search_paths }
    }

    /// Create a loader with custom search paths (useful for testing).
    pub fn with_paths(search_paths: Vec<PathBuf>) -> Self {
        Self { search_paths }
    }

    /// Scan all `*.md` files in search paths and return all discovered agent definitions.
    ///
    /// When multiple files define an agent with the same name, the one from the
    /// later search path wins (project-local overrides user-global).
    pub fn discover(&self) -> Result<HashMap<String, CustomAgentDef>, CustomAgentError> {
        let mut agents: HashMap<String, CustomAgentDef> = HashMap::new();

        for dir in &self.search_paths {
            if !dir.is_dir() {
                continue;
            }

            let entries = match std::fs::read_dir(dir) {
                Ok(e) => e,
                Err(err) => {
                    tracing::debug!(
                        dir = %dir.display(),
                        error = %err,
                        "Failed to read agents directory, skipping"
                    );
                    continue;
                }
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }

                match Self::parse_file(&path) {
                    Ok(def) => {
                        tracing::info!(
                            name = %def.name,
                            path = %path.display(),
                            "Discovered custom agent definition"
                        );
                        agents.insert(def.name.clone(), def);
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %e,
                            "Failed to load custom agent definition"
                        );
                    }
                }
            }
        }

        Ok(agents)
    }

    /// Load a specific agent by name from the search paths.
    ///
    /// Searches paths in reverse priority order (project-local first), returning
    /// the first match found.
    pub fn load(&self, name: &str) -> Result<CustomAgentDef, CustomAgentError> {
        // Search in reverse to check highest-priority paths first.
        for dir in self.search_paths.iter().rev() {
            let candidate = dir.join(format!("{name}.md"));
            if candidate.is_file() {
                return Self::parse_file(&candidate);
            }
        }

        Err(CustomAgentError::NotFound(name.to_string()))
    }

    /// Parse a single `.md` file into a `CustomAgentDef`.
    fn parse_file(path: &Path) -> Result<CustomAgentDef, CustomAgentError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| CustomAgentError::Io(path.to_path_buf(), e))?;

        let (frontmatter, body) = split_frontmatter(&content);

        let fm: FrontMatter = serde_yaml::from_str(frontmatter)
            .map_err(|e| CustomAgentError::Yaml(path.to_path_buf(), e.to_string()))?;

        let def = CustomAgentDef {
            name: fm.name,
            description: fm.description,
            model: fm.model,
            allowed_tools: fm.allowed_tools,
            allowed_directories: fm.allowed_directories,
            max_turns: fm.max_turns,
            system_prompt_suffix: fm.system_prompt_suffix,
            body_instructions: body.trim().to_string(),
            source_path: path.to_path_buf(),
        };

        def.validate()?;

        Ok(def)
    }
}

impl Default for CustomAgentLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl CustomAgentDef {
    /// Validate the agent definition for correctness.
    ///
    /// Checks:
    /// - `name` is non-empty
    /// - `description` is non-empty
    /// - `model` (if set) is a known alias
    /// - `allowed_tools` (if set) contains only known tool names
    pub fn validate(&self) -> Result<(), CustomAgentError> {
        let path = self.source_path.clone();

        if self.name.is_empty() {
            return Err(CustomAgentError::Validation(
                path,
                "Agent name must not be empty".into(),
            ));
        }

        if self.description.is_empty() {
            return Err(CustomAgentError::Validation(
                path,
                "Agent description must not be empty".into(),
            ));
        }

        if let Some(ref model) = self.model {
            let lower = model.to_lowercase();
            if !KNOWN_MODELS.contains(&lower.as_str()) {
                return Err(CustomAgentError::Validation(
                    path,
                    format!(
                        "Invalid model '{}'. Must be one of: {}",
                        model,
                        KNOWN_MODELS.join(", ")
                    ),
                ));
            }
        }

        if let Some(ref tools) = self.allowed_tools {
            for tool in tools {
                if !KNOWN_TOOLS.contains(&tool.as_str()) {
                    return Err(CustomAgentError::Validation(
                        path,
                        format!(
                            "Unknown tool '{}'. Known tools: {}",
                            tool,
                            KNOWN_TOOLS.join(", ")
                        ),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Parse a single `.md` file into a `CustomAgentDef`.
    ///
    /// Convenience wrapper around [`CustomAgentLoader::parse_file`].
    pub fn from_file(path: &Path) -> Result<Self, CustomAgentError> {
        CustomAgentLoader::parse_file(path)
    }
}

/// Split markdown content into YAML frontmatter and body text.
///
/// Returns `(frontmatter_str, body_str)`.
///
/// If no frontmatter fences (`---`) are present, returns an error string
/// as the frontmatter (which will fail YAML parsing downstream).
fn split_frontmatter(content: &str) -> (&str, String) {
    let trimmed = content.trim_start();

    if !trimmed.starts_with("---") {
        // No frontmatter at all — the whole content is body.
        // Frontmatter will be empty, causing YAML parse to fail with a
        // clear "missing field `name`" error.
        return ("", content.to_string());
    }

    // Skip the opening `---` (and any trailing whitespace on that line).
    let after_open = &trimmed[3..];

    // Find the closing `---` on its own line.
    if let Some(end) = after_open.find("\n---") {
        let yaml_part = &after_open[..end];
        let rest = &after_open[end + 4..]; // skip "\n---"

        // The rest may start with newlines; trim leading whitespace.
        let body = rest.trim_start().to_string();
        return (yaml_part, body);
    }

    // No closing fence found — treat entire content as body.
    ("", content.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // ── Parsing Tests ─────────────────────────────────────────────────

    #[test]
    fn parse_valid_frontmatter_all_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("full-agent.md");

        fs::write(
            &path,
            "---\n\
             name: my-agent\n\
             description: A custom agent for specific tasks\n\
             model: sonnet\n\
             tools:\n\
               - read\n\
               - write\n\
               - bash\n\
               - grep\n\
             allowed_directories:\n\
               - src/\n\
               - tests/\n\
             max_turns: 10\n\
             system_prompt_suffix: Always use TypeScript strict mode.\n\
             ---\n\
             \n\
             Additional instructions that form the body of the agent's system prompt.\n",
        )
        .unwrap();

        let def = CustomAgentDef::from_file(&path).unwrap();
        assert_eq!(def.name, "my-agent");
        assert_eq!(def.description, "A custom agent for specific tasks");
        assert_eq!(def.model.as_deref(), Some("sonnet"));
        assert_eq!(
            def.allowed_tools.as_deref(),
            Some(
                &[
                    "read".to_string(),
                    "write".to_string(),
                    "bash".to_string(),
                    "grep".to_string(),
                ][..]
            )
        );
        assert_eq!(
            def.allowed_directories.as_deref(),
            Some(&["src/".to_string(), "tests/".to_string()][..])
        );
        assert_eq!(def.max_turns, Some(10));
        assert_eq!(
            def.system_prompt_suffix.as_deref(),
            Some("Always use TypeScript strict mode.")
        );
        assert_eq!(
            def.body_instructions,
            "Additional instructions that form the body of the agent's system prompt."
        );
    }

    #[test]
    fn parse_minimal_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("minimal.md");

        fs::write(
            &path,
            "---\nname: minimal\ndescription: Bare minimum agent\n---\nJust do stuff.\n",
        )
        .unwrap();

        let def = CustomAgentDef::from_file(&path).unwrap();
        assert_eq!(def.name, "minimal");
        assert_eq!(def.description, "Bare minimum agent");
        assert!(def.model.is_none());
        assert!(def.allowed_tools.is_none());
        assert!(def.allowed_directories.is_none());
        assert!(def.max_turns.is_none());
        assert!(def.system_prompt_suffix.is_none());
        assert_eq!(def.body_instructions, "Just do stuff.");
    }

    #[test]
    fn missing_name_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-name.md");

        fs::write(
            &path,
            "---\ndescription: No name agent\n---\nBody text.\n",
        )
        .unwrap();

        let result = CustomAgentDef::from_file(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("missing field `name`"),
            "Expected missing field error, got: {err}"
        );
    }

    #[test]
    fn missing_description_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-desc.md");

        fs::write(
            &path,
            "---\nname: no-desc\n---\nBody text.\n",
        )
        .unwrap();

        let result = CustomAgentDef::from_file(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("description must not be empty"),
            "Expected empty description error, got: {err}"
        );
    }

    #[test]
    fn invalid_model_rejected_by_validate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad-model.md");

        fs::write(
            &path,
            "---\nname: bad-model\ndescription: Bad model\nmodel: gpt-99\n---\nBody.\n",
        )
        .unwrap();

        let result = CustomAgentDef::from_file(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Invalid model 'gpt-99'"),
            "Expected invalid model error, got: {err}"
        );
    }

    #[test]
    fn invalid_tool_rejected_by_validate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad-tool.md");

        fs::write(
            &path,
            "---\nname: bad-tool\ndescription: Bad tool\ntools:\n- read\n- teleport\n---\nBody.\n",
        )
        .unwrap();

        let result = CustomAgentDef::from_file(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Unknown tool 'teleport'"),
            "Expected unknown tool error, got: {err}"
        );
    }

    #[test]
    fn body_instructions_extracted_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("body-test.md");

        fs::write(
            &path,
            "---\nname: body-test\ndescription: Body extraction test\n---\n\n\nLine one.\nLine two.\n\nLine four.\n",
        )
        .unwrap();

        let def = CustomAgentDef::from_file(&path).unwrap();
        assert_eq!(def.body_instructions, "Line one.\nLine two.\n\nLine four.");
    }

    // ── Discovery Tests ───────────────────────────────────────────────

    #[test]
    fn directory_scan_discovers_multiple_agents() {
        let dir = tempfile::tempdir().unwrap();

        fs::write(
            dir.path().join("agent-a.md"),
            "---\nname: agent-a\ndescription: Agent A\n---\nDo A things.\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("agent-b.md"),
            "---\nname: agent-b\ndescription: Agent B\nmodel: opus\n---\nDo B things.\n",
        )
        .unwrap();
        // Non-markdown file should be ignored
        fs::write(dir.path().join("notes.txt"), "not an agent\n").unwrap();

        let loader = CustomAgentLoader::with_paths(vec![dir.path().to_path_buf()]);
        let agents = loader.discover().unwrap();

        assert_eq!(agents.len(), 2);
        assert!(agents.contains_key("agent-a"));
        assert!(agents.contains_key("agent-b"));
        assert_eq!(agents["agent-b"].model.as_deref(), Some("opus"));
    }

    #[test]
    fn project_overrides_home() {
        let home_dir = tempfile::tempdir().unwrap();
        let project_dir = tempfile::tempdir().unwrap();

        // "Home" version — lower priority (added first)
        fs::write(
            home_dir.path().join("shared-agent.md"),
            "---\nname: shared-agent\ndescription: Home version\nmodel: haiku\n---\nHome body.\n",
        )
        .unwrap();

        // "Project" version — higher priority (added second, overrides)
        fs::write(
            project_dir.path().join("shared-agent.md"),
            "---\nname: shared-agent\ndescription: Project version\nmodel: opus\n---\nProject body.\n",
        )
        .unwrap();

        let loader = CustomAgentLoader::with_paths(vec![
            home_dir.path().to_path_buf(),
            project_dir.path().to_path_buf(),
        ]);
        let agents = loader.discover().unwrap();

        assert_eq!(agents.len(), 1);
        let agent = &agents["shared-agent"];
        assert_eq!(agent.description, "Project version");
        assert_eq!(agent.model.as_deref(), Some("opus"));
        assert_eq!(agent.body_instructions, "Project body.");
    }

    #[test]
    fn load_specific_agent_by_name() {
        let dir = tempfile::tempdir().unwrap();

        fs::write(
            dir.path().join("target.md"),
            "---\nname: target\ndescription: The target agent\n---\nTarget body.\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("other.md"),
            "---\nname: other\ndescription: Another agent\n---\nOther body.\n",
        )
        .unwrap();

        let loader = CustomAgentLoader::with_paths(vec![dir.path().to_path_buf()]);

        let agent = loader.load("target").unwrap();
        assert_eq!(agent.name, "target");
        assert_eq!(agent.body_instructions, "Target body.");

        let missing = loader.load("nonexistent");
        assert!(missing.is_err());
        assert!(missing.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn load_prefers_higher_priority_path() {
        let low_dir = tempfile::tempdir().unwrap();
        let high_dir = tempfile::tempdir().unwrap();

        fs::write(
            low_dir.path().join("dup.md"),
            "---\nname: dup\ndescription: Low priority\n---\nLow.\n",
        )
        .unwrap();
        fs::write(
            high_dir.path().join("dup.md"),
            "---\nname: dup\ndescription: High priority\n---\nHigh.\n",
        )
        .unwrap();

        let loader = CustomAgentLoader::with_paths(vec![
            low_dir.path().to_path_buf(),
            high_dir.path().to_path_buf(),
        ]);

        // load() checks paths in reverse (highest priority first)
        let agent = loader.load("dup").unwrap();
        assert_eq!(agent.description, "High priority");
    }

    // ── UTF-8 Content Tests ───────────────────────────────────────────

    #[test]
    fn utf8_content_in_agent_definition() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unicode-agent.md");

        fs::write(
            &path,
            "---\nname: unicode-agent\ndescription: Één spécial àgent with 日本語 and emoji 🤖\n---\n\
             You handle files in directories likesrc/ünicöde/ and tests/数据/.\n\
             Always respond in 中文 when discussing code.\n",
        )
        .unwrap();

        let def = CustomAgentDef::from_file(&path).unwrap();
        assert_eq!(def.name, "unicode-agent");
        assert!(def.description.contains("日本語"));
        assert!(def.description.contains("🤖"));
        assert!(def.body_instructions.contains("ünicöde"));
        assert!(def.body_instructions.contains("中文"));
    }

    #[test]
    fn utf8_in_allowed_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("i18n.md");

        fs::write(
            &path,
            "---\n\
             name: i18n\n\
             description: International agent\n\
             allowed_directories:\n\
             - src/日本語/\n\
             - tests/über/\n\
             ---\nBody.\n",
        )
        .unwrap();

        let def = CustomAgentDef::from_file(&path).unwrap();
        let dirs = def.allowed_directories.unwrap();
        assert!(dirs.contains(&"src/日本語/".to_string()));
        assert!(dirs.contains(&"tests/über/".to_string()));
    }

    // ── Edge Case Tests ───────────────────────────────────────────────

    #[test]
    fn no_frontmatter_returns_yaml_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-fm.md");
        fs::write(&path, "Just plain markdown content.\n").unwrap();

        let result = CustomAgentDef::from_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn empty_file_returns_yaml_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.md");
        fs::write(&path, "").unwrap();

        let result = CustomAgentDef::from_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn frontmatter_only_no_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fm-only.md");

        fs::write(
            &path,
            "---\nname: fm-only\ndescription: No body\n---\n",
        )
        .unwrap();

        let def = CustomAgentDef::from_file(&path).unwrap();
        assert_eq!(def.name, "fm-only");
        assert_eq!(def.body_instructions, "");
    }

    #[test]
    fn valid_model_case_insensitive() {
        // "Sonnet" with capital S should still be valid
        let def = CustomAgentDef {
            name: "case-test".to_string(),
            description: "Case test".to_string(),
            model: Some("Sonnet".to_string()),
            allowed_tools: None,
            allowed_directories: None,
            max_turns: None,
            system_prompt_suffix: None,
            body_instructions: String::new(),
            source_path: PathBuf::from("case-test.md"),
        };

        assert!(def.validate().is_ok());
    }

    #[test]
    fn model_opus_and_haiku_valid() {
        for m in &["opus", "haiku", "sonnet", "Opus", "HAIKU"] {
            let def = CustomAgentDef {
                name: "test".to_string(),
                description: "test".to_string(),
                model: Some(m.to_string()),
                allowed_tools: None,
                allowed_directories: None,
                max_turns: None,
                system_prompt_suffix: None,
                body_instructions: String::new(),
                source_path: PathBuf::from("test.md"),
            };
            assert!(def.validate().is_ok(), "Model '{m}' should be valid");
        }
    }

    #[test]
    fn discover_from_nonexistent_dir_returns_empty() {
        let loader =
            CustomAgentLoader::with_paths(vec![PathBuf::from("/nonexistent/path/agents")]);
        let agents = loader.discover().unwrap();
        assert!(agents.is_empty());
    }

    #[test]
    fn system_prompt_suffix_multiline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("suffix.md");

        let content = "\
---
name: suffix-test
description: Suffix test
system_prompt_suffix: |
  Rule one: always format.
  Rule two: never abbreviate.
---
Main body instructions.
";
        fs::write(&path, content).unwrap();

        let def = CustomAgentDef::from_file(&path).unwrap();
        let suffix = def.system_prompt_suffix.unwrap();
        assert!(suffix.contains("always format"), "suffix was: {suffix:?}");
        assert!(suffix.contains("never abbreviate"), "suffix was: {suffix:?}");
    }

    #[test]
    fn all_known_tools_are_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("all-tools.md");

        let tools_list: String = KNOWN_TOOLS.iter().map(|t| format!("- {t}")).collect::<Vec<_>>().join("\n");

        fs::write(
            &path,
            format!(
                "---\n\
                 name: all-tools\n\
                 description: All tools agent\n\
                 tools:\n\
                 {tools_list}\n\
                 ---\nBody.\n"
            ),
        )
        .unwrap();

        let def = CustomAgentDef::from_file(&path).unwrap();
        assert!(def.validate().is_ok());
        let tools = def.allowed_tools.unwrap();
        assert_eq!(tools.len(), KNOWN_TOOLS.len());
    }
}
