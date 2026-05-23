//! Skill loading from disk

use crate::definition::{Skill, SkillMetadata, SkillSource};
use crate::error::{SkillError, SkillResult};
use crate::frontmatter::parse_skill_frontmatter;
use std::path::{Path, PathBuf};
use tracing::{debug, trace, warn};
use walkdir::WalkDir;

/// Load a skill from a SKILL.md file
pub fn load_skill_from_file(path: &Path) -> SkillResult<Skill> {
    if !path.exists() {
        return Err(SkillError::NotFound(path.display().to_string()));
    }

    let metadata = std::fs::metadata(path).map_err(SkillError::Io)?;
    const MAX_SKILL_SIZE: u64 = 1024 * 1024; // 1MB
    if metadata.len() > MAX_SKILL_SIZE {
        return Err(SkillError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Skill file too large: {} bytes (max {MAX_SKILL_SIZE})",
                metadata.len()
            ),
        )));
    }

    let content = std::fs::read_to_string(path).map_err(SkillError::Io)?;

    let parsed = parse_skill_frontmatter(&content, &path.display().to_string())?;

    // Determine skill name and ID
    let parent_dir = path.parent().unwrap_or_else(|| Path::new(""));

    let skill_dir_name = parent_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let name = parsed
        .frontmatter
        .name
        .clone()
        .unwrap_or_else(|| skill_dir_name.to_string());

    let id = skill_dir_name.to_string();

    // Extract description
    let description = parsed
        .frontmatter
        .description
        .clone()
        .unwrap_or_else(|| extract_description_from_body(&parsed.body));

    // Get skill root (parent directory)
    let skill_root = Some(parent_dir.to_path_buf());

    // Determine if user-invocable
    let user_invocable = parsed.frontmatter.user_invocable.unwrap_or(true);

    // Parse context
    let context = parsed.frontmatter.context;

    let body = parsed.body;
    let content_length = body.len();

    Ok(Skill {
        id,
        name,
        description,
        aliases: parsed.frontmatter.aliases.unwrap_or_default(),
        when_to_use: parsed.frontmatter.when_to_use,
        argument_hint: parsed.frontmatter.argument_hint,
        allowed_tools: parsed.frontmatter.allowed_tools.unwrap_or_default(),
        model: parsed.frontmatter.model,
        disable_model_invocation: parsed.frontmatter.disable_model_invocation.unwrap_or(false),
        user_invocable,
        hooks: parsed.frontmatter.hooks,
        context,
        agent: parsed.frontmatter.agent,
        paths: parsed.frontmatter.paths,
        version: parsed.frontmatter.version,
        source: SkillSource::User, // Will be updated by caller
        skill_root,
        file_path: Some(path.to_path_buf()),
        content: body,
        content_length,
        is_hidden: !user_invocable,
        effort: parsed.frontmatter.effort,
        arguments: parsed.frontmatter.arguments,
        created_at: chrono::Utc::now(),
        updated_at: None,
    })
}

/// Load only metadata from a SKILL.md file (frontmatter only, skips body).
///
/// This is significantly cheaper than [`load_skill_from_file`] when only the
/// name, description, and trigger patterns are needed (e.g. for LLM context
/// injection). The body content is discarded.
pub fn load_metadata_only(path: &Path) -> SkillResult<SkillMetadata> {
    if !path.exists() {
        return Err(SkillError::NotFound(path.display().to_string()));
    }

    let content = std::fs::read_to_string(path).map_err(SkillError::Io)?;

    let parsed = parse_skill_frontmatter(&content, &path.display().to_string())?;

    let parent_dir = path.parent().unwrap_or_else(|| Path::new(""));
    let skill_dir_name = parent_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let name = parsed
        .frontmatter
        .name
        .clone()
        .unwrap_or_else(|| skill_dir_name.to_string());

    let id = skill_dir_name.to_string();

    let description = parsed
        .frontmatter
        .description
        .clone()
        .unwrap_or_else(|| extract_description_from_body(&parsed.body));

    let user_invocable = parsed.frontmatter.user_invocable.unwrap_or(true);

    Ok(SkillMetadata {
        id,
        name,
        description,
        aliases: parsed.frontmatter.aliases.unwrap_or_default(),
        when_to_use: parsed.frontmatter.when_to_use,
        argument_hint: parsed.frontmatter.argument_hint,
        allowed_tools: parsed.frontmatter.allowed_tools.unwrap_or_default(),
        user_invocable,
        is_hidden: !user_invocable,
        source: SkillSource::User,
        file_path: Some(path.to_path_buf()),
    })
}

/// Load a complete skill from disk given a known file path.
///
/// Used for on-demand loading when a skill is invoked. Wraps
/// [`load_skill_from_file`] but sets the source to [`SkillSource::User`].
pub fn load_full_skill(path: &Path) -> SkillResult<Skill> {
    let mut skill = load_skill_from_file(path)?;
    skill.source = SkillSource::User;
    Ok(skill)
}

/// Extract a description from markdown body (first line or heading)
fn extract_description_from_body(body: &str) -> String {
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Skip frontmatter-like content
        if trimmed.starts_with("---") {
            continue;
        }
        // Extract from heading
        if trimmed.starts_with("#") {
            return trimmed.trim_start_matches('#').trim().to_string();
        }
        // Use first non-empty line
        return trimmed.to_string();
    }
    "No description".to_string()
}

/// Load skills from a directory containing skill subdirectories
///
/// Expected structure:
/// skills_dir/
///   skill-name-1/
///     SKILL.md
///   skill-name-2/
///     SKILL.md
pub fn load_skills_from_directory(
    skills_dir: &Path,
    source: SkillSource,
) -> SkillResult<Vec<Skill>> {
    if !skills_dir.exists() {
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();

    for entry in WalkDir::new(skills_dir)
        .min_depth(1)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Look for SKILL.md files
        if path.file_name() == Some(std::ffi::OsStr::new("SKILL.md")) {
            match load_skill_from_file(path) {
                Ok(mut skill) => {
                    skill.source = source.clone();
                    debug!("Loaded skill: {} from {:?}", skill.name, path);
                    skills.push(skill);
                }
                Err(e) => {
                    warn!("Failed to load skill from {:?}: {}", path, e);
                }
            }
        }
    }

    trace!(
        "Loaded {} skills from directory: {:?}",
        skills.len(),
        skills_dir
    );

    Ok(skills)
}

/// Discover skill directories by walking up from file paths
///
/// Searches for `.claude/skills/` and `.shannon/skills/` directories between
/// the given paths and the current working directory, plus user-level skills
/// from `~/.claude/skills/` and `~/.shannon/skills/`.
pub fn discover_skill_directories(file_paths: &[PathBuf], cwd: &Path) -> Vec<PathBuf> {
    let mut discovered = std::collections::HashSet::new();

    // Skill directory names to search (Claude Code compat + Shannon + Agent Skills Standard)
    let skill_dir_names = [".claude/skills", ".shannon/skills", ".agents/skills"];

    // Flat command directory names (Claude Code "simple command" skills)
    let command_dir_names = [".claude/commands", ".shannon/commands"];

    for file_path in file_paths {
        let mut current = file_path.parent().unwrap_or_else(|| Path::new("."));

        while current != cwd && current.starts_with(cwd) {
            for dir_name in &skill_dir_names {
                let skill_dir = current.join(dir_name);
                if skill_dir.exists() && skill_dir.is_dir() {
                    discovered.insert(skill_dir);
                }
            }

            for dir_name in &command_dir_names {
                let cmd_dir = current.join(dir_name);
                if cmd_dir.exists() && cmd_dir.is_dir() {
                    discovered.insert(cmd_dir);
                }
            }

            current = match current.parent() {
                Some(p) if p != Path::new("") => p,
                _ => break,
            };
        }
    }

    // Check cwd level
    for dir_name in &skill_dir_names {
        let cwd_skills = cwd.join(dir_name);
        if cwd_skills.exists() && cwd_skills.is_dir() {
            discovered.insert(cwd_skills);
        }
    }
    for dir_name in &command_dir_names {
        let cwd_cmds = cwd.join(dir_name);
        if cwd_cmds.exists() && cwd_cmds.is_dir() {
            discovered.insert(cwd_cmds);
        }
    }

    // User-level skills (home directory)
    if let Some(home) = dirs::home_dir() {
        for dir_name in &skill_dir_names {
            let user_skills = home.join(dir_name);
            if user_skills.exists() && user_skills.is_dir() {
                discovered.insert(user_skills);
            }
        }
        for dir_name in &command_dir_names {
            let user_cmds = home.join(dir_name);
            if user_cmds.exists() && user_cmds.is_dir() {
                discovered.insert(user_cmds);
            }
        }
    }

    // System-level skills (XDG data directories)
    if let Some(data_local) = dirs::data_local_dir() {
        let xdg_skills = data_local.join("shannon").join("skills");
        if xdg_skills.exists() && xdg_skills.is_dir() {
            discovered.insert(xdg_skills);
        }
        let xdg_agents = data_local.join("agents").join("skills");
        if xdg_agents.exists() && xdg_agents.is_dir() {
            discovered.insert(xdg_agents);
        }
    }

    let mut result: Vec<_> = discovered.into_iter().collect();
    result.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    result
}

/// Load skills from a flat `commands/` directory (Claude Code "simple command" format).
///
/// Unlike the subdirectory-based skill layout (`skill-name/SKILL.md`), this
/// treats each `*.md` file in *commands_dir* as a standalone skill. The
/// filename (without `.md`) becomes the skill ID and default name.
///
/// The source is always set to [`SkillSource::CommandsDeprecated`].
pub fn load_commands_from_directory(
    commands_dir: &Path,
    source: SkillSource,
) -> SkillResult<Vec<Skill>> {
    if !commands_dir.exists() {
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();

    for entry in std::fs::read_dir(commands_dir).map_err(SkillError::Io)? {
        let entry = entry.map_err(SkillError::Io)?;
        let path = entry.path();

        // Only process .md files (skip directories, hidden files, etc.)
        if !path.is_file() {
            continue;
        }

        let extension = path.extension().and_then(|e| e.to_str());
        if extension != Some("md") {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        // Skip hidden files (e.g. .gitkeep.md)
        if stem.starts_with('.') {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to read command file {:?}: {}", path, e);
                continue;
            }
        };

        let parsed = match parse_skill_frontmatter(&content, stem) {
            Ok(p) => p,
            Err(e) => {
                warn!("Failed to parse command file {:?}: {}", path, e);
                continue;
            }
        };

        let name = parsed
            .frontmatter
            .name
            .clone()
            .unwrap_or_else(|| stem.to_string());

        let id = stem.to_string();

        let description = parsed
            .frontmatter
            .description
            .clone()
            .unwrap_or_else(|| extract_description_from_body(&parsed.body));

        let user_invocable = parsed.frontmatter.user_invocable.unwrap_or(true);

        let body = parsed.body;
        let content_length = body.len();

        let skill = Skill {
            id,
            name,
            description,
            aliases: parsed.frontmatter.aliases.unwrap_or_default(),
            when_to_use: parsed.frontmatter.when_to_use,
            argument_hint: parsed.frontmatter.argument_hint,
            allowed_tools: parsed.frontmatter.allowed_tools.unwrap_or_default(),
            model: parsed.frontmatter.model,
            disable_model_invocation: parsed.frontmatter.disable_model_invocation.unwrap_or(false),
            user_invocable,
            hooks: parsed.frontmatter.hooks,
            context: parsed.frontmatter.context,
            agent: parsed.frontmatter.agent,
            paths: parsed.frontmatter.paths,
            version: parsed.frontmatter.version,
            source: source.clone(),
            skill_root: Some(commands_dir.to_path_buf()),
            file_path: Some(path),
            content: body,
            content_length,
            is_hidden: !user_invocable,
            effort: parsed.frontmatter.effort,
            arguments: parsed.frontmatter.arguments,
            created_at: chrono::Utc::now(),
            updated_at: None,
        };

        debug!(
            "Loaded command skill: {} from {:?}",
            skill.name, skill.file_path
        );
        skills.push(skill);
    }

    trace!(
        "Loaded {} command skills from directory: {:?}",
        skills.len(),
        commands_dir
    );

    Ok(skills)
}

/// Validate that a path doesn't escape the base directory.
/// Resolves symlinks via canonicalization to prevent symlink-based traversal.
pub fn validate_path_within_base(path: &Path, base: &Path) -> SkillResult<()> {
    let canonical_path = path
        .canonicalize()
        .map_err(|e| SkillError::PathTraversal(format!("Cannot resolve path {path:?}: {e}")))?;
    let canonical_base = base
        .canonicalize()
        .map_err(|e| SkillError::PathTraversal(format!("Cannot resolve base {base:?}: {e}")))?;

    if !canonical_path.starts_with(&canonical_base) {
        return Err(SkillError::PathTraversal(format!(
            "Path {path:?} resolves outside base directory"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_description_from_body() {
        let body = r#"# My Skill

This is a description."#;
        assert_eq!(extract_description_from_body(body), "My Skill");
    }

    #[test]
    fn test_extract_description_from_plain_text() {
        let body = "Just a simple description";
        assert_eq!(
            extract_description_from_body(body),
            "Just a simple description"
        );
    }

    #[test]
    fn test_validate_path_within_base() {
        // This test may behave differently across systems
        // Just verify that valid paths work correctly
        let base = Path::new("/tmp/test_shannon_skills_validate");
        let _ = std::fs::create_dir_all(base);

        if !base.exists() {
            return; // Skip test if we can't create directory
        }

        // Create a test file
        let test_file = base.join("sub").join("file.txt");
        let _ = std::fs::create_dir_all(test_file.parent().unwrap());
        let _ = std::fs::write(&test_file, "test");

        // Test valid path within base
        assert!(validate_path_within_base(&test_file, base).is_ok());

        // Cleanup
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn test_load_metadata_only_from_file() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_file = skill_dir.join("SKILL.md");
        std::fs::write(
            &skill_file,
            r#"---
name: my-skill
description: A helpful skill
alias:
  - ms
allowed-tools:
  - bash
  - read
argument-hint: "<files>"
---
# My Skill

This is a very long body that should not be loaded
when we only need metadata. It contains detailed instructions
and examples that would consume many tokens if included in
the LLM context window.
"#,
        )
        .unwrap();

        let meta = load_metadata_only(&skill_file).unwrap();
        assert_eq!(meta.id, "my-skill");
        assert_eq!(meta.name, "my-skill");
        assert_eq!(meta.description, "A helpful skill");
        assert_eq!(meta.aliases, vec!["ms".to_string()]);
        assert_eq!(
            meta.allowed_tools,
            vec!["bash".to_string(), "read".to_string()]
        );
        assert_eq!(meta.argument_hint, Some("<files>".to_string()));
        assert!(meta.user_invocable);
        assert!(!meta.is_hidden);
        assert_eq!(meta.file_path, Some(skill_file));
    }

    #[test]
    fn test_load_metadata_only_missing_file() {
        let result = load_metadata_only(Path::new("/nonexistent/SKILL.md"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_metadata_only_no_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("plain-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_file = skill_dir.join("SKILL.md");
        std::fs::write(
            &skill_file,
            "# Plain Skill\n\nJust a body with no frontmatter.\n",
        )
        .unwrap();

        let meta = load_metadata_only(&skill_file).unwrap();
        // Should fall back to directory name for id and name
        assert_eq!(meta.id, "plain-skill");
        assert_eq!(meta.name, "plain-skill");
        // Description extracted from body
        assert_eq!(meta.description, "Plain Skill");
        assert!(meta.aliases.is_empty());
        assert!(meta.allowed_tools.is_empty());
    }

    #[test]
    fn test_load_full_skill_from_file() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("full-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_file = skill_dir.join("SKILL.md");
        std::fs::write(
            &skill_file,
            r#"---
name: full-skill
description: A full skill
---
# Full Skill Body

Detailed instructions go here.
"#,
        )
        .unwrap();

        let skill = load_full_skill(&skill_file).unwrap();
        assert_eq!(skill.id, "full-skill");
        assert_eq!(skill.name, "full-skill");
        assert_eq!(skill.description, "A full skill");
        assert!(skill.content.contains("Detailed instructions go here"));
        assert_eq!(skill.source, SkillSource::User);
    }

    #[test]
    fn test_load_metadata_vs_full_content_difference() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("compare-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_file = skill_dir.join("SKILL.md");

        let body = "X".repeat(10_000);
        let content = format!("---\nname: compare-skill\ndescription: Short desc\n---\n{body}");
        std::fs::write(&skill_file, &content).unwrap();

        let meta = load_metadata_only(&skill_file).unwrap();
        let full = load_full_skill(&skill_file).unwrap();

        // Metadata should not contain the body
        assert_eq!(meta.description, "Short desc");
        // Full skill should contain the body
        assert_eq!(full.content.len(), 10_000);
    }

    // --- Flat command loading tests ---

    #[test]
    fn test_load_commands_from_directory_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let cmds = tmp.path().join("commands");
        std::fs::create_dir_all(&cmds).unwrap();

        // Create a flat .md command file
        std::fs::write(
            cmds.join("deploy.md"),
            "---\ndescription: Deploy the app\n---\n# Deploy\n\nDeploy to production.\n",
        )
        .unwrap();

        let skills = load_commands_from_directory(&cmds, SkillSource::CommandsDeprecated).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "deploy");
        assert_eq!(skills[0].name, "deploy");
        assert_eq!(skills[0].description, "Deploy the app");
        assert!(skills[0].content.contains("Deploy to production"));
        assert_eq!(skills[0].source, SkillSource::CommandsDeprecated);
    }

    #[test]
    fn test_load_commands_from_directory_with_name_override() {
        let tmp = tempfile::tempdir().unwrap();
        let cmds = tmp.path().join("commands");
        std::fs::create_dir_all(&cmds).unwrap();

        std::fs::write(
            cmds.join("my-cmd.md"),
            "---\nname: My Custom Command\ndescription: Custom\n---\nBody here.\n",
        )
        .unwrap();

        let skills = load_commands_from_directory(&cmds, SkillSource::CommandsDeprecated).unwrap();

        assert_eq!(skills[0].id, "my-cmd");
        assert_eq!(skills[0].name, "My Custom Command");
    }

    #[test]
    fn test_load_commands_from_directory_ignores_non_md() {
        let tmp = tempfile::tempdir().unwrap();
        let cmds = tmp.path().join("commands");
        std::fs::create_dir_all(&cmds).unwrap();

        std::fs::write(cmds.join("valid.md"), "# Valid\nBody.\n").unwrap();
        std::fs::write(cmds.join("ignore.txt"), "Not a command.\n").unwrap();
        std::fs::write(cmds.join("ignore.json"), "{}\n").unwrap();

        let skills = load_commands_from_directory(&cmds, SkillSource::CommandsDeprecated).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "valid");
    }

    #[test]
    fn test_load_commands_from_directory_ignores_hidden_files() {
        let tmp = tempfile::tempdir().unwrap();
        let cmds = tmp.path().join("commands");
        std::fs::create_dir_all(&cmds).unwrap();

        std::fs::write(cmds.join("visible.md"), "# Visible\nBody.\n").unwrap();
        std::fs::write(cmds.join(".hidden.md"), "# Hidden\nBody.\n").unwrap();

        let skills = load_commands_from_directory(&cmds, SkillSource::CommandsDeprecated).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "visible");
    }

    #[test]
    fn test_load_commands_from_nonexistent_directory() {
        let skills = load_commands_from_directory(
            Path::new("/nonexistent/commands"),
            SkillSource::CommandsDeprecated,
        )
        .unwrap();
        assert!(skills.is_empty());
    }

    #[test]
    fn test_load_commands_from_directory_no_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let cmds = tmp.path().join("commands");
        std::fs::create_dir_all(&cmds).unwrap();

        std::fs::write(cmds.join("simple.md"), "# Simple Command\n\nJust a body.\n").unwrap();

        let skills = load_commands_from_directory(&cmds, SkillSource::CommandsDeprecated).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "simple");
        assert_eq!(skills[0].name, "simple");
        // Description extracted from body heading
        assert_eq!(skills[0].description, "Simple Command");
    }

    #[test]
    fn test_load_commands_from_directory_multiple_files() {
        let tmp = tempfile::tempdir().unwrap();
        let cmds = tmp.path().join("commands");
        std::fs::create_dir_all(&cmds).unwrap();

        std::fs::write(cmds.join("alpha.md"), "---\n---\nAlpha body.\n").unwrap();
        std::fs::write(cmds.join("beta.md"), "---\n---\nBeta body.\n").unwrap();
        std::fs::write(cmds.join("gamma.md"), "---\n---\nGamma body.\n").unwrap();

        let skills = load_commands_from_directory(&cmds, SkillSource::CommandsDeprecated).unwrap();

        assert_eq!(skills.len(), 3);

        let ids: std::collections::HashSet<_> = skills.iter().map(|s| s.id.clone()).collect();
        assert!(ids.contains("alpha"));
        assert!(ids.contains("beta"));
        assert!(ids.contains("gamma"));
    }
}
