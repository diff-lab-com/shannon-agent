//! File-related commands — text save, diff, apply, tree, working-dir info.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).
//! More file commands will move here in future extractions.

use serde::{Deserialize, Serialize};
use std::path::Path;

use base64::Engine;

use crate::commands::AppState;
use crate::commands_agents::resolve_working_dir;
use crate::events::HunkAction;
use crate::resolve_path_in_working_dir;

const MAX_ATTACHMENT_SIZE: u64 = 25 * 1024 * 1024;
const MAX_ATTACHMENT_COUNT: usize = 10;

/// Best-effort PDF text extraction. We intentionally avoid pulling in a
/// heavy PDF crate; the approach is to shell out to `pdftotext` (poppler)
/// if installed, otherwise fall back to the raw UTF-8 decode. This keeps
/// the dependency surface flat while still giving real content for the
/// common case where poppler is available on the user's PATH.
async fn extract_pdf_text_best_effort(path: &Path) -> String {
    use std::process::Command;

    let path_str = path.to_string_lossy().into_owned();
    let output = Command::new("pdftotext").arg(&path_str).arg("-").output();
    if let Ok(out) = output {
        if out.status.success() {
            return String::from_utf8_lossy(&out.stdout).into_owned();
        }
    }

    // Fallback: best-effort UTF-8 decode of the raw bytes.
    tokio::fs::read(path)
        .await
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentPayload {
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub name: String,
    pub size: u64,
}

fn attachment_mime(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "rs" => "text/x-rust",
        "ts" => "text/typescript",
        "tsx" => "text/typescript",
        "js" => "text/javascript",
        "jsx" => "text/javascript",
        "py" => "text/x-python",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Read one attachment for conversion into an assistant message content block.
#[tauri::command]
pub async fn read_attachment(path: String) -> Result<AttachmentPayload, String> {
    let file_path = Path::new(&path);
    let metadata = tokio::fs::metadata(file_path)
        .await
        .map_err(|e| format!("Cannot read attachment metadata: {e}"))?;
    if !metadata.is_file() {
        return Err("Attachment path is not a file".into());
    }
    if metadata.len() > MAX_ATTACHMENT_SIZE {
        return Err(format!(
            "Attachment exceeds the 25 MB limit: {}",
            file_path.display()
        ));
    }
    let bytes = tokio::fs::read(file_path)
        .await
        .map_err(|e| format!("Cannot read attachment: {e}"))?;
    let mime = attachment_mime(file_path);
    let name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&path)
        .to_string();
    let size = bytes.len() as u64;
    if mime.starts_with("image/") {
        Ok(AttachmentPayload {
            mime,
            base64: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            text: None,
            name,
            size,
        })
    } else if mime == "application/pdf" {
        let text = extract_pdf_text_best_effort(file_path).await;
        Ok(AttachmentPayload {
            mime,
            base64: None,
            text: Some(text),
            name,
            size,
        })
    } else {
        let text = String::from_utf8(bytes)
            .map_err(|_| "Attachment is not valid UTF-8 text".to_string())?;
        Ok(AttachmentPayload {
            mime,
            base64: None,
            text: Some(text),
            name,
            size,
        })
    }
}

/// Batch variant used by the UI: read multiple paths in sequence and enforce
/// the per-message `MAX_ATTACHMENT_COUNT` cap before any I/O happens.
#[tauri::command]
pub async fn read_attachments(paths: Vec<String>) -> Result<Vec<AttachmentPayload>, String> {
    if paths.len() > MAX_ATTACHMENT_COUNT {
        return Err(format!(
            "Cannot attach more than {MAX_ATTACHMENT_COUNT} files at once"
        ));
    }
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        out.push(read_attachment(p).await?);
    }
    Ok(out)
}

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
                .args(["show", &format!("HEAD:{path}")])
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
    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read file {file_path}: {e}"))?;

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
            return Err(format!("Hunk {idx} out of bounds for file {file_path}"));
        }

        match hunk.action.as_str() {
            "accept" => {
                // Keep the lines (do nothing)
            }
            "reject" => {
                lines[start_idx..end_idx].fill("");
            }
            _ => {
                return Err(format!("Unknown action {} in hunk {}", hunk.action, idx));
            }
        }
    }

    // Write back the modified content
    let modified_content = lines.join("\n") + "\n";
    let mut file = fs::File::create(&file_path)
        .map_err(|e| format!("Failed to create file {file_path}: {e}"))?;
    file.write_all(modified_content.as_bytes())
        .map_err(|e| format!("Failed to write file {file_path}: {e}"))?;

    Ok(())
}

/// Recursively read a directory and return a file tree.
#[tauri::command]
#[tracing::instrument(fields(path = %path))]
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
    use std::io::Write;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("shannon-attachment-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.push(name);
        dir
    }

    #[tokio::test]
    async fn image_attachment_returns_base64_with_image_mime() {
        let png = b"\x89PNG\r\n\x1a\n".to_vec();
        let path = temp_path("pixel.png");
        std::fs::write(&path, &png).unwrap();

        let payload = read_attachment(path.to_string_lossy().into_owned())
            .await
            .unwrap();
        assert_eq!(payload.mime, "image/png");
        assert_eq!(payload.name, "pixel.png");
        assert_eq!(payload.size, png.len() as u64);
        assert!(payload.text.is_none());
        let b64 = payload.base64.expect("image payload must carry base64");
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(&b64)
                .unwrap(),
            png
        );
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn text_attachment_returns_text_block() {
        let path = temp_path("note.md");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"# Hello\nworld").unwrap();
        let payload = read_attachment(path.to_string_lossy().into_owned())
            .await
            .unwrap();
        assert_eq!(payload.mime, "text/markdown");
        assert_eq!(payload.text.as_deref(), Some("# Hello\nworld"));
        assert!(payload.base64.is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn text_attachment_rejects_non_utf8() {
        let path = temp_path("binary.md");
        std::fs::write(&path, [0xFF, 0xFE, 0xFD, 0xFC]).unwrap();
        let err = read_attachment(path.to_string_lossy().into_owned())
            .await
            .unwrap_err();
        assert!(err.contains("UTF-8"), "got {err}");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn pdf_attachment_returns_text_without_panicking() {
        let path = temp_path("doc.pdf");
        std::fs::write(&path, b"%PDF-1.4 placeholder not a real pdf").unwrap();
        let payload = read_attachment(path.to_string_lossy().into_owned())
            .await
            .unwrap();
        assert_eq!(payload.mime, "application/pdf");
        assert!(payload.base64.is_none());
        assert!(payload.text.is_some());
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn rejects_files_over_25_mb() {
        let path = temp_path("huge.bin");
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(MAX_ATTACHMENT_SIZE + 1).unwrap();
        let err = read_attachment(path.to_string_lossy().into_owned())
            .await
            .unwrap_err();
        assert!(err.contains("25 MB"), "expected limit error, got {err}");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn rejects_more_than_ten_attachments() {
        let paths: Vec<String> = (0..11).map(|i| format!("/nope/{i}")).collect();
        let err = read_attachments(paths).await.unwrap_err();
        assert!(err.contains("10"), "expected count limit, got {err}");
    }

    #[tokio::test]
    async fn rejects_directory() {
        let path = temp_path("");
        std::fs::create_dir_all(&path).unwrap();
        let err = read_attachment(path.to_string_lossy().into_owned())
            .await
            .unwrap_err();
        assert!(err.contains("not a file"));
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn attachment_mime_guess_table() {
        assert_eq!(attachment_mime(Path::new("a.png")), "image/png");
        assert_eq!(attachment_mime(Path::new("a.JPG")), "image/jpeg");
        assert_eq!(attachment_mime(Path::new("a.webp")), "image/webp");
        assert_eq!(attachment_mime(Path::new("a.GIF")), "image/gif");
        assert_eq!(attachment_mime(Path::new("a.pdf")), "application/pdf");
        assert_eq!(attachment_mime(Path::new("a.md")), "text/markdown");
        assert_eq!(attachment_mime(Path::new("a.rs")), "text/x-rust");
        assert_eq!(attachment_mime(Path::new("a.ts")), "text/typescript");
        assert_eq!(attachment_mime(Path::new("a.py")), "text/x-python");
        assert_eq!(attachment_mime(Path::new("a.json")), "application/json");
        assert_eq!(attachment_mime(Path::new("a.toml")), "application/toml");
        assert_eq!(attachment_mime(Path::new("a.yaml")), "application/yaml");
        assert_eq!(attachment_mime(Path::new("a")), "application/octet-stream");
    }

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
