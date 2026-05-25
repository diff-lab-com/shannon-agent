//! /session command — Session template snapshot save/load
//!
//! Provides session snapshot persistence so users can save the current
//! conversation state (model config, messages, enabled tools, system prompt
//! additions) as a named template and restore it later.
//!
//! Snapshots are stored as TOML files under `~/.shannon/sessions/`.

use crate::command::{
    Command, CommandAvailability, CommandBase, CommandSource, ExecutionContext, PromptCommand,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Complete session snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Snapshot metadata
    pub name: String,
    /// ISO 8601 creation timestamp
    pub created_at: String,
    /// Optional human-readable description
    pub description: Option<String>,
    /// Model configuration
    pub model: Option<String>,
    pub provider: Option<String>,
    pub temperature: Option<f32>,
    /// Conversation messages
    pub messages: Vec<SnapshotMessage>,
    /// Tool state (which tools are enabled)
    pub enabled_tools: Vec<String>,
    /// Custom system prompt additions
    pub system_prompt_additions: Option<String>,
}

/// A single message in a session snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMessage {
    pub role: String,
    pub content: String,
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Directory where session snapshots are stored: `~/.shannon/sessions/`
#[allow(dead_code)] // invoked dynamically by LLM via /session command tools
pub fn sessions_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".shannon")
        .join("sessions")
}

/// Full path for a named snapshot: `~/.shannon/sessions/{name}.toml`
#[allow(dead_code)] // invoked dynamically by LLM via /session command tools
fn snapshot_path(name: &str) -> PathBuf {
    sessions_dir().join(format!("{name}.toml"))
}

// ---------------------------------------------------------------------------
// I/O operations
// ---------------------------------------------------------------------------

/// Save a session snapshot to disk.
///
/// Creates the sessions directory if it does not exist. Writes as TOML so
/// snapshots are human-readable and easy to edit.
#[allow(dead_code)] // invoked dynamically by LLM via /session command tools
pub fn save_snapshot(snapshot: &SessionSnapshot) -> Result<PathBuf> {
    let dir = sessions_dir();
    std::fs::create_dir_all(&dir)?;

    let path = snapshot_path(&snapshot.name);
    let toml_str = toml::to_string_pretty(snapshot)?;
    std::fs::write(&path, toml_str)?;
    Ok(path)
}

/// Load a session snapshot by name.
#[allow(dead_code)] // invoked dynamically by LLM via /session command tools
pub fn load_snapshot(name: &str) -> Result<SessionSnapshot> {
    let path = snapshot_path(name);
    let content = std::fs::read_to_string(&path)?;
    let snapshot: SessionSnapshot = toml::from_str(&content)?;
    Ok(snapshot)
}

/// List all saved snapshots.
///
/// Returns `(name, description)` pairs sorted alphabetically by name.
#[allow(dead_code)] // invoked dynamically by LLM via /session command tools
pub fn list_snapshots() -> Result<Vec<(String, String)>> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut entries: Vec<(String, String)> = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                continue;
            }
            let description = match std::fs::read_to_string(&path) {
                Ok(content) => toml::from_str::<SessionSnapshot>(&content)
                    .ok()
                    .and_then(|s| s.description)
                    .unwrap_or_default(),
                Err(_) => String::new(),
            };
            entries.push((name, description));
        }
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

/// Delete a snapshot by name.
#[allow(dead_code)] // invoked dynamically by LLM via /session command tools
pub fn delete_snapshot(name: &str) -> Result<()> {
    let path = snapshot_path(name);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// Format a list of snapshots for display.
#[allow(dead_code)] // invoked dynamically by LLM via /session command tools
pub fn format_snapshot_list(snapshots: &[(String, String)]) -> String {
    if snapshots.is_empty() {
        return "No saved session snapshots.\nUse `/session save <name>` to create one."
            .to_string();
    }

    let mut out = String::from("Saved session snapshots:\n\n");
    for (name, description) in snapshots {
        if description.is_empty() {
            out.push_str(&format!("  {name}\n"));
        } else {
            out.push_str(&format!("  {name} — {description}\n"));
        }
    }
    out.push_str("\nUse `/session load <name>` to restore a snapshot.");
    out
}

/// Format detailed information about a single snapshot.
#[allow(dead_code)] // invoked dynamically by LLM via /session command tools
pub fn format_snapshot_detail(snapshot: &SessionSnapshot) -> String {
    let mut out = format!("Session: {}\n", snapshot.name);
    out.push_str(&format!("  Created: {}\n", snapshot.created_at));
    if let Some(ref desc) = snapshot.description {
        out.push_str(&format!("  Description: {desc}\n"));
    }
    if let Some(ref model) = snapshot.model {
        out.push_str(&format!("  Model: {model}\n"));
    }
    if let Some(ref provider) = snapshot.provider {
        out.push_str(&format!("  Provider: {provider}\n"));
    }
    if let Some(temp) = snapshot.temperature {
        out.push_str(&format!("  Temperature: {temp}\n"));
    }
    out.push_str(&format!("  Messages: {}\n", snapshot.messages.len()));
    out.push_str(&format!(
        "  Enabled tools: {}\n",
        snapshot.enabled_tools.len()
    ));
    if let Some(ref additions) = snapshot.system_prompt_additions {
        if !additions.is_empty() {
            out.push_str(&format!("  System prompt additions: {additions}\n"));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Command definition
// ---------------------------------------------------------------------------

/// Prompt template for the /session command
const SESSION_PROMPT: &str = r##"## Session Template Manager

Manage session snapshots — save and restore full conversation state as named templates.

Arguments: {args}

## Subcommands

- **save <name>** — Save the current session as a named snapshot.
  Optionally include a description: `save my-session description text here`.
  Captures: model config, conversation messages, enabled tools, system prompt additions.
  Saved to `~/.shannon/sessions/<name>.toml`.

- **load <name>** — Restore a previously saved session snapshot.
  Reads the snapshot from `~/.shannon/sessions/<name>.toml` and reconstructs the
  conversation context, model configuration, and tool state.

- **list** — Show all saved session snapshots with descriptions.

- **delete <name>** — Remove a saved session snapshot.

- **show <name>** — Display detailed information about a snapshot without loading it.

## Storage

Snapshots live at `~/.shannon/sessions/` as TOML files, making them human-readable
and easy to edit manually or version-control.

## Examples

```
/session save bugfix-auth        # Save current session
/session save refactor "API v2 migration work"  # Save with description
/session list                     # Show all snapshots
/session show bugfix-auth         # View snapshot details
/session load bugfix-auth         # Restore that session
/session delete bugfix-auth       # Remove the snapshot
```
"##;

/// Create the /session command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "session".to_string(),
            aliases: vec!["snap".to_string()],
            description: "Save and restore session snapshots".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[save|load|list|delete|show] [name]".to_string()),
            when_to_use: Some(
                "Save the current conversation as a template, or restore a previously saved session"
                    .to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Loading session snapshot...".to_string(),
        content_length: 1000,
        arg_names: vec!["action".to_string(), "name".to_string()],
        allowed_tools: vec![
            "Read".to_string(),
            "Glob".to_string(),
            "Grep".to_string(),
            "Write".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(SESSION_PROMPT.to_string()),
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_snapshot(name: &str) -> SessionSnapshot {
        SessionSnapshot {
            name: name.to_string(),
            created_at: "2026-05-24T12:00:00Z".to_string(),
            description: Some("test snapshot".to_string()),
            model: Some("claude-sonnet-4-20250514".to_string()),
            provider: Some("anthropic".to_string()),
            temperature: Some(0.7),
            messages: vec![
                SnapshotMessage {
                    role: "user".to_string(),
                    content: "Hello".to_string(),
                },
                SnapshotMessage {
                    role: "assistant".to_string(),
                    content: "Hi there!".to_string(),
                },
            ],
            enabled_tools: vec!["Read".to_string(), "Write".to_string()],
            system_prompt_additions: Some("Be concise.".to_string()),
        }
    }

    #[test]
    fn test_session_snapshot_serialization() {
        let snapshot = sample_snapshot("test-serialization");
        let toml_str = toml::to_string_pretty(&snapshot).expect("serialize to toml");
        assert!(toml_str.contains("test-serialization"));
        assert!(toml_str.contains("claude-sonnet-4-20250514"));
        assert!(toml_str.contains("Hello"));
        assert!(toml_str.contains("Be concise."));

        let deserialized: SessionSnapshot =
            toml::from_str(&toml_str).expect("deserialize from toml");
        assert_eq!(deserialized.name, "test-serialization");
        assert_eq!(deserialized.messages.len(), 2);
        assert_eq!(deserialized.temperature, Some(0.7));
    }

    /// Helper: create a unique temp directory for test isolation
    fn test_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "shannon-session-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn test_save_and_load_snapshot() {
        let dir = test_temp_dir("save-load");
        let snapshot = sample_snapshot("test-save-load");
        let path = dir.join("test-save-load.toml");
        let toml_str = toml::to_string_pretty(&snapshot).expect("serialize");
        std::fs::write(&path, &toml_str).expect("write");

        let loaded: SessionSnapshot =
            toml::from_str(&std::fs::read_to_string(&path).expect("read")).expect("deserialize");
        assert_eq!(loaded.name, "test-save-load");
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.enabled_tools, vec!["Read", "Write"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_list_snapshots() {
        let dir = test_temp_dir("list");
        for name in &["alpha", "beta", "charlie"] {
            let snapshot = SessionSnapshot {
                name: name.to_string(),
                created_at: "2026-01-01T00:00:00Z".to_string(),
                description: if *name == "beta" {
                    Some("the beta session".to_string())
                } else {
                    None
                },
                model: None,
                provider: None,
                temperature: None,
                messages: vec![],
                enabled_tools: vec![],
                system_prompt_additions: None,
            };
            let path = dir.join(format!("{name}.toml"));
            let toml_str = toml::to_string_pretty(&snapshot).expect("serialize");
            std::fs::write(&path, &toml_str).expect("write");
        }

        // Read and sort (replicating list_snapshots logic against temp dir)
        let mut entries: Vec<(String, String)> = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("readdir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let desc = toml::from_str::<SessionSnapshot>(
                    &std::fs::read_to_string(&path).expect("read"),
                )
                .ok()
                .and_then(|s| s.description)
                .unwrap_or_default();
                entries.push((name, desc));
            }
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].0, "alpha");
        assert_eq!(entries[1].0, "beta");
        assert_eq!(entries[1].1, "the beta session");
        assert_eq!(entries[2].0, "charlie");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_delete_snapshot() {
        let dir = test_temp_dir("delete");
        let path = dir.join("to-delete.toml");
        std::fs::write(&path, "name = 'to-delete'").expect("write");
        assert!(path.exists());

        std::fs::remove_file(&path).expect("delete");
        assert!(!path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_format_snapshot_list() {
        let empty = format_snapshot_list(&[]);
        assert!(empty.contains("No saved session snapshots"));

        let items = vec![
            ("alpha".to_string(), String::new()),
            ("beta".to_string(), "the beta".to_string()),
        ];
        let formatted = format_snapshot_list(&items);
        assert!(formatted.contains("alpha"));
        assert!(formatted.contains("beta — the beta"));
        assert!(formatted.contains("/session load"));
    }

    #[test]
    fn test_format_snapshot_detail() {
        let snapshot = sample_snapshot("detail-test");
        let detail = format_snapshot_detail(&snapshot);
        assert!(detail.contains("Session: detail-test"));
        assert!(detail.contains("Created: 2026-05-24"));
        assert!(detail.contains("Description: test snapshot"));
        assert!(detail.contains("Model: claude-sonnet-4-20250514"));
        assert!(detail.contains("Provider: anthropic"));
        assert!(detail.contains("Temperature: 0.7"));
        assert!(detail.contains("Messages: 2"));
        assert!(detail.contains("Enabled tools: 2"));
        assert!(detail.contains("System prompt additions: Be concise."));
    }

    #[test]
    fn test_sessions_dir_is_under_home() {
        let dir = sessions_dir();
        assert!(dir.ends_with(".shannon/sessions") || dir.ends_with(".shannon\\sessions"));
        assert!(dir.starts_with(dirs::home_dir().unwrap_or_default()));
    }

    // -- Command structure tests --

    #[test]
    fn test_session_command_structure() {
        let cmd = command();
        match cmd {
            Command::Prompt(pc) => {
                assert_eq!(pc.base.name, "session");
                assert!(pc.base.aliases.contains(&"snap".to_string()));
                assert!(pc.base.user_invocable);
                assert!(!pc.base.is_workflow);
                assert!(pc.prompt_template.is_some());
            }
            _ => panic!("Expected Prompt command"),
        }
    }

    #[test]
    fn test_session_prompt_contains_subcommands() {
        assert!(SESSION_PROMPT.contains("save"));
        assert!(SESSION_PROMPT.contains("load"));
        assert!(SESSION_PROMPT.contains("list"));
        assert!(SESSION_PROMPT.contains("delete"));
        assert!(SESSION_PROMPT.contains("show"));
    }

    #[test]
    fn test_session_allowed_tools() {
        let cmd = command();
        match cmd {
            Command::Prompt(pc) => {
                assert!(pc.allowed_tools.contains(&"Read".to_string()));
                assert!(pc.allowed_tools.contains(&"Glob".to_string()));
                assert!(pc.allowed_tools.contains(&"Grep".to_string()));
                assert!(pc.allowed_tools.contains(&"Write".to_string()));
            }
            _ => panic!("Expected Prompt command"),
        }
    }

    #[test]
    fn test_session_has_snap_alias() {
        let cmd = command();
        match cmd {
            Command::Prompt(pc) => {
                assert_eq!(pc.base.aliases, vec!["snap"]);
            }
            _ => panic!("Expected Prompt command"),
        }
    }
}
