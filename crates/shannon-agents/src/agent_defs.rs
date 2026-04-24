//! Custom agent definitions loaded from `.shannon/agents/*.toml` and `.claude/agents/*.md`.
//!
//! Agent definitions allow users to pre-configure agent types with custom
//! system prompts, capabilities, models, and tool access — similar to
//! Claude Code's `.claude/agents/*.md` pattern.
//!
//! ## File Formats
//!
//! ### TOML (`.shannon/agents/*.toml`) — full configuration
//!
//! ```toml
//! name = "backend-dev"
//! description = "Backend development specialist"
//! system_prompt = """You are a backend developer agent..."""
//! model = "claude-sonnet"
//! capabilities = ["rust", "api-design", "database"]
//! allowed_tools = ["bash", "read", "write", "grep"]
//! max_concurrent_tasks = 3
//! plan_mode_required = false
//! ```
//!
//! ### Markdown (`.claude/agents/*.md`) — Claude Code compatible
//!
//! The filename (sans `.md`) becomes the agent name, and the file content
//! becomes the system prompt. Optional YAML front matter is supported:
//!
//! ```markdown
//! ---
//! model: claude-opus
//! temperature: 0.3
//! ---
//! You are a code reviewer agent. Focus on security and performance.
//! ```
//!
//! Files are loaded from (later entries override earlier):
//! 1. `~/.shannon/agents/` (user-global, TOML)
//! 2. `~/.claude/agents/` (user-global, Markdown — Claude Code compatible)
//! 3. `.shannon/agents/` (project-local, TOML)
//! 4. `.claude/agents/` (project-local, Markdown — Claude Code compatible)

use crate::teammate::TeammateConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A custom agent definition loaded from a TOML file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Short name for the agent type (e.g. "backend-dev", "reviewer")
    pub name: String,
    /// Human-readable description of what this agent does
    #[serde(default)]
    pub description: String,
    /// System prompt injected when spawning agents of this type
    pub system_prompt: Option<String>,
    /// LLM model to use (e.g. "claude-sonnet", "gpt-4")
    #[serde(default)]
    pub model: Option<String>,
    /// Capabilities this agent possesses
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Tools this agent is allowed to use (empty = all tools)
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Maximum concurrent tasks for this agent type
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_tasks: usize,
    /// Whether this agent requires plan approval before execution
    #[serde(default)]
    pub plan_mode_required: bool,
    /// Temperature for AI responses (0.0 - 1.0)
    #[serde(default)]
    pub temperature: Option<f32>,
}

fn default_max_concurrent() -> usize {
    3
}

/// Parsed front matter from a markdown agent file.
struct FrontMatter {
    model: Option<String>,
    temperature: Option<f32>,
    description: Option<String>,
    capabilities: Option<Vec<String>>,
}

/// Parse optional YAML front matter from markdown content.
///
/// Front matter is enclosed between `---` fences at the start of the file.
/// Only simple key-value pairs are parsed; no nested structures.
fn parse_front_matter(content: &str) -> (FrontMatter, String) {
    let trimmed = content.trim_start();

    if !trimmed.starts_with("---") {
        return (FrontMatter {
            model: None,
            temperature: None,
            description: None,
            capabilities: None,
        }, content.to_string());
    }

    // Find the closing ---
    let rest = &trimmed[3..];
    if let Some(end) = rest.find("---") {
        let yaml = &rest[..end];
        let body = rest[end + 3..].trim_start().to_string();

        let mut fm = FrontMatter {
            model: None,
            temperature: None,
            description: None,
            capabilities: None,
        };

        for line in yaml.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim().trim_matches('"').trim_matches('\'');
                match key {
                    "model" => fm.model = Some(value.to_string()),
                    "temperature" => fm.temperature = value.parse().ok(),
                    "description" => fm.description = Some(value.to_string()),
                    "capabilities" => {
                        // Parse comma-separated or bracket-enclosed list
                        let cleaned = value.trim_start_matches('[').trim_end_matches(']');
                        let caps: Vec<String> = cleaned
                            .split(',')
                            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        if !caps.is_empty() {
                            fm.capabilities = Some(caps);
                        }
                    }
                    _ => {} // ignore unknown keys
                }
            }
        }

        return (fm, body);
    }

    // No closing fence found — treat entire content as body
    (FrontMatter {
        model: None,
        temperature: None,
        description: None,
        capabilities: None,
    }, content.to_string())
}

impl AgentDefinition {
    /// Convert this definition into a TeammateConfig for spawning.
    pub fn to_teammate_config(&self) -> TeammateConfig {
        TeammateConfig {
            agent_type: self.name.clone(),
            capabilities: self.capabilities.clone(),
            max_concurrent_tasks: self.max_concurrent_tasks,
            plan_mode_required: self.plan_mode_required,
            model: self.model.clone(),
            system_prompt: self.system_prompt.clone(),
            temperature: self.temperature,
            is_lead: false,
            allowed_tools: self.allowed_tools.clone(),
            permission_mode: None,
        }
    }

    /// Load a single agent definition from a TOML file.
    pub fn from_file(path: &Path) -> Result<Self, AgentDefError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| AgentDefError::Io(path.to_path_buf(), e))?;

        let def: Self = toml::from_str(&content)
            .map_err(|e| AgentDefError::Parse(path.to_path_buf(), e.to_string()))?;

        if def.name.is_empty() {
            return Err(AgentDefError::Validation(
                path.to_path_buf(),
                "Agent name must not be empty".into(),
            ));
        }

        Ok(def)
    }

    /// Load an agent definition from a Markdown file (Claude Code compatible).
    ///
    /// The filename (without `.md`) becomes the agent name. The file content
    /// becomes the system prompt. Optional YAML front matter between `---` fences
    /// is parsed for `model`, `temperature`, and `description`.
    ///
    /// Example:
    /// ```markdown
    /// ---
    /// model: claude-opus
    /// temperature: 0.3
    /// ---
    /// You are a code reviewer agent...
    /// ```
    pub fn from_markdown_file(path: &Path) -> Result<Self, AgentDefError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| AgentDefError::Io(path.to_path_buf(), e))?;

        // Derive name from filename (e.g. "backend-dev.md" → "backend-dev")
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        if name.is_empty() {
            return Err(AgentDefError::Validation(
                path.to_path_buf(),
                "Agent name derived from filename is empty".into(),
            ));
        }

        // Parse optional YAML front matter
        let (front_matter, system_prompt) = parse_front_matter(&content);

        let mut def = Self {
            name,
            description: front_matter.description.unwrap_or_default(),
            system_prompt: if system_prompt.trim().is_empty() {
                None
            } else {
                Some(system_prompt.trim().to_string())
            },
            model: front_matter.model,
            capabilities: Vec::new(),
            allowed_tools: Vec::new(),
            max_concurrent_tasks: 3,
            plan_mode_required: false,
            temperature: front_matter.temperature,
        };

        // Parse capabilities from front matter if present
        if let Some(caps) = front_matter.capabilities {
            def.capabilities = caps;
        }

        Ok(def)
    }
}

/// Errors that can occur loading agent definitions.
#[derive(Debug, thiserror::Error)]
pub enum AgentDefError {
    #[error("IO error reading {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("Parse error in {0}: {1}")]
    Parse(PathBuf, String),
    #[error("Validation error in {0}: {1}")]
    Validation(PathBuf, String),
}

/// Registry of loaded agent definitions, keyed by agent name.
#[derive(Debug, Clone, Default)]
pub struct AgentDefinitionRegistry {
    definitions: HashMap<String, AgentDefinition>,
}

impl AgentDefinitionRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load agent definitions from both project-local and user-global directories.
    ///
    /// Loading order (later overrides earlier):
    /// 1. `~/.shannon/agents/` (user-global, TOML)
    /// 2. `~/.claude/agents/` (user-global, Markdown — Claude Code compatible)
    /// 3. `.shannon/agents/` (project-local, TOML)
    /// 4. `.claude/agents/` (project-local, Markdown — Claude Code compatible)
    pub fn load_from_dirs() -> Self {
        let mut registry = Self::new();

        // 1. Load user-global TOML definitions (lowest priority)
        if let Some(home) = dirs::home_dir() {
            let global_dir = home.join(".shannon").join("agents");
            if global_dir.is_dir() {
                registry.load_from_dir(&global_dir);
            }

            // 2. Load user-global Markdown definitions (Claude Code compatible)
            let claude_global_dir = home.join(".claude").join("agents");
            if claude_global_dir.is_dir() {
                registry.load_markdown_from_dir(&claude_global_dir);
            }
        }

        // 3. Load project-local TOML definitions (higher priority)
        let local_dir = PathBuf::from(".shannon").join("agents");
        if local_dir.is_dir() {
            registry.load_from_dir(&local_dir);
        }

        // 4. Load project-local Markdown definitions (highest priority)
        let claude_local_dir = PathBuf::from(".claude").join("agents");
        if claude_local_dir.is_dir() {
            registry.load_markdown_from_dir(&claude_local_dir);
        }

        registry
    }

    /// Load all `.toml` files from a directory.
    pub fn load_from_dir(&mut self, dir: &Path) {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!(dir = %dir.display(), error = %e, "Failed to read agent definitions directory");
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                match AgentDefinition::from_file(&path) {
                    Ok(def) => {
                        tracing::info!(
                            name = %def.name,
                            path = %path.display(),
                            "Loaded agent definition"
                        );
                        self.definitions.insert(def.name.clone(), def);
                    }
                    Err(e) => {
                        tracing::warn!(path = %path.display(), error = %e, "Failed to load agent definition");
                    }
                }
            }
        }
    }

    /// Load all `.md` files from a directory as Claude Code compatible agent definitions.
    ///
    /// The filename (without `.md`) becomes the agent name, and the file content
    /// becomes the system prompt. Optional YAML front matter is parsed for
    /// `model`, `temperature`, and `description` fields.
    pub fn load_markdown_from_dir(&mut self, dir: &Path) {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::debug!(dir = %dir.display(), error = %e, "Failed to read markdown agent definitions directory");
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                match AgentDefinition::from_markdown_file(&path) {
                    Ok(def) => {
                        tracing::info!(
                            name = %def.name,
                            path = %path.display(),
                            "Loaded markdown agent definition"
                        );
                        self.definitions.insert(def.name.clone(), def);
                    }
                    Err(e) => {
                        tracing::warn!(path = %path.display(), error = %e, "Failed to load markdown agent definition");
                    }
                }
            }
        }
    }

    /// Get an agent definition by name.
    pub fn get(&self, name: &str) -> Option<&AgentDefinition> {
        self.definitions.get(name)
    }

    /// List all registered agent definition names.
    pub fn list_names(&self) -> Vec<String> {
        self.definitions.keys().cloned().collect()
    }

    /// Get all registered definitions.
    pub fn all(&self) -> &HashMap<String, AgentDefinition> {
        &self.definitions
    }

    /// Check if any definitions are loaded.
    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }

    /// Get a summary string of all loaded definitions.
    pub fn summary(&self) -> String {
        if self.definitions.is_empty() {
            return "No custom agent definitions loaded.".to_string();
        }

        let mut lines = Vec::new();
        lines.push(format!("Loaded {} agent definition(s):", self.definitions.len()));
        for (name, def) in &self.definitions {
            let caps = if def.capabilities.is_empty() {
                String::new()
            } else {
                format!(" [{}]", def.capabilities.join(", "))
            };
            lines.push(format!("  - {}{}: {}", name, caps, def.description));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn parse_minimal_definition() {
        let toml = r#"
name = "test-agent"
"#;
        let def: AgentDefinition = toml::from_str(toml).unwrap();
        assert_eq!(def.name, "test-agent");
        assert!(def.system_prompt.is_none());
        assert!(def.model.is_none());
        assert!(def.capabilities.is_empty());
        assert_eq!(def.max_concurrent_tasks, 3);
        assert!(!def.plan_mode_required);
    }

    #[test]
    fn parse_full_definition() {
        let toml = r#"
name = "backend-dev"
description = "Backend development specialist"
system_prompt = "You are a Rust backend developer."
model = "claude-sonnet"
capabilities = ["rust", "api-design", "database"]
allowed_tools = ["bash", "read", "write"]
max_concurrent_tasks = 5
plan_mode_required = true
temperature = 0.7
"#;
        let def: AgentDefinition = toml::from_str(toml).unwrap();
        assert_eq!(def.name, "backend-dev");
        assert_eq!(def.description, "Backend development specialist");
        assert_eq!(def.system_prompt.as_deref(), Some("You are a Rust backend developer."));
        assert_eq!(def.model.as_deref(), Some("claude-sonnet"));
        assert_eq!(def.capabilities, vec!["rust", "api-design", "database"]);
        assert_eq!(def.allowed_tools, vec!["bash", "read", "write"]);
        assert_eq!(def.max_concurrent_tasks, 5);
        assert!(def.plan_mode_required);
        assert_eq!(def.temperature, Some(0.7));
    }

    #[test]
    fn to_teammate_config() {
        let def = AgentDefinition {
            name: "reviewer".to_string(),
            description: "Code reviewer".to_string(),
            system_prompt: Some("Review code carefully.".to_string()),
            model: Some("claude-opus".to_string()),
            capabilities: vec!["code-review".to_string()],
            allowed_tools: vec!["read".to_string(), "grep".to_string()],
            max_concurrent_tasks: 2,
            plan_mode_required: false,
            temperature: Some(0.3),
        };

        let config = def.to_teammate_config();
        assert_eq!(config.agent_type, "reviewer");
        assert_eq!(config.system_prompt.as_deref(), Some("Review code carefully."));
        assert_eq!(config.model.as_deref(), Some("claude-opus"));
        assert_eq!(config.capabilities, vec!["code-review"]);
        assert_eq!(config.max_concurrent_tasks, 2);
        assert_eq!(config.temperature, Some(0.3));
    }

    #[test]
    fn load_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test-agent.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        write!(f, r#"
name = "my-agent"
description = "Test agent"
system_prompt = "Hello"
capabilities = ["test"]
"#).unwrap();

        let def = AgentDefinition::from_file(&file_path).unwrap();
        assert_eq!(def.name, "my-agent");
        assert_eq!(def.capabilities, vec!["test"]);
    }

    #[test]
    fn reject_empty_name() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("bad.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        write!(f, "name = \"\"\n").unwrap();

        let result = AgentDefinition::from_file(&file_path);
        assert!(result.is_err());
    }

    #[test]
    fn registry_load_and_get() {
        let dir = tempfile::tempdir().unwrap();

        let f1 = dir.path().join("alpha.toml");
        std::fs::write(&f1, "name = \"alpha\"\ndescription = \"Agent A\"\n").unwrap();

        let f2 = dir.path().join("beta.toml");
        std::fs::write(&f2, "name = \"beta\"\ndescription = \"Agent B\"\n").unwrap();

        let mut registry = AgentDefinitionRegistry::new();
        registry.load_from_dir(dir.path());

        assert!(registry.get("alpha").is_some());
        assert!(registry.get("beta").is_some());
        assert!(registry.get("gamma").is_none());
        assert_eq!(registry.list_names().len(), 2);
        assert!(!registry.is_empty());
    }

    #[test]
    fn local_overrides_global() {
        let global = tempfile::tempdir().unwrap();
        let local = tempfile::tempdir().unwrap();

        // Global version
        std::fs::write(
            global.path().join("dev.toml"),
            "name = \"dev\"\ndescription = \"Global dev\"\nmodel = \"claude-haiku\"\n",
        ).unwrap();

        // Local override
        std::fs::write(
            local.path().join("dev.toml"),
            "name = \"dev\"\ndescription = \"Local dev\"\nmodel = \"claude-opus\"\n",
        ).unwrap();

        let mut registry = AgentDefinitionRegistry::new();
        registry.load_from_dir(global.path());
        registry.load_from_dir(local.path());

        let def = registry.get("dev").unwrap();
        assert_eq!(def.description, "Local dev");
        assert_eq!(def.model.as_deref(), Some("claude-opus"));
    }

    #[test]
    fn summary_format() {
        let mut registry = AgentDefinitionRegistry::new();
        let empty_summary = registry.summary();
        assert!(empty_summary.contains("No custom agent"));

        registry.definitions.insert("dev".to_string(), AgentDefinition {
            name: "dev".to_string(),
            description: "Developer".to_string(),
            system_prompt: None,
            model: None,
            capabilities: vec!["rust".to_string()],
            allowed_tools: vec![],
            max_concurrent_tasks: 3,
            plan_mode_required: false,
            temperature: None,
        });

        let summary = registry.summary();
        assert!(summary.contains("1 agent definition"));
        assert!(summary.contains("dev"));
        assert!(summary.contains("rust"));
    }

    // -- Markdown agent definition tests --

    #[test]
    fn parse_markdown_simple() {
        let tmp = std::env::temp_dir().join(format!("shannon-agent-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();

        let file_path = tmp.join("code-reviewer.md");
        fs::write(&file_path, "You are a code reviewer. Focus on security.").unwrap();

        let def = AgentDefinition::from_markdown_file(&file_path).unwrap();
        assert_eq!(def.name, "code-reviewer");
        assert_eq!(def.system_prompt.as_deref(), Some("You are a code reviewer. Focus on security."));
        assert!(def.model.is_none());
        assert!(def.temperature.is_none());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn parse_markdown_with_front_matter() {
        let tmp = std::env::temp_dir().join(format!("shannon-agent-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();

        let file_path = tmp.join("backend-dev.md");
        fs::write(&file_path, "---\nmodel: claude-opus\ntemperature: 0.3\ndescription: Backend specialist\n---\nYou are a backend developer.").unwrap();

        let def = AgentDefinition::from_markdown_file(&file_path).unwrap();
        assert_eq!(def.name, "backend-dev");
        assert_eq!(def.system_prompt.as_deref(), Some("You are a backend developer."));
        assert_eq!(def.model.as_deref(), Some("claude-opus"));
        assert_eq!(def.temperature, Some(0.3));
        assert_eq!(def.description, "Backend specialist");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn parse_markdown_with_capabilities() {
        let tmp = std::env::temp_dir().join(format!("shannon-agent-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();

        let file_path = tmp.join("security.md");
        fs::write(&file_path, "---\nmodel: claude-sonnet\ncapabilities: [security, owasp]\n---\nAudit code for vulnerabilities.").unwrap();

        let def = AgentDefinition::from_markdown_file(&file_path).unwrap();
        assert_eq!(def.name, "security");
        assert_eq!(def.capabilities, vec!["security", "owasp"]);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn parse_markdown_empty_body() {
        let tmp = std::env::temp_dir().join(format!("shannon-agent-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();

        let file_path = tmp.join("minimal.md");
        fs::write(&file_path, "").unwrap();

        let def = AgentDefinition::from_markdown_file(&file_path).unwrap();
        assert_eq!(def.name, "minimal");
        assert!(def.system_prompt.is_none());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_registry_from_markdown_dir() {
        let tmp = std::env::temp_dir().join(format!("shannon-agent-test-{}", uuid::Uuid::new_v4()));
        let agents_dir = tmp.join("agents");
        fs::create_dir_all(&agents_dir).unwrap();

        fs::write(agents_dir.join("reviewer.md"), "Review code for bugs.").unwrap();
        fs::write(agents_dir.join("tester.md"), "---\nmodel: claude-haiku\n---\nWrite tests.").unwrap();

        let mut registry = AgentDefinitionRegistry::new();
        registry.load_markdown_from_dir(&agents_dir);

        assert_eq!(registry.all().len(), 2);
        assert!(registry.get("reviewer").is_some());
        assert!(registry.get("tester").is_some());
        assert_eq!(registry.get("tester").unwrap().model.as_deref(), Some("claude-haiku"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
