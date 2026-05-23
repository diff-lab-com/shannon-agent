//! Integration tests for agent loading and discovery
//!
//! Tests:
//! - Agent definition parsing from markdown files on disk
//! - Loading agents from directories with mixed valid/invalid files
//! - Agent directory discovery walking up the filesystem
//! - Edge cases in agent frontmatter (empty body, extra fields, etc.)

use shannon_skills::{
    AgentColor, AgentEffort, AgentIsolation, AgentModel, AgentPermissionMode,
    discover_agent_directories, load_agents_from_directory, parse_agent_definition,
};
use std::fs;
use std::path::Path;

/// Helper: write an agent markdown file to a temp directory and return its path.
fn write_agent_file(dir: &Path, filename: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(filename);
    fs::write(&path, content).unwrap();
    path
}

#[test]
fn test_parse_agent_with_all_fields_from_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_agent_file(
        tmp.path(),
        "full-agent.md",
        r#"---
name: full-agent
description: An agent with every field populated
tools:
  - Read
  - Glob
disallowedTools:
  - Write
model: opus
permissionMode: bypassPermissions
maxTurns: 50
effort: max
skills:
  - code-review
isolation: worktree
color: purple
background: true
---

You are a full-featured agent.
"#,
    );

    let content = fs::read_to_string(&path).unwrap();
    let def = parse_agent_definition(&content, &path).unwrap();

    assert_eq!(def.name, "full-agent");
    assert_eq!(def.description, "An agent with every field populated");
    assert_eq!(def.tools, vec!["Read", "Glob"]);
    assert_eq!(def.disallowed_tools, vec!["Write"]);
    assert_eq!(def.model, AgentModel::Opus);
    assert_eq!(def.permission_mode, AgentPermissionMode::BypassPermissions);
    assert_eq!(def.max_turns, Some(50));
    assert_eq!(def.effort, AgentEffort::Max);
    assert_eq!(def.skills, vec!["code-review"]);
    assert_eq!(def.isolation, AgentIsolation::Worktree);
    assert_eq!(def.color, AgentColor::Purple);
    assert!(def.background);
    assert_eq!(def.prompt, "You are a full-featured agent.");
    assert_eq!(def.source_path, path);
}

#[test]
fn test_parse_agent_defaults_when_minimal_frontmatter() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_agent_file(
        tmp.path(),
        "minimal.md",
        r#"---
name: tiny
description: Bare minimum
---

Do something simple.
"#,
    );

    let content = fs::read_to_string(&path).unwrap();
    let def = parse_agent_definition(&content, &path).unwrap();

    assert_eq!(def.name, "tiny");
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
}

#[test]
fn test_parse_agent_ignores_unknown_fields() {
    let content = r#"---
name: extra-fields
description: Has extra YAML fields
futureField: should be ignored
customConfig:
  enabled: true
---

Body.
"#;
    let def = parse_agent_definition(content, Path::new("extra.md")).unwrap();
    assert_eq!(def.name, "extra-fields");
    assert_eq!(def.prompt, "Body.");
}

#[test]
fn test_parse_agent_empty_body_is_trimmed() {
    let content = "---\nname: empty-body\ndescription: No prompt\n---\n   \n";
    let def = parse_agent_definition(content, Path::new("empty.md")).unwrap();
    assert_eq!(def.prompt, "");
}

#[test]
fn test_load_agents_from_directory_ignores_subdirectories() {
    let tmp = tempfile::tempdir().unwrap();
    let agents_dir = tmp.path().join("agents");
    fs::create_dir_all(&agents_dir).unwrap();

    // Valid agent file at top level
    write_agent_file(
        &agents_dir,
        "top-level.md",
        r#"---
name: top-level
description: At top level
---
Top level agent.
"#,
    );

    // Subdirectory should not be traversed
    let subdir = agents_dir.join("subdir");
    fs::create_dir_all(&subdir).unwrap();
    write_agent_file(
        &subdir,
        "nested.md",
        r#"---
name: nested
description: In subdirectory
---
Nested agent.
"#,
    );

    let agents = load_agents_from_directory(&agents_dir).unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].name, "top-level");
}

#[test]
fn test_load_agents_from_directory_with_multiple_valid_files() {
    let tmp = tempfile::tempdir().unwrap();
    let agents_dir = tmp.path().join("agents");
    fs::create_dir_all(&agents_dir).unwrap();

    write_agent_file(
        &agents_dir,
        "agent-a.md",
        r#"---
name: agent-a
description: First agent
tools: Read, Grep
model: haiku
---
Agent A prompt.
"#,
    );

    write_agent_file(
        &agents_dir,
        "agent-b.md",
        r#"---
name: agent-b
description: Second agent
tools:
  - Write
  - Edit
  - Bash
effort: high
---
Agent B prompt.
"#,
    );

    let agents = load_agents_from_directory(&agents_dir).unwrap();
    assert_eq!(agents.len(), 2);

    let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
    assert!(names.contains(&"agent-a"));
    assert!(names.contains(&"agent-b"));

    let a = agents.iter().find(|a| a.name == "agent-a").unwrap();
    assert_eq!(a.tools, vec!["Read", "Grep"]);
    assert_eq!(a.model, AgentModel::Haiku);

    let b = agents.iter().find(|a| a.name == "agent-b").unwrap();
    assert_eq!(b.tools, vec!["Write", "Edit", "Bash"]);
    assert_eq!(b.effort, AgentEffort::High);
}

#[test]
fn test_load_agents_skips_non_markdown_files() {
    let tmp = tempfile::tempdir().unwrap();
    let agents_dir = tmp.path().join("agents");
    fs::create_dir_all(&agents_dir).unwrap();

    // Write valid agent
    write_agent_file(
        &agents_dir,
        "valid.md",
        r#"---
name: valid
description: Valid agent
---
Valid.
"#,
    );

    // Write non-agent files (non-.md extensions are skipped)
    fs::write(agents_dir.join("readme.txt"), "Not an agent").unwrap();
    fs::write(agents_dir.join("config.yaml"), "key: value").unwrap();
    fs::write(agents_dir.join("data.json"), "{}").unwrap();

    let agents = load_agents_from_directory(&agents_dir).unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].name, "valid");
}

#[test]
fn test_discover_agent_directories_from_temp_dir() {
    let tmp = tempfile::tempdir().unwrap();

    // Create both .claude/agents and .shannon/agents
    let claude_agents = tmp.path().join(".claude").join("agents");
    let shannon_agents = tmp.path().join(".shannon").join("agents");
    fs::create_dir_all(&claude_agents).unwrap();
    fs::create_dir_all(&shannon_agents).unwrap();

    let dirs = discover_agent_directories(tmp.path());

    // Filter to only paths under tmp (home dir may also contribute)
    let local: Vec<_> = dirs.iter().filter(|p| p.starts_with(tmp.path())).collect();

    assert_eq!(local.len(), 2);
    assert!(local.iter().any(|p| p.ends_with(".claude/agents")));
    assert!(local.iter().any(|p| p.ends_with(".shannon/agents")));
}

#[test]
fn test_discover_agent_directories_only_existing_dirs() {
    let tmp = tempfile::tempdir().unwrap();

    // Create only .claude/agents, not .shannon/agents
    let claude_agents = tmp.path().join(".claude").join("agents");
    fs::create_dir_all(&claude_agents).unwrap();

    let dirs = discover_agent_directories(tmp.path());
    let local: Vec<_> = dirs.iter().filter(|p| p.starts_with(tmp.path())).collect();

    assert_eq!(local.len(), 1);
    assert!(local[0].ends_with(".claude/agents"));
}
