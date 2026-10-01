//! Desktop skill → tool bridge (G1 P0-2.1).
//!
//! The desktop chat engine shares the `ToolRegistry` with MCP tools but,
//! unlike the REPL, never registered installed skills as model-callable
//! tools — a hub-installed skill was browsable in the UI yet invisible in
//! chat. This module ports the REPL bridge
//! (`crates/shannon-ui/src/skill_bridge.rs`) to the desktop crate, which
//! cannot depend on the TUI crate: each user-invocable skill becomes a
//! `skill_<id>` `Tool` backed by the shared `SkillExecutor`.

use async_trait::async_trait;
use serde_json::{Value, json};
use shannon_core::tools::{Tool, ToolOutput, ToolRegistry, ToolResult};
use shannon_skills::{
    Skill, SkillContext, SkillExecutor, SkillPermissions, SkillRegistry, SkillSource,
    bundled::{BundledSkills, init_bundled_skills},
    loader::load_skills_from_directory,
};
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Adapter that wraps a [`Skill`] as a [`Tool`] for the chat registry.
///
/// Mirrors `shannon_ui::skill_bridge::SkillToolAdapter` (same `skill_<id>`
/// naming, same source-derived permission defaults) so a skill behaves
/// identically in the desktop chat and the TUI REPL.
pub struct DesktopSkillToolAdapter {
    skill: Skill,
    executor: SkillExecutor,
    tool_name: String,
}

impl DesktopSkillToolAdapter {
    pub fn new(skill: Skill) -> Self {
        let tool_name = format!("skill_{}", skill.id);
        Self {
            tool_name,
            executor: SkillExecutor::new(),
            skill,
        }
    }
}

#[async_trait]
impl Tool for DesktopSkillToolAdapter {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.skill.description
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "args": {
                    "type": "string",
                    "description": format!("Arguments for the '{}' skill", self.skill.name)
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let args_str = input.get("args").and_then(|v| v.as_str()).unwrap_or("");
        let arguments: Vec<String> = if args_str.is_empty() {
            Vec::new()
        } else {
            args_str.split_whitespace().map(|s| s.to_string()).collect()
        };

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let context = SkillContext {
            arguments,
            cwd,
            session_id: "desktop-session".to_string(),
            effort_level: "medium".to_string(),
            // Source-aware permission defaults (F26): project-sourced skills
            // never get shell execution just by being installed.
            permissions: SkillPermissions::for_source(&self.skill.source),
        };

        match self.executor.execute(&self.skill, &context) {
            Ok(result) => Ok(ToolOutput::success(result.prompt_content)),
            Err(e) => Ok(ToolOutput::error(format!(
                "Skill '{}' execution failed: {}",
                self.skill.name, e
            ))),
        }
    }

    fn category(&self) -> &str {
        "skill"
    }
}

/// Default skill directories, mirroring the REPL bridge: home-level
/// `~/.shannon/skills` (where the extensions hub installs) plus the
/// project-level `.shannon/skills` and `.claude/skills` under cwd.
fn default_skill_dirs() -> Vec<(PathBuf, SkillSource)> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push((home.join(".shannon").join("skills"), SkillSource::User));
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push((cwd.join(".shannon").join("skills"), SkillSource::Project));
        dirs.push((cwd.join(".claude").join("skills"), SkillSource::Project));
    }
    dirs
}

/// Startup hook for the bin crate's `main.rs` setup: registers the chat
/// tools over the shared [`crate::commands::AppState`] registries. Errors
/// (missing dirs, bad SKILL.md, duplicate names) are logged and skipped —
/// never fatal. Returns the number of tools registered.
pub fn register_for_state(state: &crate::commands::AppState) -> usize {
    register_skills_as_chat_tools(&state.tools, &state.skill_registry)
}

/// Imp-3 — the system-prompt block advertising installed skills: the same
/// `format_skills_for_llm()` listing the REPL injects via its skill bridge,
/// plus the `/name` ↔ `skill_<name>` tool mapping the desktop needs so a
/// user's `/trigger` text resolves to the registered tool. Empty when no
/// skills are available.
pub fn skills_for_chat_prompt(skill_registry: &SkillRegistry) -> String {
    let mut block = skill_registry.format_skills_for_llm();
    if block.is_empty() {
        return block;
    }
    block.push_str(
        "\nWhen the user's message is one of these slash triggers (optionally followed by \
         arguments), invoke the matching `skill_<name>` tool with the remaining text as \
         its `args` input instead of answering the raw text directly.",
    );
    block
}

/// Load installed skills (hub-installed home skills + project skills, plus
/// the compile-time bundled set) and register every user-invocable one as a
/// `skill_<id>` tool in the chat registry.
///
/// Also hydrates `skill_registry` so `list_skills` / `get_skill_detail`
/// commands see the same set. Errors (missing dirs, bad SKILL.md, duplicate
/// names) are logged and skipped — never fatal.
///
/// Returns the number of tools registered.
pub fn register_skills_as_chat_tools(
    registry: &ToolRegistry,
    skill_registry: &SkillRegistry,
) -> usize {
    let mut count = 0usize;

    // --- Bundled (compile-time) skills ---
    let bundled = BundledSkills::new();
    if let Err(e) = init_bundled_skills(&bundled) {
        warn!("Failed to initialise bundled skills: {e}");
    }
    for skill in bundled.list() {
        if !skill.is_user_invocable() {
            continue;
        }
        // Imp-3: bundled skills join the LLM-visible registry too, so the
        // chat system prompt advertises the full surface (tools + list).
        if let Err(e) = skill_registry.register(skill.clone()) {
            debug!("bundled skill registry entry skipped: {e}");
        }
        match registry.register(Box::new(DesktopSkillToolAdapter::new(skill))) {
            Ok(()) => count += 1,
            Err(e) => debug!("bundled skill tool registration skipped: {e}"),
        }
    }

    // --- On-disk skills (home + project) ---
    for (dir, source) in default_skill_dirs() {
        if !dir.exists() {
            continue;
        }
        debug!("Loading skills from {:?}", dir);
        match load_skills_from_directory(&dir, source) {
            Ok(skills) => {
                if let Err(e) = skill_registry.register_all(skills) {
                    warn!("Error registering skills from {:?}: {}", dir, e);
                }
            }
            Err(e) => warn!("Failed to load skills from {:?}: {}", dir, e),
        }
    }

    for skill in skill_registry.list() {
        if !skill.is_user_invocable() {
            continue;
        }
        match registry.register(Box::new(DesktopSkillToolAdapter::new(skill))) {
            Ok(()) => count += 1,
            // Duplicates happen (bundled + user override with the same id).
            Err(e) => debug!("skill tool registration skipped: {e}"),
        }
    }

    if count > 0 {
        info!("Registered {count} skill(s) as chat tools");
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_names_skills_as_skill_id() {
        let skill = Skill::new(
            "my-skill".to_string(),
            "My Skill".to_string(),
            "A test skill".to_string(),
            "Hello ${0}".to_string(),
        );
        let adapter = DesktopSkillToolAdapter::new(skill);
        assert_eq!(adapter.name(), "skill_my-skill");
        assert_eq!(adapter.category(), "skill");
        assert_eq!(adapter.description(), "A test skill");
    }

    #[tokio::test]
    async fn adapter_executes_and_renders_args() {
        let skill = Skill::new(
            "greet".to_string(),
            "Greet".to_string(),
            "Greets the user".to_string(),
            "Hello ${0}!".to_string(),
        );
        let adapter = DesktopSkillToolAdapter::new(skill);
        let result = adapter.execute(json!({"args": "World"})).await.unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content, "Hello World!");
    }

    #[test]
    fn register_into_fresh_registry_succeeds() {
        // A fresh registry + no on-disk skills must not fail; bundled skills
        // alone should register at least one tool.
        let registry = ToolRegistry::new();
        let skill_registry = SkillRegistry::new();
        let count = register_skills_as_chat_tools(&registry, &skill_registry);
        assert!(count > 0, "expected bundled skills to register");
    }

    /// Imp-3: bundled skills join the LLM-visible registry, and the prompt
    /// block lists them with triggers plus the `skill_<name>` mapping note.
    #[test]
    fn skills_for_chat_prompt_lists_skills_and_tool_mapping() {
        let registry = ToolRegistry::new();
        let skill_registry = SkillRegistry::new();
        register_skills_as_chat_tools(&registry, &skill_registry);

        let block = skills_for_chat_prompt(&skill_registry);
        assert!(block.starts_with("Available skills"), "{block}");
        assert!(
            block.contains("skill_<name>"),
            "mapping note missing: {block}"
        );
        // Bundled skills are part of the advertised surface.
        assert!(block.contains("commit"), "{block}");
    }

    #[test]
    fn skills_for_chat_prompt_empty_when_no_skills() {
        let empty = SkillRegistry::new();
        assert_eq!(skills_for_chat_prompt(&empty), "");
    }
}
