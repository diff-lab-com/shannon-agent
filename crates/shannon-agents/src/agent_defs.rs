//! Custom agent definitions loaded from `.shannon/agents/*.toml`.
//!
//! Agent definitions allow users to pre-configure agent types with custom
//! system prompts, capabilities, models, and tool access — similar to
//! Claude Code's `.claude/agents/*.md` pattern.
//!
//! ## File Format (TOML)
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
//! Files are loaded from:
//! 1. `.shannon/agents/` (project-local, checked into VCS)
//! 2. `~/.shannon/agents/` (user-global)

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
    /// Project-local definitions (`.shannon/agents/`) take precedence over
    /// user-global definitions (`~/.shannon/agents/`).
    pub fn load_from_dirs() -> Self {
        let mut registry = Self::new();

        // Load user-global definitions first (lower priority)
        if let Some(home) = dirs::home_dir() {
            let global_dir = home.join(".shannon").join("agents");
            if global_dir.is_dir() {
                registry.load_from_dir(&global_dir);
            }
        }

        // Load project-local definitions (higher priority, overrides global)
        let local_dir = PathBuf::from(".shannon").join("agents");
        if local_dir.is_dir() {
            registry.load_from_dir(&local_dir);
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
}
