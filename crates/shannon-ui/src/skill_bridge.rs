//! Bridge between shannon-skills and the ToolRegistry.
//!
//! Provides:
//! - `SkillToolAdapter`: wraps a loaded skill as a `Tool` trait object
//! - `register_skills_as_tools()`: loads skills from default directories
//!   and registers each user-invocable one as a tool in the `ToolRegistry`

use async_trait::async_trait;
use serde_json::{json, Value};
use shannon_core::tools::{Tool, ToolError, ToolOutput, ToolRegistry, ToolResult};
use shannon_skills::{
    Skill, SkillContext, SkillExecutor, SkillPermissions,
    SkillRegistry,
    loader::load_skills_from_directory,
    definition::SkillSource,
    bundled::{BundledSkills, init_bundled_skills},
};
use std::path::PathBuf;
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// SkillToolAdapter
// ---------------------------------------------------------------------------

/// Adapter that wraps a [`Skill`] as a [`Tool`] so it can be registered in
/// the [`ToolRegistry`] and invoked by the query engine like any other tool.
///
/// Only skills that are *user-invocable* are wrapped. Skills whose
/// `disable_model_invocation` is true (pure prompt templates) are also
/// supported -- they return the rendered prompt content as the tool output
/// for the query engine to feed into the LLM.
pub struct SkillToolAdapter {
    /// The wrapped skill definition.
    skill: Skill,
    /// Executor used to render / run the skill.
    executor: SkillExecutor,
    /// Prefixed tool name (e.g. "skill_commit").
    tool_name: String,
}

impl SkillToolAdapter {
    /// Create a new adapter for the given skill.
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
impl Tool for SkillToolAdapter {
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
        // Extract arguments from the input JSON.
        let args_str = input
            .get("args")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let arguments: Vec<String> = if args_str.is_empty() {
            Vec::new()
        } else {
            args_str
                .split_whitespace()
                .map(|s| s.to_string())
                .collect()
        };

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let context = SkillContext {
            arguments,
            cwd,
            session_id: "repl-session".to_string(),
            permissions: SkillPermissions::default(),
        };

        match self.executor.execute(&self.skill, &context) {
            Ok(result) => {
                if result.skip_model_invocation {
                    // Pure command skill -- return output directly.
                    Ok(ToolOutput::success(result.prompt_content))
                } else {
                    // Prompt-template skill -- return the rendered prompt
                    // so the query engine can feed it to the LLM.
                    Ok(ToolOutput::success(result.prompt_content))
                }
            }
            Err(e) => Ok(ToolOutput::error(format!(
                "Skill '{}' execution failed: {}",
                self.skill.name,
                e
            ))),
        }
    }

    fn category(&self) -> &str {
        "skill"
    }
}

// ---------------------------------------------------------------------------
// Registration helpers
// ---------------------------------------------------------------------------

/// Default directories to search for skill files.
fn default_skill_dirs() -> Vec<(PathBuf, SkillSource)> {
    let mut dirs = Vec::new();

    // 1. ~/.shannon/skills/
    if let Some(home) = dirs::home_dir() {
        dirs.push((home.join(".shannon").join("skills"), SkillSource::User));
    }

    // 2. .shannon/skills/ (project-local)
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push((cwd.join(".shannon").join("skills"), SkillSource::Project));
    }

    // 3. .claude/skills/ (Claude Code compatibility)
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push((cwd.join(".claude").join("skills"), SkillSource::Project));
    }

    dirs
}

/// Load skills from the standard directories, create a
/// [`SkillToolAdapter`] for each user-invocable skill, and register them
/// in the provided [`ToolRegistry`].
///
/// Also registers the built-in bundled skills from `shannon-skills`.
///
/// Returns the number of skills that were successfully registered.
///
/// Errors from missing directories or invalid skill files are logged and
/// silently skipped -- the application must not crash because of a bad
/// skill file.
pub fn register_skills_as_tools(registry: &mut ToolRegistry) -> usize {
    let mut count = 0usize;

    // --- Register bundled skills ---
    let bundled = BundledSkills::new();
    if let Err(e) = init_bundled_skills(&bundled) {
        warn!("Failed to initialise bundled skills: {}", e);
    }
    for skill in bundled.list() {
        if !skill.is_user_invocable() {
            continue;
        }
        let adapter = SkillToolAdapter::new(skill);
        match registry.register(Box::new(adapter)) {
            Ok(()) => {
                debug!("Registered bundled skill as tool: skill_{}",
                    registry.list().last().map(|n| n.strip_prefix("skill_").unwrap_or(n)).unwrap_or("?"));
                count += 1;
            }
            Err(e) => {
                warn!("Skipping bundled skill (registration error): {}", e);
            }
        }
    }

    // --- Load user / project skills from disk ---
    let skill_registry = SkillRegistry::new();

    for (dir, source) in default_skill_dirs() {
        if !dir.exists() {
            continue;
        }
        debug!("Loading skills from: {:?}", dir);
        match load_skills_from_directory(&dir, source) {
            Ok(skills) => {
                if let Err(e) = skill_registry.register_all(skills) {
                    warn!("Error registering skills from {:?}: {}", dir, e);
                }
            }
            Err(e) => {
                warn!("Failed to load skills from {:?}: {}", dir, e);
            }
        }
    }

    // Wrap each user-invocable skill as a Tool and register it.
    for skill in skill_registry.list() {
        if !skill.is_user_invocable() {
            continue;
        }
        let adapter = SkillToolAdapter::new(skill);
        match registry.register(Box::new(adapter)) {
            Ok(()) => {
                count += 1;
            }
            Err(e) => {
                // Duplicate name is possible (e.g. bundled + user override).
                // Log and skip rather than crashing.
                debug!("Skill tool registration skipped: {}", e);
            }
        }
    }

    if count > 0 {
        info!("Registered {} skill(s) as tools", count);
    }

    count
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_tool_adapter_name() {
        let skill = Skill::new(
            "my-skill".to_string(),
            "My Skill".to_string(),
            "A test skill".to_string(),
            "Hello ${0}".to_string(),
        );
        let adapter = SkillToolAdapter::new(skill);
        assert_eq!(adapter.name(), "skill_my-skill");
    }

    #[test]
    fn test_skill_tool_adapter_description() {
        let skill = Skill::new(
            "test".to_string(),
            "Test".to_string(),
            "A test description".to_string(),
            "Content".to_string(),
        );
        let adapter = SkillToolAdapter::new(skill);
        assert_eq!(adapter.description(), "A test description");
    }

    #[test]
    fn test_skill_tool_adapter_input_schema() {
        let skill = Skill::new(
            "test".to_string(),
            "Test".to_string(),
            "A test".to_string(),
            "Content".to_string(),
        );
        let adapter = SkillToolAdapter::new(skill);
        let schema = adapter.input_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["args"].is_object());
    }

    #[test]
    fn test_skill_tool_adapter_category() {
        let skill = Skill::new(
            "test".to_string(),
            "Test".to_string(),
            "A test".to_string(),
            "Content".to_string(),
        );
        let adapter = SkillToolAdapter::new(skill);
        assert_eq!(adapter.category(), "skill");
    }

    #[tokio::test]
    async fn test_skill_tool_adapter_execute_simple() {
        let skill = Skill::new(
            "greet".to_string(),
            "Greet".to_string(),
            "Greets the user".to_string(),
            "Hello ${0}!".to_string(),
        );
        let adapter = SkillToolAdapter::new(skill);
        let input = json!({"args": "World"});
        let result = adapter.execute(input).await.unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content, "Hello World!");
    }

    #[tokio::test]
    async fn test_skill_tool_adapter_execute_no_args() {
        let skill = Skill::new(
            "echo".to_string(),
            "Echo".to_string(),
            "Echoes".to_string(),
            "No args provided".to_string(),
        );
        let adapter = SkillToolAdapter::new(skill);
        let input = json!({});
        let result = adapter.execute(input).await.unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content, "No args provided");
    }

    #[test]
    fn test_register_skills_as_tools_does_not_crash() {
        // Calling register_skills_as_tools on a fresh registry should succeed
        // even when no skill directories exist.
        let mut registry = ToolRegistry::new();
        let count = register_skills_as_tools(&mut registry);
        // At minimum the bundled skills should be registered.
        assert!(count > 0, "Expected at least bundled skills to register");
    }

    #[test]
    fn test_skill_tool_adapter_execute_multi_args() {
        let skill = Skill::new(
            "multi".to_string(),
            "Multi".to_string(),
            "Multi-arg skill".to_string(),
            "Args: ${0} and ${1}".to_string(),
        );
        let adapter = SkillToolAdapter::new(skill);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let input = json!({"args": "first second"});
        let result = rt.block_on(adapter.execute(input)).unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content, "Args: first and second");
    }
}
