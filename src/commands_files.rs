//! File-related commands — text save, diff, apply, tree, working-dir info.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).
//! More file commands will move here in future extractions.

use serde::{Deserialize, Serialize};

use crate::commands::AppState;
use crate::commands_agents::resolve_working_dir;
use crate::events::HunkAction;
use crate::resolve_path_in_working_dir;

/// Write text content to a file, creating parent directories as needed.
#[tauri::command]
pub async fn save_text_file(path: String, content: String) -> Result<(), String> {
    let target = std::path::Path::new(&path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
    }
    std::fs::write(target, content)
        .map_err(|e| format!("Failed to write {}: {e}", target.display()))
}

/// File diff result for the diff viewer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    pub old_content: String,
    pub new_content: String,
    pub file_name: String,
    pub language: String,
}

/// A node in the file tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTreeNode {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub node_type: String, // "file" or "directory"
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<FileTreeNode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// Working directory info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingDirInfo {
    pub root: String,
    pub branch: String,
    pub modified_files: Vec<String>,
    pub status: String, // "clean", "dirty", "merge-conflict"
}

/// Get the diff for a file (working tree vs last committed, or old vs new content).
#[tauri::command]
pub async fn get_file_diff(path: String) -> Result<FileDiff, String> {
    use std::process::Command;

    // Validate path is within CWD to prevent path traversal
    let file_path = std::path::Path::new(&path);
    let canonical = file_path
        .canonicalize()
        .map_err(|e| format!("Invalid path: {e}"))?;
    let cwd = std::env::current_dir()
        .map_err(|e| format!("Cannot determine CWD: {e}"))?
        .canonicalize()
        .map_err(|e| format!("Cannot canonicalize CWD: {e}"))?;
    if !canonical.starts_with(&cwd) {
        return Err("Path outside workspace".to_string());
    }

    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());

    // Detect language from extension
    let language = file_path
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "plaintext".to_string());

    // Try git diff first
    let dir = file_path.parent().unwrap_or(std::path::Path::new("."));
    let git_output = Command::new("git")
        .args(["diff", "HEAD", "--", &path])
        .current_dir(dir)
        .output();

    let (old_content, new_content) = match git_output {
        Ok(output) if output.status.success() && !output.stdout.is_empty() => {
            // Parse unified diff - for simplicity, just read current file as new
            // and reconstruct old from git show
            let new = std::fs::read_to_string(&path).unwrap_or_default();
            let old_output = Command::new("git")
                .args(["show", &format!("HEAD:{}", path)])
                .current_dir(dir)
                .output();
            let old = match old_output {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
                _ => String::new(),
            };
            (old, new)
        }
        _ => {
            // Not a git repo or no changes - read file as new, empty old
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            (String::new(), content)
        }
    };

    Ok(FileDiff {
        old_content,
        new_content,
        file_name,
        language,
    })
}

/// Apply diff with hunk actions.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn apply_diff(
    state: tauri::State<'_, AppState>,
    file_path: String,
    hunks: Vec<HunkAction>,
) -> Result<(), String> {
    use std::fs;
    use std::io::Write;

    // Security: validate the file path is inside the working directory. The
    // previous `contains("..")` check was insufficient — it allowed absolute
    // paths like `/etc/hosts`, and did not catch symlinks that escape the
    // workspace. Canonicalize + starts_with closes all three holes at once.
    let working_dir = resolve_working_dir(&state).await;
    let path = resolve_path_in_working_dir(&file_path, &working_dir)?;
    if !path.is_file() {
        return Err(format!("File not found: {}", path.display()));
    }
    let file_path = path.to_string_lossy().into_owned();

    // Read current file content
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read file {}: {}", file_path, e))?;

    let mut lines: Vec<&str> = content.lines().collect();

    // Apply hunk actions in reverse order to maintain line numbers
    let mut sorted_hunks: Vec<_> = hunks.iter().enumerate().collect();
    sorted_hunks.sort_by_key(|(idx, h)| (std::cmp::Reverse(h.line_start), *idx));

    for (idx, hunk) in sorted_hunks {
        if hunk.line_start == 0 || hunk.line_end == 0 {
            continue; // Invalid hunk
        }

        let start_idx = (hunk.line_start - 1) as usize;
        let end_idx = hunk.line_end as usize;

        if start_idx >= lines.len() || end_idx > lines.len() {
            return Err(format!("Hunk {} out of bounds for file {}", idx, file_path));
        }

        match hunk.action.as_str() {
            "accept" => {
                // Keep the lines (do nothing)
            }
            "reject" => {
                // Remove the lines by replacing with empty strings
                for i in start_idx..end_idx {
                    lines[i] = "";
                }
            }
            _ => {
                return Err(format!("Unknown action {} in hunk {}", hunk.action, idx));
            }
        }
    }

    // Write back the modified content
    let modified_content = lines.join("\n") + "\n";
    let mut file = fs::File::create(&file_path)
        .map_err(|e| format!("Failed to create file {}: {}", file_path, e))?;
    file.write_all(modified_content.as_bytes())
        .map_err(|e| format!("Failed to write file {}: {}", file_path, e))?;

    Ok(())
}

/// Recursively read a directory and return a file tree.
#[tauri::command]
pub async fn get_file_tree(path: String) -> Result<Vec<FileTreeNode>, String> {
    use std::fs;
    let root = std::path::Path::new(&path);
    if !root.is_dir() {
        return Err("Path is not a directory".into());
    }
    fn build_tree(dir: &std::path::Path) -> Result<Vec<FileTreeNode>, String> {
        let mut entries: Vec<std::fs::DirEntry> = fs::read_dir(dir)
            .map_err(|e| format!("Cannot read dir: {e}"))?
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                !name.starts_with('.') && name != "target" && name != "node_modules"
            })
            .collect();
        entries.sort_by(|a, b| {
            let a_is_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let b_is_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
            b_is_dir.cmp(&a_is_dir).then_with(|| {
                a.file_name()
                    .to_string_lossy()
                    .cmp(&b.file_name().to_string_lossy())
            })
        });
        let mut nodes = Vec::new();
        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let entry_path = entry.path().to_string_lossy().to_string();
            let metadata = entry
                .metadata()
                .map_err(|e| format!("Metadata error: {e}"))?;
            if metadata.is_dir() {
                let children = build_tree(&entry.path())?;
                nodes.push(FileTreeNode {
                    name,
                    path: entry_path,
                    node_type: "directory".into(),
                    children,
                    modified: None,
                    size: None,
                });
            } else {
                nodes.push(FileTreeNode {
                    name,
                    path: entry_path,
                    node_type: "file".into(),
                    children: Vec::new(),
                    modified: None,
                    size: Some(metadata.len()),
                });
            }
        }
        Ok(nodes)
    }
    build_tree(root)
}

/// Get working directory info including git branch and modified files.
#[tauri::command]
pub async fn get_working_dir_info() -> Result<WorkingDirInfo, String> {
    use std::process::Command;
    let cwd = std::env::current_dir().map_err(|e| format!("Cannot determine CWD: {e}"))?;
    let root = cwd.to_string_lossy().to_string();
    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&cwd)
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(o) } else { None })
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    let modified: Vec<String> = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&cwd)
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(o) } else { None })
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|line| line.get(3..).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let has_conflicts = Command::new("git")
        .args(["diff", "--name-only", "--diff-filter=U"])
        .current_dir(&cwd)
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(o) } else { None })
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    let status = if has_conflicts {
        "merge-conflict".into()
    } else if !modified.is_empty() {
        "dirty".into()
    } else {
        "clean".into()
    };
    Ok(WorkingDirInfo {
        root,
        branch,
        modified_files: modified,
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_diff_round_trips_through_serde() {
        let diff = FileDiff {
            old_content: "old text".to_string(),
            new_content: "new text".to_string(),
            file_name: "test.rs".to_string(),
            language: "rust".to_string(),
        };
        let json = serde_json::to_string(&diff).unwrap();
        let back: FileDiff = serde_json::from_str(&json).unwrap();
        assert_eq!(back.old_content, diff.old_content);
        assert_eq!(back.new_content, diff.new_content);
        assert_eq!(back.file_name, diff.file_name);
        assert_eq!(back.language, diff.language);
    }
}
