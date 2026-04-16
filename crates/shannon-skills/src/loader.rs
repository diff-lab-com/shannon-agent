//! Skill loading from disk

use crate::definition::{Skill, SkillSource};
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

    let content = std::fs::read_to_string(path)
        .map_err(SkillError::Io)?;

    let parsed = parse_skill_frontmatter(&content, &path.display().to_string())?;

    // Determine skill name and ID
    let parent_dir = path.parent()
        .unwrap_or_else(|| Path::new(""));

    let skill_dir_name = parent_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let name = parsed.frontmatter.name
        .clone()
        .unwrap_or_else(|| skill_dir_name.to_string());

    let id = skill_dir_name.to_string();

    // Extract description
    let description = parsed.frontmatter.description
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
        created_at: chrono::Utc::now(),
        updated_at: None,
    })
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
pub fn discover_skill_directories(
    file_paths: &[PathBuf],
    cwd: &Path,
) -> Vec<PathBuf> {
    let mut discovered = std::collections::HashSet::new();

    // Skill directory names to search (Claude Code compat + Shannon)
    let skill_dir_names = [".claude/skills", ".shannon/skills"];

    for file_path in file_paths {
        let mut current = file_path.parent()
            .unwrap_or_else(|| Path::new("."));

        while current != cwd && current.starts_with(cwd) {
            for dir_name in &skill_dir_names {
                let skill_dir = current.join(dir_name);
                if skill_dir.exists() && skill_dir.is_dir() {
                    discovered.insert(skill_dir);
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

    // User-level skills (home directory)
    if let Some(home) = dirs::home_dir() {
        for dir_name in &skill_dir_names {
            let user_skills = home.join(dir_name);
            if user_skills.exists() && user_skills.is_dir() {
                discovered.insert(user_skills);
            }
        }
    }

    let mut result: Vec<_> = discovered.into_iter().collect();
    result.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    result
}

/// Validate that a path doesn't escape the base directory
pub fn validate_path_within_base(path: &Path, base: &Path) -> SkillResult<()> {
    let canonical_path = path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf());
    let canonical_base = base.canonicalize()
        .unwrap_or_else(|_| base.to_path_buf());

    if !canonical_path.starts_with(&canonical_base) {
        return Err(SkillError::PathTraversal(format!(
            "Path {path:?} escapes base {base:?}"
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
        assert_eq!(extract_description_from_body(body), "Just a simple description");
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
}
