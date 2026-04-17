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

/// Gather git context (branch, recent commits, status summary) as a string.
/// Returns None if not in a git repo or git is unavailable.
pub fn git_context(dir: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let mut ctx = String::from("## Git Context\n\n");

    // Current branch
    if let Ok(branch_out) = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir)
        .output()
    {
        if branch_out.status.success() {
            let branch = String::from_utf8_lossy(&branch_out.stdout).trim().to_string();
            if !branch.is_empty() {
                ctx.push_str(&format!("Branch: {branch}\n"));
            }
        }
    }

    // Recent commits (last 5)
    if let Ok(log_out) = std::process::Command::new("git")
        .args(["log", "--oneline", "-5"])
        .current_dir(dir)
        .output()
    {
        if log_out.status.success() {
            let log = String::from_utf8_lossy(&log_out.stdout).trim().to_string();
            if !log.is_empty() {
                ctx.push_str(&format!("Recent commits:\n{log}\n"));
            }
        }
    }

    // Status summary
    if let Ok(status_out) = std::process::Command::new("git")
        .args(["status", "--short"])
        .current_dir(dir)
        .output()
    {
        if status_out.status.success() {
            let status = String::from_utf8_lossy(&status_out.stdout).trim().to_string();
            if !status.is_empty() {
                let count = status.lines().count();
                ctx.push_str(&format!("Working tree: {count} changed file(s)\n"));
            } else {
                ctx.push_str("Working tree: clean\n");
            }
        }
    }

    Some(ctx)
}

/// Load full project context: instruction files + git context.
/// Returns None only if nothing at all is available.
pub fn load_full_context(dir: &Path) -> Option<ProjectInstructions> {
    let mut all_content = String::new();
    let mut all_files: Vec<String> = Vec::new();

    // Load user-level instructions from ~/.claude/CLAUDE.md (global user instructions)
    if let Some(home) = dirs::home_dir() {
        for filename in INSTRUCTION_FILES {
            let home_file = home.join(".claude").join(filename);
            if home_file.is_file() {
                if let Ok(content) = std::fs::read_to_string(&home_file) {
                    if !content.trim().is_empty() {
                        all_content.push_str(&format!("--- ~/.claude/{filename} ---\n\n{content}\n\n"));
                        all_files.push(format!("~/.claude/{filename}"));
                    }
                }
            }
        }
    }

    // Load project-level instructions (walks up from dir to root)
    if let Some(proj) = load_from_directory(dir) {
        all_content.push_str(&proj.content);
        all_files.extend(proj.loaded_files);
    }

    // Load git context
    if let Some(git) = git_context(dir) {
        all_content.push_str(&git);
        all_files.push("git context".to_string());
    }

    if all_content.is_empty() {
        None
    } else {
        Some(ProjectInstructions {
            content: all_content,
            loaded_files: all_files,
        })
    }
}

// ---------------------------------------------------------------------------
// InstructionWatcher — lightweight mtime-based hot-reload
// ---------------------------------------------------------------------------

/// Tracks modification times of project instruction files and detects changes.
///
/// Uses a simple mtime comparison — no external file-watching dependencies needed.
/// Call `check_and_reload()` before each query to detect changes and get updated
/// instructions.
#[derive(Debug)]
pub struct InstructionWatcher {
    /// The root directory to watch (project working directory).
    watch_dir: PathBuf,
    /// Map of file path → last known modification time.
    mtimes: std::collections::HashMap<PathBuf, std::time::SystemTime>,
    /// Cached combined instruction content (for when nothing changed).
    cached_content: Option<String>,
}

impl InstructionWatcher {
    /// Create a new watcher for the given working directory.
    pub fn new(watch_dir: PathBuf) -> Self {
        let mut watcher = Self {
            watch_dir,
            mtimes: std::collections::HashMap::new(),
            cached_content: None,
        };
        // Initial scan
        let _ = watcher.scan_mtimes();
        watcher
    }

    /// Scan all instruction files and record their mtimes.
    fn scan_mtimes(&mut self) -> std::collections::HashMap<PathBuf, std::time::SystemTime> {
        let mut current_mtimes = std::collections::HashMap::new();

        // Check home-level instructions
        if let Some(home) = dirs::home_dir() {
            for filename in INSTRUCTION_FILES {
                let path = home.join(".claude").join(filename);
                if path.is_file() {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        if let Ok(mtime) = meta.modified() {
                            current_mtimes.insert(path, mtime);
                        }
                    }
                }
            }
        }

        // Walk up from watch_dir to root
        let mut current = Some(self.watch_dir.clone());
        while let Some(path) = current.take() {
            for filename in INSTRUCTION_FILES {
                let candidate = path.join(filename);
                if candidate.is_file() {
                    if let Ok(meta) = std::fs::metadata(&candidate) {
                        if let Ok(mtime) = meta.modified() {
                            current_mtimes.insert(candidate, mtime);
                        }
                    }
                }
            }
            current = path.parent().map(|p| p.to_path_buf());
        }

        current_mtimes
    }

    /// Check if any instruction files have changed since the last check.
    ///
    /// Returns `Some((changed_files, new_content))` if files changed, `None` if unchanged.
    pub fn check_and_reload(&mut self) -> Option<(Vec<String>, String)> {
        let new_mtimes = self.scan_mtimes();

        // Check if mtimes changed or files added/removed
        let changed = new_mtimes.len() != self.mtimes.len()
            || new_mtimes.iter().any(|(path, mtime)| {
                self.mtimes.get(&*path).map_or(true, |old| old != mtime)
            });

        if !changed {
            return None;
        }

        // Reload instructions
        let changed_paths: Vec<String> = new_mtimes
            .keys()
            .filter(|p| {
                self.mtimes.get(&**p).map_or(true, |old| {
                    new_mtimes.get(&**p).map_or(false, |cur| cur != old)
                })
            })
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        self.mtimes = new_mtimes;

        let new_content = match load_full_context(&self.watch_dir) {
            Some(instr) => instr.content,
            None => String::new(),
        };

        self.cached_content = Some(new_content.clone());
        Some((changed_paths, new_content))
    }

    /// Get the cached instruction content (reload first if needed).
    pub fn cached_instructions(&self) -> Option<&str> {
        self.cached_content.as_deref()
    }
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

    #[test]
    fn test_git_context_in_repo() {
        // This test runs in the shannon-code repo itself, so git context should work
        let cwd = std::env::current_dir().unwrap();
        let ctx = git_context(&cwd);
        // We're in a git repo, so should get Some
        assert!(ctx.is_some(), "Should get git context in a git repo");
        let ctx = ctx.unwrap();
        assert!(ctx.contains("Branch"), "Should contain branch info");
        assert!(ctx.contains("Recent commits"), "Should contain recent commits");
    }

    #[test]
    fn test_git_context_not_repo() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let ctx = git_context(&tmp);
        assert!(ctx.is_none(), "Should return None for non-git directory");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_full_context_with_git() {
        // Running in shannon-code repo: both instructions and git context should load
        let cwd = std::env::current_dir().unwrap();
        let result = load_full_context(&cwd);
        assert!(result.is_some(), "Should load full context in shannon-code repo");
        let instr = result.unwrap();
        // Should have either CLAUDE.md or git context (or both)
        assert!(
            instr.loaded_files.contains(&"CLAUDE.md".to_string())
                || instr.loaded_files.contains(&"git context".to_string()),
            "Should load at least one source"
        );
    }

    #[test]
    fn test_load_full_context_nothing() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let result = load_full_context(&tmp);
        // May or may not be None depending on whether ~/.claude/CLAUDE.md exists
        // The important thing is it doesn't panic and returns a valid result
        if let Some(instr) = result {
            // If something was loaded, it should only be user-level or git context
            assert!(!instr.content.is_empty());
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_full_context_instructions_only() {
        let tmp = std::env::temp_dir().join(format!("shannon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("CLAUDE.md"), "# Test instructions").unwrap();
        let result = load_full_context(&tmp);
        assert!(result.is_some(), "Should load instructions even without git");
        let instr = result.unwrap();
        assert!(instr.content.contains("Test instructions"));
        assert!(instr.loaded_files.contains(&"CLAUDE.md".to_string()));
        let _ = fs::remove_dir_all(&tmp);
    }
}
