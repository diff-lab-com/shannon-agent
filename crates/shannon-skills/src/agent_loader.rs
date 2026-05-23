//! Agent definition loading from Claude Code-compatible `.claude/agents/*.md` files.
//!
//! Agent definitions are Markdown files with YAML frontmatter that describe
//! specialized AI agents with their own system prompts, tool restrictions,
//! model preferences, and execution settings.
//!
//! ## File Format
//!
//! ```markdown
//! ---
//! name: code-reviewer
//! description: Reviews code for quality and best practices
//! tools: Read, Glob, Grep, Bash
//! disallowedTools: Write, Edit
//! model: sonnet
//! ---
//!
//! You are a code reviewer...
//! ```
//!
//! ## Discovery
//!
//! Agents are discovered from `.claude/agents/` and `.shannon/agents/`
//! directories, searched from the current working directory upward and
//! in the user's home directory.

use crate::error::SkillError;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Isolation mode for agent execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentIsolation {
    /// No special isolation.
    #[default]
    None,
    /// Run the agent in a dedicated git worktree.
    Worktree,
}

/// Colour used to visually identify an agent in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentColor {
    #[default]
    Default,
    Red,
    Blue,
    Green,
    Yellow,
    Purple,
    Orange,
    Pink,
    Cyan,
}

/// Model preference for an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentModel {
    #[default]
    Inherit,
    Haiku,
    Sonnet,
    Opus,
}

/// Permission mode for agent execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AgentPermissionMode {
    #[default]
    Default,
    AcceptEdits,
    Auto,
    DontAsk,
    BypassPermissions,
    Plan,
}

/// Effort level for an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentEffort {
    Low,
    #[default]
    Medium,
    High,
    Xhigh,
    Max,
}

/// Intermediate representation used only during YAML deserialization.
///
/// The `tools` and `disallowed_tools` fields accept several shapes:
/// - a YAML list (`[Read, Glob]`)
/// - a comma-separated string (`"Read, Glob"`)
/// - a space-separated string (`"Read Glob"`)
///
/// After deserialization the strings are split into individual items.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct AgentFrontmatter {
    name: Option<String>,
    description: Option<String>,
    tools: Option<serde_yaml::Value>,
    #[serde(rename = "disallowedTools")]
    disallowed_tools: Option<serde_yaml::Value>,
    model: Option<AgentModel>,
    #[serde(rename = "permissionMode")]
    permission_mode: Option<AgentPermissionMode>,
    #[serde(rename = "maxTurns")]
    max_turns: Option<u32>,
    effort: Option<AgentEffort>,
    skills: Option<Vec<String>>,
    isolation: Option<AgentIsolation>,
    color: Option<AgentColor>,
    background: Option<bool>,
}

/// A fully parsed agent definition ready for use by the runtime.
#[derive(Debug, Clone)]
pub struct AgentDefinition {
    /// Unique agent identifier (lowercase + hyphens).
    pub name: String,
    /// Human-readable description of when to use this agent.
    pub description: String,
    /// Tools the agent is explicitly allowed to use.
    pub tools: Vec<String>,
    /// Tools the agent must not use.
    pub disallowed_tools: Vec<String>,
    /// Model preference.
    pub model: AgentModel,
    /// Permission mode.
    pub permission_mode: AgentPermissionMode,
    /// Maximum agentic turns.
    pub max_turns: Option<u32>,
    /// Effort level.
    pub effort: AgentEffort,
    /// Skills to preload for this agent.
    pub skills: Vec<String>,
    /// Isolation mode.
    pub isolation: AgentIsolation,
    /// Visual colour identifier.
    pub color: AgentColor,
    /// Whether to always run as a background task.
    pub background: bool,
    /// System prompt (body of the markdown file).
    pub prompt: String,
    /// File the definition was loaded from.
    pub source_path: PathBuf,
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Convert a YAML value that may be a list or a string into a `Vec<String>`.
fn parse_string_list(value: Option<&serde_yaml::Value>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };

    match value {
        serde_yaml::Value::Sequence(seq) => seq
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        serde_yaml::Value::String(s) => split_tool_string(s),
        _ => Vec::new(),
    }
}

/// Split a comma- or space-separated tool string into individual names.
fn split_tool_string(s: &str) -> Vec<String> {
    // If the string contains commas, split on commas; otherwise split on whitespace.
    if s.contains(',') {
        s.split(',')
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect()
    } else {
        s.split_whitespace().map(|part| part.to_string()).collect()
    }
}

/// Extract frontmatter (between `---` markers) and body from content.
///
/// Reuses the same logic as the skill frontmatter parser.
fn extract_frontmatter(content: &str) -> Result<(&str, &str), SkillError> {
    if !content.starts_with("---") {
        return Ok(("", content));
    }

    let rest = &content[3..]; // Skip opening ---
    let end_idx = rest
        .find("\n---")
        .ok_or_else(|| SkillError::InvalidFormat("Missing closing --- in frontmatter".into()))?;

    let frontmatter = &rest[..end_idx];
    let body_start = end_idx + 4; // Skip \n---
    let body = &rest[body_start..];

    Ok((frontmatter, body))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse an agent definition from raw Markdown content.
///
/// `source_path` is recorded on the resulting [`AgentDefinition`] for
/// traceability but is not read by this function.
pub fn parse_agent_definition(
    content: &str,
    source_path: &Path,
) -> Result<AgentDefinition, SkillError> {
    let (frontmatter_str, body) = extract_frontmatter(content)?;

    let fm: AgentFrontmatter = if frontmatter_str.trim().is_empty() {
        AgentFrontmatter::default()
    } else {
        serde_yaml::from_str(frontmatter_str).map_err(|e| SkillError::FrontmatterParse {
            name: source_path.display().to_string(),
            message: e.to_string(),
        })?
    };

    let name = fm.name.ok_or_else(|| SkillError::InvalidMetadata {
        path: source_path.to_path_buf(),
        reason: "missing required field: name".into(),
    })?;

    let description = fm.description.ok_or_else(|| SkillError::InvalidMetadata {
        path: source_path.to_path_buf(),
        reason: "missing required field: description".into(),
    })?;

    let tools = parse_string_list(fm.tools.as_ref());
    let disallowed_tools = parse_string_list(fm.disallowed_tools.as_ref());

    Ok(AgentDefinition {
        name,
        description,
        tools,
        disallowed_tools,
        model: fm.model.unwrap_or_default(),
        permission_mode: fm.permission_mode.unwrap_or_default(),
        max_turns: fm.max_turns,
        effort: fm.effort.unwrap_or_default(),
        skills: fm.skills.unwrap_or_default(),
        isolation: fm.isolation.unwrap_or_default(),
        color: fm.color.unwrap_or_default(),
        background: fm.background.unwrap_or(false),
        prompt: body.trim().to_string(),
        source_path: source_path.to_path_buf(),
    })
}

/// Load all `*.md` agent definitions from a directory.
///
/// Non-`.md` files and sub-directories are silently skipped.  Individual
/// parse failures are logged as warnings and do not prevent other files from
/// being loaded.
pub fn load_agents_from_directory(agents_dir: &Path) -> Result<Vec<AgentDefinition>, SkillError> {
    if !agents_dir.exists() {
        return Ok(Vec::new());
    }

    let mut agents = Vec::new();

    for entry in WalkDir::new(agents_dir)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Only process .md files
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }

        if !path.is_file() {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to read agent file {:?}: {}", path, e);
                continue;
            }
        };

        match parse_agent_definition(&content, path) {
            Ok(agent) => {
                debug!("Loaded agent: {} from {:?}", agent.name, path);
                agents.push(agent);
            }
            Err(e) => {
                warn!("Failed to parse agent from {:?}: {}", path, e);
            }
        }
    }

    debug!("Loaded {} agents from {:?}", agents.len(), agents_dir);
    Ok(agents)
}

/// Discover agent directories by searching from `cwd` upward and in the
/// user's home directory.
///
/// Looks for `.claude/agents/` and `.shannon/agents/` in the following
/// locations (in order):
///
/// 1. The current working directory
/// 2. Each parent directory up to the filesystem root
/// 3. The user's home directory (`~/.claude/agents/`, `~/.shannon/agents/`)
///
/// Returns directories that exist, sorted so that deeper (more specific)
/// paths come first.
pub fn discover_agent_directories(cwd: &Path) -> Vec<PathBuf> {
    let mut discovered = std::collections::HashSet::new();

    let agent_dir_names = [".claude/agents", ".shannon/agents"];

    // Walk from cwd upward to the filesystem root.
    let mut current: &Path = cwd;
    loop {
        for dir_name in &agent_dir_names {
            let agent_dir = current.join(dir_name);
            if agent_dir.is_dir() {
                discovered.insert(agent_dir);
            }
        }

        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }

    // User-level agents from home directory.
    if let Some(home) = dirs::home_dir() {
        for dir_name in &agent_dir_names {
            let home_agents = home.join(dir_name);
            if home_agents.is_dir() {
                discovered.insert(home_agents);
            }
        }
    }

    let mut result: Vec<_> = discovered.into_iter().collect();
    // Deeper paths first (more specific projects override general ones).
    result.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_full_agent_definition() {
        let content = r#"---
name: code-reviewer
description: Reviews code for quality and best practices
tools:
  - Read
  - Glob
  - Grep
  - Bash
disallowedTools:
  - Write
  - Edit
model: sonnet
permissionMode: auto
maxTurns: 20
effort: high
skills:
  - api-conventions
  - error-handling-patterns
isolation: worktree
color: blue
background: true
---

You are a code reviewer. Analyze code and provide specific, actionable feedback.
"#;

        let def = parse_agent_definition(content, Path::new("code-reviewer.md")).unwrap();

        assert_eq!(def.name, "code-reviewer");
        assert_eq!(
            def.description,
            "Reviews code for quality and best practices"
        );
        assert_eq!(def.tools, vec!["Read", "Glob", "Grep", "Bash"]);
        assert_eq!(def.disallowed_tools, vec!["Write", "Edit"]);
        assert_eq!(def.model, AgentModel::Sonnet);
        assert_eq!(def.permission_mode, AgentPermissionMode::Auto);
        assert_eq!(def.max_turns, Some(20));
        assert_eq!(def.effort, AgentEffort::High);
        assert_eq!(
            def.skills,
            vec!["api-conventions", "error-handling-patterns"]
        );
        assert_eq!(def.isolation, AgentIsolation::Worktree);
        assert_eq!(def.color, AgentColor::Blue);
        assert!(def.background);
        assert_eq!(
            def.prompt,
            "You are a code reviewer. Analyze code and provide specific, actionable feedback."
        );
    }

    #[test]
    fn parse_minimal_agent() {
        let content = r#"---
name: simple-agent
description: A bare-bones agent
---

Just do the thing.
"#;

        let def = parse_agent_definition(content, Path::new("simple.md")).unwrap();

        assert_eq!(def.name, "simple-agent");
        assert_eq!(def.description, "A bare-bones agent");
        assert!(def.tools.is_empty());
        assert!(def.disallowed_tools.is_empty());
        assert_eq!(def.model, AgentModel::Inherit);
        assert_eq!(def.permission_mode, AgentPermissionMode::Default);
        assert_eq!(def.max_turns, None);
        assert_eq!(def.effort, AgentEffort::Medium);
        assert!(def.skills.is_empty());
        assert_eq!(def.isolation, AgentIsolation::None);
        assert_eq!(def.color, AgentColor::Default);
        assert!(!def.background);
        assert_eq!(def.prompt, "Just do the thing.");
    }

    #[test]
    fn parse_tools_as_comma_separated_string() {
        let content = r#"---
name: string-tools
description: Uses string-formatted tools
tools: "Read, Glob, Grep"
disallowedTools: Write, Edit
---

Body.
"#;

        let def = parse_agent_definition(content, Path::new("str.md")).unwrap();
        assert_eq!(def.tools, vec!["Read", "Glob", "Grep"]);
        assert_eq!(def.disallowed_tools, vec!["Write", "Edit"]);
    }

    #[test]
    fn parse_tools_as_space_separated_string() {
        let content = r#"---
name: space-tools
description: Uses space-separated tools
tools: Read Glob Grep Bash
---

Body.
"#;

        let def = parse_agent_definition(content, Path::new("space.md")).unwrap();
        assert_eq!(def.tools, vec!["Read", "Glob", "Grep", "Bash"]);
    }

    #[test]
    fn missing_name_returns_error() {
        let content = r#"---
description: No name field
---

Body.
"#;

        let err = parse_agent_definition(content, Path::new("bad.md")).unwrap_err();
        match err {
            SkillError::InvalidMetadata { reason, .. } => {
                assert!(
                    reason.contains("name"),
                    "expected 'name' in error, got: {reason}"
                );
            }
            other => panic!("expected InvalidMetadata, got: {other}"),
        }
    }

    #[test]
    fn missing_description_returns_error() {
        let content = r#"---
name: has-name
---

Body.
"#;

        let err = parse_agent_definition(content, Path::new("bad.md")).unwrap_err();
        match err {
            SkillError::InvalidMetadata { reason, .. } => {
                assert!(
                    reason.contains("description"),
                    "expected 'description' in error, got: {reason}"
                );
            }
            other => panic!("expected InvalidMetadata, got: {other}"),
        }
    }

    #[test]
    fn missing_closing_frontmatter_returns_error() {
        let content = r#"---
name: broken
description: No closing delimiter

This is just body with no closing ---.
"#;

        let err = parse_agent_definition(content, Path::new("broken.md")).unwrap_err();
        match err {
            SkillError::InvalidFormat(msg) => {
                assert!(
                    msg.contains("closing"),
                    "expected 'closing' in error, got: {msg}"
                );
            }
            other => panic!("expected InvalidFormat, got: {other}"),
        }
    }

    #[test]
    fn no_frontmatter_returns_error_due_to_missing_name() {
        // No frontmatter at all - entire content treated as body, then name is missing.
        let content = "Just a body with no frontmatter at all.\n";

        let err = parse_agent_definition(content, Path::new("nofm.md")).unwrap_err();
        match err {
            SkillError::InvalidMetadata { reason, .. } => {
                assert!(reason.contains("name"));
            }
            other => panic!("expected InvalidMetadata, got: {other}"),
        }
    }

    #[test]
    fn invalid_yaml_returns_frontmatter_parse_error() {
        let content = r#"---
name: [invalid yaml
description: broken
---

Body.
"#;

        let err = parse_agent_definition(content, Path::new("badyaml.md")).unwrap_err();
        match err {
            SkillError::FrontmatterParse { .. } => {}
            other => panic!("expected FrontmatterParse, got: {other}"),
        }
    }

    #[test]
    fn load_agents_from_directory_works() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join("agents");
        fs::create_dir_all(&agents_dir).unwrap();

        // Write two agent files.
        fs::write(
            agents_dir.join("reviewer.md"),
            r#"---
name: reviewer
description: Reviews code
tools:
  - Read
  - Grep
model: haiku
---

You review code.
"#,
        )
        .unwrap();

        fs::write(
            agents_dir.join("builder.md"),
            r#"---
name: builder
description: Builds features
tools: Write, Edit, Bash
maxTurns: 30
---

You build features.
"#,
        )
        .unwrap();

        // Write a non-md file that should be skipped.
        fs::write(agents_dir.join("notes.txt"), "not an agent").unwrap();

        let agents = load_agents_from_directory(&agents_dir).unwrap();
        assert_eq!(agents.len(), 2);

        let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"reviewer"));
        assert!(names.contains(&"builder"));

        // Verify one agent's content in detail.
        let reviewer = agents.iter().find(|a| a.name == "reviewer").unwrap();
        assert_eq!(reviewer.tools, vec!["Read", "Grep"]);
        assert_eq!(reviewer.model, AgentModel::Haiku);
        assert_eq!(reviewer.prompt, "You review code.");
    }

    #[test]
    fn load_agents_from_nonexistent_directory() {
        let agents = load_agents_from_directory(Path::new("/no/such/directory")).unwrap();
        assert!(agents.is_empty());
    }

    #[test]
    fn load_agents_skips_bad_files() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join("agents");
        fs::create_dir_all(&agents_dir).unwrap();

        // A valid agent.
        fs::write(
            agents_dir.join("good.md"),
            r#"---
name: good-agent
description: A good one
---

Good body.
"#,
        )
        .unwrap();

        // An invalid agent (missing name).
        fs::write(
            agents_dir.join("bad.md"),
            r#"---
description: No name here
---

Bad body.
"#,
        )
        .unwrap();

        let agents = load_agents_from_directory(&agents_dir).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "good-agent");
    }

    #[test]
    fn all_effort_variants() {
        for (yaml, expected) in [
            ("low", AgentEffort::Low),
            ("medium", AgentEffort::Medium),
            ("high", AgentEffort::High),
            ("xhigh", AgentEffort::Xhigh),
            ("max", AgentEffort::Max),
        ] {
            let content = format!("---\nname: e\ndescription: d\neffort: {yaml}\n\n---\nbody\n");
            let def = parse_agent_definition(&content, Path::new("e.md")).unwrap();
            assert_eq!(def.effort, expected, "failed for effort={yaml}");
        }
    }

    #[test]
    fn all_model_variants() {
        for (yaml, expected) in [
            ("haiku", AgentModel::Haiku),
            ("sonnet", AgentModel::Sonnet),
            ("opus", AgentModel::Opus),
            ("inherit", AgentModel::Inherit),
        ] {
            let content = format!("---\nname: m\ndescription: d\nmodel: {yaml}\n\n---\nbody\n");
            let def = parse_agent_definition(&content, Path::new("m.md")).unwrap();
            assert_eq!(def.model, expected, "failed for model={yaml}");
        }
    }

    #[test]
    fn all_permission_mode_variants() {
        for (yaml, expected) in [
            ("default", AgentPermissionMode::Default),
            ("acceptEdits", AgentPermissionMode::AcceptEdits),
            ("auto", AgentPermissionMode::Auto),
            ("dontAsk", AgentPermissionMode::DontAsk),
            ("bypassPermissions", AgentPermissionMode::BypassPermissions),
            ("plan", AgentPermissionMode::Plan),
        ] {
            let content =
                format!("---\nname: p\ndescription: d\npermissionMode: {yaml}\n\n---\nbody\n");
            let def = parse_agent_definition(&content, Path::new("p.md")).unwrap();
            assert_eq!(
                def.permission_mode, expected,
                "failed for permissionMode={yaml}"
            );
        }
    }

    #[test]
    fn all_color_variants() {
        for (yaml, expected) in [
            ("default", AgentColor::Default),
            ("red", AgentColor::Red),
            ("blue", AgentColor::Blue),
            ("green", AgentColor::Green),
            ("yellow", AgentColor::Yellow),
            ("purple", AgentColor::Purple),
            ("orange", AgentColor::Orange),
            ("pink", AgentColor::Pink),
            ("cyan", AgentColor::Cyan),
        ] {
            let content = format!("---\nname: c\ndescription: d\ncolor: {yaml}\n\n---\nbody\n");
            let def = parse_agent_definition(&content, Path::new("c.md")).unwrap();
            assert_eq!(def.color, expected, "failed for color={yaml}");
        }
    }

    #[test]
    fn discover_agent_directories_finds_existing_dirs() {
        let tmp = tempfile::tempdir().unwrap();

        // Create .claude/agents in the temp dir.
        let claude_agents = tmp.path().join(".claude").join("agents");
        fs::create_dir_all(&claude_agents).unwrap();

        // Create .shannon/agents in the temp dir.
        let shannon_agents = tmp.path().join(".shannon").join("agents");
        fs::create_dir_all(&shannon_agents).unwrap();

        let dirs = discover_agent_directories(tmp.path());
        assert!(
            dirs.len() >= 2,
            "expected at least 2 directories, got {dirs:?}"
        );

        let paths: Vec<String> = dirs.iter().map(|p| p.display().to_string()).collect();
        assert!(
            paths.iter().any(|p| p.contains(".claude/agents")),
            "missing .claude/agents in {paths:?}"
        );
        assert!(
            paths.iter().any(|p| p.contains(".shannon/agents")),
            "missing .shannon/agents in {paths:?}"
        );
    }

    #[test]
    fn discover_agent_directories_returns_empty_when_none_exist() {
        let tmp = tempfile::tempdir().unwrap();
        // tmp has no .claude/agents or .shannon/agents.
        let dirs = discover_agent_directories(tmp.path());
        // May still find home-level dirs, so just ensure no panic.
        // Filter to only paths under tmp for assertion.
        let local: Vec<_> = dirs.iter().filter(|p| p.starts_with(tmp.path())).collect();
        assert!(local.is_empty());
    }

    #[test]
    fn split_tool_string_comma_vs_space() {
        assert_eq!(
            split_tool_string("Read, Glob, Grep"),
            vec!["Read", "Glob", "Grep"]
        );
        assert_eq!(
            split_tool_string("Read Glob Grep"),
            vec!["Read", "Glob", "Grep"]
        );
        assert_eq!(split_tool_string("  Read ,  Glob  "), vec!["Read", "Glob"]);
        assert!(split_tool_string("").is_empty());
    }
}
