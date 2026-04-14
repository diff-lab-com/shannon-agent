//! Auto-loading project instructions from CLAUDE.md / AGENTS.md files.

use std::path::{Path, PathBuf};

/// Default filenames to search for, in priority order.
const INSTRUCTION_FILES: &[&str] = &["CLAUDE.md", "AGENTS.md", "GEMINI.md"];

/// Result of loading project instructions.
#[derive(Debug, Clone)]
pub struct ProjectInstructions {
    /// The combined content of all found instruction files.
    pub content: String,
    /// Which files were found and loaded.
    pub loaded_files: Vec<String>,
}

/// Load project instructions from the given directory and all parent directories.
///
/// Searches for `CLAUDE.md`, `AGENTS.md`, and `GEMINI.md` in the working directory
/// and each parent directory up to the filesystem root. Files found deeper in the
/// tree (closer to the working directory) are placed *after* those from parent
/// directories, so the most specific instructions come last and take visual precedence.
///
/// Returns `None` if no instruction files are found.
pub fn load_from_directory(dir: &Path) -> Option<ProjectInstructions> {
    let mut found: Vec<(PathBuf, String)> = Vec::new();

    // Walk up from dir to root, collecting instruction files
    let mut current = Some(dir.to_path_buf());
    while let Some(path) = current.take() {
        for filename in INSTRUCTION_FILES {
            let candidate = path.join(filename);
            if candidate.is_file() {
                if let Ok(content) = std::fs::read_to_string(&candidate) {
                    if !content.trim().is_empty() {
                        found.push((candidate, content));
                    }
                }
            }
        }
        current = path.parent().map(|p| p.to_path_buf());
    }

    if found.is_empty() {
        return None;
    }

    // Reverse so that root-level files come first, working-dir files last
    found.reverse();

    let loaded_files: Vec<String> = found
        .iter()
        .map(|(p, _)| {
            p.strip_prefix(dir)
                .unwrap_or(p)
                .to_string_lossy()
                .to_string()
        })
        .collect();

    let mut content = String::from("# Project Instructions\n\n");
    for (path, file_content) in &found {
        let display_name = path
            .strip_prefix(dir)
            .unwrap_or(path)
            .to_string_lossy();
        content.push_str(&format!("--- {display_name} ---\n\n{file_content}\n\n"));
    }

    Some(ProjectInstructions {
        content,
        loaded_files,
    })
}

/// Load project instructions from the current working directory.
pub fn load_from_cwd() -> Option<ProjectInstructions> {
    std::env::current_dir()
        .ok()
        .and_then(|dir| load_from_directory(&dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_load_nonexistent_dir() {
        assert!(load_from_directory(Path::new("/nonexistent/path/xyz")).is_none());
    }

    #[test]
    fn test_load_empty_dir() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        assert!(load_from_directory(&tmp).is_none());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_claude_md() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Test\n\nUse Rust best practices.").unwrap();

        let result = load_from_directory(&tmp);
        assert!(result.is_some());
        let instructions = result.unwrap();
        assert!(instructions.content.contains("Use Rust best practices"));
        assert!(instructions.loaded_files.contains(&"CLAUDE.md".to_string()));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_multiple_files() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Claude rules").unwrap();
        fs::write(tmp.join("AGENTS.md"), "# Agent rules").unwrap();

        let result = load_from_directory(&tmp);
        assert!(result.is_some());
        let instructions = result.unwrap();
        assert!(instructions.content.contains("Claude rules"));
        assert!(instructions.content.contains("Agent rules"));
        assert_eq!(instructions.loaded_files.len(), 2);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_empty_file_skipped() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("CLAUDE.md"), "   \n  \n").unwrap();

        assert!(load_from_directory(&tmp).is_none());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_parent_directory() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        let child = tmp.join("subdir");
        fs::create_dir_all(&child).unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Parent project rules").unwrap();

        let result = load_from_directory(&child);
        assert!(result.is_some());
        assert!(result.unwrap().content.contains("Parent project rules"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
