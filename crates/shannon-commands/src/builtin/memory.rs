//! /memory command - Cross-session memory management
//!
//! Provides persistent memory that survives across sessions, similar to
//! Claude Code's memory system. Stores key facts about projects, preferences,
//! and decisions in `~/.shannon/memory/`.

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

const MEMORY_PROMPT: &str = r##"
Manage cross-session memory for persistent context.

Arguments: {args}

## Subcommands

- **save <text>** — Save a memory entry for this project
- **list** (or no args) — Show all saved memories for this project
- **search <query>** — Search memories by keyword
- **delete <id>** — Delete a specific memory entry
- **clear** — Clear all memories for this project (with confirmation)
- **export** — Export memories as markdown
- **auto** — Toggle auto-memory (automatic context capture)

## How Memory Works

Memories are stored per-project in `~/.shannon/memory/`. Each entry has:
- **id**: Unique identifier
- **content**: The memory text
- **category**: auto-detected (architecture, preference, decision, bug, api, config)
- **created**: Timestamp
- **project**: Associated project path

## Auto-Memory

When auto-memory is enabled (default: on), the system automatically captures:
- Architecture decisions and their rationale
- Bug fixes and root causes
- API patterns and conventions discovered
- Configuration choices and why
- Important file paths and their purposes
- User preferences expressed during conversation

Auto-memory triggers at:
- End of significant code changes
- When bugs are identified and fixed
- When the user says "remember this" or "note this"
- When architectural decisions are made

## Memory Files

- `~/.shannon/memory/<project_hash>.json` — Per-project memories
- `~/.shannon/memory/global.json` — Cross-project memories

## Usage Tips

- Save architectural decisions with rationale: `/memory save "Use tokio channels for inter-agent communication because..."`
- Record user preferences: `/memory save "User prefers snake_case for Rust, camelCase for JS"`
- Mark important discoveries: `/memory save "The auth middleware at src/auth.rs handles JWT validation"`

Memories are automatically included in the system prompt for future sessions.
"##;

/// Create the /memory command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "memory".to_string(),
            aliases: vec!["mem".to_string(), "remember".to_string()],
            description: "Manage cross-session persistent memory".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[save|list|search|delete|clear|export|auto] [text]".to_string()),
            when_to_use: Some(
                "Save important context that should persist across sessions".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Loading memory...".to_string(),
        content_length: 1200,
        arg_names: vec!["action".to_string(), "text".to_string()],
        allowed_tools: vec![
            "Bash(mkdir -p ~/.shannon/memory:*)".to_string(),
            "Bash(cat ~/.shannon/memory/*.json:*)".to_string(),
            "Read".to_string(),
            "Write".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(MEMORY_PROMPT.to_string()),
    }))
}

/// Memory entry stored in JSON
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryEntry {
    /// Unique identifier
    pub id: String,
    /// Memory content
    pub content: String,
    /// Auto-detected category
    pub category: String,
    /// Creation timestamp (ISO 8601)
    pub created: String,
    /// Project path hash
    pub project: String,
}

/// Memory file structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct MemoryFile {
    /// All memory entries
    pub entries: Vec<MemoryEntry>,
    /// Auto-memory enabled
    pub auto_enabled: bool,
}

impl MemoryFile {
    /// Get the memory directory path
    pub fn memory_dir() -> std::path::PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("~"))
            .join(".shannon")
            .join("memory")
    }

    /// Get the project-specific memory file path
    pub fn project_path(project_dir: &str) -> std::path::PathBuf {
        let hash = simple_hash(project_dir);
        Self::memory_dir().join(format!("{hash}.json"))
    }

    /// Load memories for a project
    pub fn load(project_dir: &str) -> Self {
        let path = Self::project_path(project_dir);
        if path.exists() {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Save memories to disk
    pub fn save(&self, project_dir: &str) -> std::io::Result<()> {
        let dir = Self::memory_dir();
        std::fs::create_dir_all(&dir)?;
        let path = Self::project_path(project_dir);
        let content = serde_json::to_string_pretty(self)?;
        // Atomic write via temp file
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &content)?;
        std::fs::rename(&tmp, &path)
    }

    /// Add a new memory entry
    pub fn add(&mut self, content: String, category: String, project: String) -> String {
        let id = format!("mem_{}", simple_hash(&format!("{}{}{}", content, category, chrono_like_now())));
        self.entries.push(MemoryEntry {
            id: id.clone(),
            content,
            category,
            created: chrono_like_now(),
            project,
        });
        id
    }
}

/// Simple hash for project paths (no external dependency)
fn simple_hash(s: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Timestamp without chrono dependency
fn chrono_like_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "memory");
        assert!(cmd.aliases().contains(&"remember".to_string()));
    }

    #[test]
    fn test_memory_entry_creation() {
        let entry = MemoryEntry {
            id: "mem_1234".to_string(),
            content: "Use tokio channels".to_string(),
            category: "architecture".to_string(),
            created: "1234567890".to_string(),
            project: "abcd".to_string(),
        };
        assert_eq!(entry.id, "mem_1234");
        assert_eq!(entry.category, "architecture");
    }

    #[test]
    fn test_simple_hash_deterministic() {
        let h1 = simple_hash("test");
        let h2 = simple_hash("test");
        assert_eq!(h1, h2);
        let h3 = simple_hash("other");
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_memory_file_default() {
        let mf = MemoryFile::default();
        assert!(mf.entries.is_empty());
        assert!(!mf.auto_enabled);
    }
}
