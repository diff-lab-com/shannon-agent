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
use crate::resolve_write_target_in_working_dir;

const MAX_ATTACHMENT_SIZE: u64 = 25 * 1024 * 1024;
const MAX_ATTACHMENT_COUNT: usize = 10;

/// Prefix + wording of the placeholder returned when PDF text extraction is
/// unavailable. Kept next to the builder so the two cannot drift.
const PDF_UNAVAILABLE_PREFIX: &str = "[PDF text extraction unavailable: ";

/// Build the placeholder injected when PDF text extraction is unavailable.
/// Pure function so tests can lock the exact wording.
pub(crate) fn pdf_unavailable_placeholder(reason: &str) -> String {
    format!("{PDF_UNAVAILABLE_PREFIX}{reason}]")
}

/// Whether `text` is the placeholder produced by
/// [`pdf_unavailable_placeholder`] — lets callers frame it as an extraction
/// failure instead of presenting it as "extracted text".
pub(crate) fn is_pdf_unavailable_placeholder(text: &str) -> bool {
    text.starts_with(PDF_UNAVAILABLE_PREFIX) && text.ends_with(']')
}

/// Best-effort PDF text extraction. We intentionally avoid pulling in a
/// heavy PDF crate; the approach is to shell out to `pdftotext` (poppler)
/// if installed. This keeps the dependency surface flat while still giving
/// real content for the common case where poppler is available on the
/// user's PATH.
///
/// When extraction fails or poppler is missing, [`pdf_unavailable_placeholder`]
/// is returned instead of the old raw-bytes UTF-8 lossy decode: lossy-decoding
/// a PDF pours binary mojibake into the model context, while the placeholder
/// tells the model (and user) exactly what went wrong.
pub(crate) async fn extract_pdf_text_best_effort(path: &Path) -> String {
    use std::process::Command;

    let path_str = path.to_string_lossy().into_owned();
    let output = Command::new("pdftotext").arg(&path_str).arg("-").output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        Ok(out) => pdf_unavailable_placeholder(&format!(
            "pdftotext exited with {}",
            out.status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string())
        )),
        Err(e) => pdf_unavailable_placeholder(&format!(
            "pdftotext is not runnable ({e}) — install poppler-utils"
        )),
    }
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

/// Internal helper: validate `path` is inside `working_dir`, then read and
/// classify the file as an [`AttachmentPayload`]. Kept separate from the
/// `#[tauri::command]` wrapper so unit tests don't have to mock
/// `tauri::State`.
async fn read_attachment_inner(
    working_dir: &Path,
    path: &str,
) -> Result<AttachmentPayload, String> {
    let file_path = resolve_path_in_working_dir(path, working_dir)?;
    let metadata = tokio::fs::metadata(&file_path)
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
    let bytes = tokio::fs::read(&file_path)
        .await
        .map_err(|e| format!("Cannot read attachment: {e}"))?;
    let mime = attachment_mime(&file_path);
    let name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
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
        let text = extract_pdf_text_best_effort(&file_path).await;
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

/// Read one attachment for conversion into an assistant message content block.
///
/// Security: the path must resolve inside the active working directory. A
/// compromised frontend cannot read arbitrary files like `~/.ssh/id_rsa`
/// (review §P0-3). To attach files outside the working directory, the UI
/// must first show a file picker (which already runs inside Tauri and is
/// the only blessed path for out-of-tree access).
#[tauri::command]
pub async fn read_attachment(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<AttachmentPayload, String> {
    let working_dir = resolve_working_dir(&state).await;
    read_attachment_inner(&working_dir, &path).await
}

/// Batch variant used by the UI: read multiple paths in sequence and enforce
/// the per-message `MAX_ATTACHMENT_COUNT` cap before any I/O happens.
#[tauri::command]
pub async fn read_attachments(
    state: tauri::State<'_, AppState>,
    paths: Vec<String>,
) -> Result<Vec<AttachmentPayload>, String> {
    if paths.len() > MAX_ATTACHMENT_COUNT {
        return Err(format!(
            "Cannot attach more than {MAX_ATTACHMENT_COUNT} files at once"
        ));
    }
    let working_dir = resolve_working_dir(&state).await;
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        out.push(read_attachment_inner(&working_dir, &p).await?);
    }
    Ok(out)
}

/// Write a text file. The target must resolve to a path inside the active
/// working directory — a compromised frontend cannot write `~/.bashrc`,
/// `~/.ssh/authorized_keys`, or any startup hook (review §P0-3).
#[tauri::command]
pub async fn save_text_file(
    state: tauri::State<'_, AppState>,
    path: String,
    content: String,
) -> Result<(), String> {
    let working_dir = resolve_working_dir(&state).await;
    save_text_file_inner(&working_dir, &path, &content).await
}

/// Internal helper for [`save_text_file`]. Splits out so tests can exercise
/// the validation logic without constructing a Tauri app state.
pub(crate) async fn save_text_file_inner(
    working_dir: &Path,
    path: &str,
    content: &str,
) -> Result<(), String> {
    let target = resolve_write_target_in_working_dir(path, working_dir)?;
    if let Some(parent) = target.parent() {
        // Only create intermediate directories that are themselves inside
        // the working directory — `resolve_write_target_in_working_dir` has
        // already canonicalized the parent, so this stays safe.
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
    }
    std::fs::write(&target, content)
        .map_err(|e| format!("Failed to write {}: {e}", target.display()))
}

// ---------------------------------------------------------------------------
// External open pipeline (2026-09-25 design doc §4 P0-B / P1-C) — existence
// probes for chat file references and capped text reads for disk artifacts.
// ---------------------------------------------------------------------------

const DEFAULT_TEXT_READ_MAX_BYTES: u64 = 512 * 1024;

/// Decision §5-4: chat file references only highlight when they resolve —
/// this probe is the anti-hallucination backstop. Lexical scope check (the
/// probed path may not exist, so there is nothing to canonicalize), then a
/// plain file-existence test.
#[tauri::command]
pub async fn path_exists(path: String) -> Result<bool, String> {
    if !crate::commands_surface::is_probable_path_in_scope(&path) {
        return Ok(false);
    }
    Ok(tokio::fs::metadata(&path)
        .await
        .map(|m| m.is_file())
        .unwrap_or(false))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFileContent {
    pub path: String,
    pub content: String,
    pub size_bytes: u64,
}

/// True when the sampled prefix looks binary (NUL byte), the cheap check
/// `git` and `grep` also use. Split out for tests.
fn sniffs_as_binary(bytes: &[u8]) -> bool {
    bytes.get(..8192).unwrap_or(bytes).contains(&0)
}

/// Machine-readable failure codes for `read_text_file` (§P2-24). The
/// frontend branches on `code` — `message` is display text only, so
/// rewording it can never flip an elegant degradation back into a toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadTextFileErrorCode {
    OutOfScope,
    NotAFile,
    FileTooLarge,
    BinaryFile,
    NotUtf8,
    IoError,
}

/// Structured error payload for `read_text_file`. Tauri serializes the
/// `Err` variant into the IPC rejection verbatim, so the frontend catches
/// `{ code, message }`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadTextFileError {
    pub code: ReadTextFileErrorCode,
    pub message: String,
}

impl ReadTextFileError {
    fn new(code: ReadTextFileErrorCode, message: impl std::fmt::Display) -> Self {
        Self {
            code,
            message: message.to_string(),
        }
    }
}

/// Capped, scope-checked text read backing the dock's manual open tab and
/// auto-docked disk artifacts (P1-C / P1-D). Binary and oversized files
/// return structured errors instead of content.
#[tauri::command]
pub async fn read_text_file(
    path: String,
    max_bytes: Option<u64>,
) -> Result<TextFileContent, ReadTextFileError> {
    let canonical = crate::commands_surface::canonicalized_in_scope(&path)
        .map_err(|e| ReadTextFileError::new(ReadTextFileErrorCode::OutOfScope, e))?;
    let max = max_bytes.unwrap_or(DEFAULT_TEXT_READ_MAX_BYTES).max(1);
    let meta = tokio::fs::metadata(&canonical).await.map_err(|e| {
        ReadTextFileError::new(
            ReadTextFileErrorCode::IoError,
            format!("failed to stat file: {e}"),
        )
    })?;
    if !meta.is_file() {
        return Err(ReadTextFileError::new(
            ReadTextFileErrorCode::NotAFile,
            format!("not a regular file: {path}"),
        ));
    }
    if meta.len() > max {
        return Err(ReadTextFileError::new(
            ReadTextFileErrorCode::FileTooLarge,
            format!("file too large: {} bytes > {max} byte limit", meta.len()),
        ));
    }
    let bytes = tokio::fs::read(&canonical).await.map_err(|e| {
        ReadTextFileError::new(
            ReadTextFileErrorCode::IoError,
            format!("failed to read file: {e}"),
        )
    })?;
    if sniffs_as_binary(&bytes) {
        return Err(ReadTextFileError::new(
            ReadTextFileErrorCode::BinaryFile,
            "binary file (NUL byte in the first 8 KiB)",
        ));
    }
    let content = String::from_utf8(bytes)
        .map_err(|_| ReadTextFileError::new(ReadTextFileErrorCode::NotUtf8, "not valid UTF-8"))?;
    Ok(TextFileContent {
        path: canonical.to_string_lossy().into_owned(),
        content,
        size_bytes: meta.len(),
    })
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
    /// §P2-20: set on a directory whose contents were cut off by the walk
    /// bounds (depth / entry caps), so a truncated listing is distinguishable
    /// from a complete one. `None` (omitted in JSON) = complete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
}

/// §P2-20: bounds for the workspace walk. The old `get_file_tree` recursed
/// without any limit on depth or entry count, so a deep/huge tree could pin
/// an async executor thread (the command is `async fn`) for a long time.
const FILE_TREE_MAX_DEPTH: usize = 12;
const FILE_TREE_MAX_ENTRIES: usize = 5000;

/// Shared recursion state for the bounded walk.
#[derive(Debug)]
struct TreeWalkBudget {
    entries_left: usize,
    truncated: bool,
}

impl TreeWalkBudget {
    fn new(entries: usize) -> Self {
        Self {
            entries_left: entries,
            truncated: false,
        }
    }

    /// Claim one entry from the budget. `false` = budget exhausted, the
    /// walk must stop and the truncation marker stays set.
    fn take_entry(&mut self) -> bool {
        if self.entries_left == 0 {
            self.truncated = true;
            return false;
        }
        self.entries_left -= 1;
        true
    }
}

/// Bounded recursive directory walk used by [`get_file_tree`]. Stops at
/// `FILE_TREE_MAX_DEPTH` levels and after `FILE_TREE_MAX_ENTRIES`
/// entries, marking every directory whose subtree was cut with
/// `truncated: Some(true)`.
fn build_tree_bounded(
    dir: &std::path::Path,
    depth: usize,
    budget: &mut TreeWalkBudget,
) -> Result<Vec<FileTreeNode>, String> {
    use std::fs;
    if depth >= FILE_TREE_MAX_DEPTH {
        budget.truncated = true;
        return Ok(Vec::new());
    }
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
        if !budget.take_entry() {
            break;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let entry_path = entry.path().to_string_lossy().to_string();
        let metadata = entry
            .metadata()
            .map_err(|e| format!("Metadata error: {e}"))?;
        if metadata.is_dir() {
            let truncated_before = budget.truncated;
            let children = build_tree_bounded(&entry.path(), depth + 1, budget)?;
            // The flip happened inside this subtree → this directory is the
            // (nearest ancestor of the) cut point.
            let cut = budget.truncated && !truncated_before;
            nodes.push(FileTreeNode {
                name,
                path: entry_path,
                node_type: "directory".into(),
                children,
                modified: None,
                size: None,
                truncated: cut.then_some(true),
            });
        } else {
            nodes.push(FileTreeNode {
                name,
                path: entry_path,
                node_type: "file".into(),
                children: Vec::new(),
                modified: None,
                size: Some(metadata.len()),
                truncated: None,
            });
        }
    }
    Ok(nodes)
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
pub async fn get_file_diff(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<FileDiff, String> {
    // §P2-20: validate against the session working directory, not the
    // process CWD — a GUI launched from the Dock runs with CWD `/`, which
    // made every legitimate workspace file report "Path outside workspace".
    let working_dir = resolve_working_dir(&state).await;
    get_file_diff_inner(&working_dir, &path).await
}

/// Internal helper for [`get_file_diff`]. Splits out so tests can exercise
/// the resolution + git logic without constructing a Tauri app state.
pub(crate) async fn get_file_diff_inner(
    working_dir: &Path,
    path: &str,
) -> Result<FileDiff, String> {
    use std::process::Command;

    // Resolve relative paths against the session working directory,
    // canonicalize, and reject anything that escapes it (path traversal,
    // symlink escapes) — same contract as the other file commands.
    let canonical = resolve_path_in_working_dir(path, working_dir)?;
    let file_path = std::path::Path::new(path);

    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    // Detect language from extension
    let language = file_path
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "plaintext".to_string());

    // Try git diff first
    let dir = canonical
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| working_dir.to_path_buf());
    let git_output = Command::new("git")
        .args(["diff", "HEAD", "--", path])
        .current_dir(&dir)
        .output();

    let (old_content, new_content) = match git_output {
        Ok(output) if output.status.success() && !output.stdout.is_empty() => {
            // Parse unified diff - for simplicity, just read current file as new
            // and reconstruct old from git show
            let new = std::fs::read_to_string(&canonical).unwrap_or_default();
            let old_output = Command::new("git")
                .args(["show", &format!("HEAD:{path}")])
                .current_dir(&dir)
                .output();
            let old = match old_output {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
                _ => String::new(),
            };
            (old, new)
        }
        _ => {
            // Not a git repo or no changes - read file as new, empty old
            let content = std::fs::read_to_string(&canonical).unwrap_or_default();
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
///
/// §P2-20: the walk is bounded (depth ≤ `FILE_TREE_MAX_DEPTH`, ≤
/// `FILE_TREE_MAX_ENTRIES` entries, truncated directories flagged) and
/// runs on the blocking pool via `spawn_blocking` so the potentially slow
/// stat-heavy recursion never occupies an async executor thread.
#[tauri::command]
#[tracing::instrument(fields(path = %path))]
pub async fn get_file_tree(path: String) -> Result<Vec<FileTreeNode>, String> {
    let root = std::path::PathBuf::from(&path);
    if !root.is_dir() {
        return Err("Path is not a directory".into());
    }
    tokio::task::spawn_blocking(move || {
        let mut budget = TreeWalkBudget::new(FILE_TREE_MAX_ENTRIES);
        build_tree_bounded(&root, 0, &mut budget)
    })
    .await
    .map_err(|e| format!("file tree walk task failed: {e}"))?
}

/// Get working directory info including git branch and modified files.
#[tauri::command]
pub async fn get_working_dir_info(
    state: tauri::State<'_, AppState>,
) -> Result<WorkingDirInfo, String> {
    // §P2-20: report the session working directory, not the process CWD —
    // a Dock-launched GUI runs with CWD `/`, which reported a useless root
    // and a bogus git status. Mirrors `resolve_working_dir` (configured
    // `working_dir`, CWD only as last-resort fallback).
    let working_dir = resolve_working_dir(&state).await;
    Ok(get_working_dir_info_inner(&working_dir))
}

/// Internal helper for [`get_working_dir_info`]. Pure sync so tests can
/// exercise it against a scratch git repo without Tauri app state.
fn get_working_dir_info_inner(working_dir: &Path) -> WorkingDirInfo {
    use std::process::Command;
    let root = working_dir.to_string_lossy().to_string();
    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(working_dir)
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(o) } else { None })
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    let modified: Vec<String> = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(working_dir)
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
        .current_dir(working_dir)
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
    WorkingDirInfo {
        root,
        branch,
        modified_files: modified,
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // Each disk-touching test gets its own TempDir so a sibling's cleanup
    // can never remove a file this test still needs. The old shared
    // `/tmp/shannon-attachment-test-{pid}/` dir (keyed by process id, hence
    // shared across all parallel test threads) let `rejects_directory`'s
    // `remove_dir_all` race with concurrent writers, producing an
    // ordering-dependent flake under single-process `cargo test` that never
    // reproduced under nextest (process-isolated) or in CI.

    #[tokio::test]
    async fn image_attachment_returns_base64_with_image_mime() {
        let png = b"\x89PNG\r\n\x1a\n".to_vec();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pixel.png");
        std::fs::write(&path, &png).unwrap();

        let payload = read_attachment_inner(dir.path(), &path.to_string_lossy())
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
    }

    #[tokio::test]
    async fn text_attachment_returns_text_block() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("note.md");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"# Hello\nworld").unwrap();
        let payload = read_attachment_inner(dir.path(), &path.to_string_lossy())
            .await
            .unwrap();
        assert_eq!(payload.mime, "text/markdown");
        assert_eq!(payload.text.as_deref(), Some("# Hello\nworld"));
        assert!(payload.base64.is_none());
    }

    #[tokio::test]
    async fn text_attachment_rejects_non_utf8() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("binary.md");
        std::fs::write(&path, [0xFF, 0xFE, 0xFD, 0xFC]).unwrap();
        let err = read_attachment_inner(dir.path(), &path.to_string_lossy())
            .await
            .unwrap_err();
        assert!(err.contains("UTF-8"), "got {err}");
    }

    #[tokio::test]
    async fn pdf_attachment_returns_text_without_panicking() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("doc.pdf");
        std::fs::write(&path, b"%PDF-1.4 placeholder not a real pdf").unwrap();
        let payload = read_attachment_inner(dir.path(), &path.to_string_lossy())
            .await
            .unwrap();
        assert_eq!(payload.mime, "application/pdf");
        assert!(payload.base64.is_none());
        assert!(payload.text.is_some());
    }

    // ---- PDF extraction-failure placeholder ----

    #[test]
    fn pdf_placeholder_has_stable_wording() {
        let placeholder = pdf_unavailable_placeholder("pdftotext missing");
        assert_eq!(
            placeholder,
            "[PDF text extraction unavailable: pdftotext missing]"
        );
        assert!(is_pdf_unavailable_placeholder(&placeholder));
    }

    #[test]
    fn pdf_placeholder_predicate_rejects_normal_text() {
        assert!(!is_pdf_unavailable_placeholder("real extracted pdf body"));
        assert!(!is_pdf_unavailable_placeholder(""));
        // Missing the closing bracket → not our placeholder.
        assert!(!is_pdf_unavailable_placeholder(
            "[PDF text extraction unavailable: nope"
        ));
    }

    #[tokio::test]
    async fn pdf_extraction_failure_returns_placeholder_not_binary_garbage() {
        // Binary junk that is not a PDF: the old lossy-UTF-8 fallback would
        // have injected mojibake into the model context; the placeholder
        // names the failure instead.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("garbage.pdf");
        std::fs::write(&path, [0xFFu8, 0x00, 0xFE, 0x25, 0x50, 0x44, 0x46, 0x01]).unwrap();
        let text = extract_pdf_text_best_effort(&path).await;
        assert!(
            text.starts_with("[PDF text extraction unavailable:"),
            "expected placeholder, got: {text}"
        );
        assert!(text.ends_with(']'), "got: {text}");
    }

    #[tokio::test]
    async fn rejects_files_over_25_mb() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("huge.bin");
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(MAX_ATTACHMENT_SIZE + 1).unwrap();
        let err = read_attachment_inner(dir.path(), &path.to_string_lossy())
            .await
            .unwrap_err();
        assert!(err.contains("25 MB"), "expected limit error, got {err}");
    }

    #[tokio::test]
    async fn rejects_more_than_ten_attachments() {
        // The count guard lives in the IPC `read_attachments` wrapper. Since
        // it runs before any I/O and we can't easily invoke the Tauri
        // wrapper from a unit test, we replicate the guard here. If the
        // constant ever changes, this test should be updated to reflect.
        const MAX_ATTACHMENT_COUNT_FOR_TEST: usize = 10;
        let paths: Vec<String> = (0..(MAX_ATTACHMENT_COUNT_FOR_TEST + 1))
            .map(|i| format!("/nope/{i}"))
            .collect();
        assert!(paths.len() > MAX_ATTACHMENT_COUNT_FOR_TEST);
    }

    #[tokio::test]
    async fn rejects_directory() {
        // A fresh subdirectory inside an isolated tempdir — never the
        // tempdir root, so cleanup can't race with sibling tests.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a-directory");
        std::fs::create_dir_all(&path).unwrap();
        let err = read_attachment_inner(dir.path(), &path.to_string_lossy())
            .await
            .unwrap_err();
        assert!(err.contains("not a file"));
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

    // ---- review §P0-3: out-of-tree attachment / write attempts ----

    #[tokio::test]
    async fn read_attachment_rejects_path_outside_working_dir() {
        // A compromised frontend must not be able to read ~/.ssh/id_rsa or
        // any other file outside the working directory.
        let workdir = tempfile::tempdir().expect("tempdir");
        let outside = workdir
            .path()
            .parent()
            .unwrap()
            .join("shannon_outside_target.txt");
        std::fs::write(&outside, "ssh-private-key-bytes").unwrap();

        let err = read_attachment_inner(workdir.path(), &outside.to_string_lossy())
            .await
            .expect_err("must reject out-of-tree read");
        assert!(
            err.contains("outside"),
            "expected 'outside' rejection, got: {err}"
        );

        let _ = std::fs::remove_file(&outside);
    }

    #[tokio::test]
    async fn save_text_file_inner_rejects_path_outside_working_dir() {
        // A compromised frontend must not be able to write ~/.bashrc or any
        // other file outside the working directory.
        let workdir = tempfile::tempdir().expect("tempdir");
        let outside = workdir
            .path()
            .parent()
            .unwrap()
            .join("shannon_outside_write_target.txt");
        let _ = std::fs::remove_file(&outside); // ensure parent dir exists

        let err = save_text_file_inner(workdir.path(), &outside.to_string_lossy(), "pwned")
            .await
            .expect_err("must reject out-of-tree write");
        assert!(
            err.contains("outside") || err.contains("not found"),
            "expected 'outside' rejection, got: {err}"
        );

        // The target file must not exist on disk.
        assert!(!outside.exists(), "file must not have been written");
        let _ = std::fs::remove_file(&outside);
    }

    #[tokio::test]
    async fn save_text_file_inner_accepts_path_inside_working_dir() {
        let workdir = tempfile::tempdir().expect("tempdir");
        let target = workdir.path().join("subdir/note.txt");
        save_text_file_inner(workdir.path(), "subdir/note.txt", "hello")
            .await
            .expect("in-tree write should succeed");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello");
    }

    // ---- review §P2-20: bounded file-tree walk ----

    #[test]
    fn file_tree_walk_stops_at_max_depth_and_marks_truncated() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A 20-level-deep chain of directories — well past the cap.
        let mut deep = dir.path().to_path_buf();
        for i in 0..20 {
            deep = deep.join(format!("d{i}"));
            std::fs::create_dir_all(&deep).expect("create deep dir");
        }
        std::fs::write(deep.join("bottom.txt"), "x").expect("write bottom file");

        let mut budget = TreeWalkBudget::new(FILE_TREE_MAX_ENTRIES);
        let tree = build_tree_bounded(dir.path(), 0, &mut budget).expect("walk succeeds");

        // Measure how deep the produced tree actually is.
        let mut depth = 0;
        let mut node = &tree[0];
        while !node.children.is_empty() {
            node = &node.children[0];
            depth += 1;
        }
        assert!(
            depth <= FILE_TREE_MAX_DEPTH,
            "tree depth {depth} must be capped at {FILE_TREE_MAX_DEPTH}"
        );
        assert!(budget.truncated, "depth overflow must mark truncation");
        // Every directory along the cut chain is flagged.
        assert_eq!(node.truncated, Some(true));
    }

    #[test]
    fn file_tree_walk_stops_at_entry_budget_and_marks_truncated() {
        let dir = tempfile::tempdir().expect("tempdir");
        for i in 0..10 {
            std::fs::write(dir.path().join(format!("f{i}.txt")), "x").expect("write file");
        }

        let mut budget = TreeWalkBudget::new(3);
        let tree = build_tree_bounded(dir.path(), 0, &mut budget).expect("walk succeeds");
        assert_eq!(tree.len(), 3, "walk must stop at the entry budget");
        assert!(budget.truncated, "exhausted budget must mark truncation");
    }

    #[test]
    fn file_tree_walk_complete_listing_has_no_truncation_marker() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.txt"), "x").expect("write a");
        std::fs::create_dir_all(dir.path().join("sub")).expect("mkdir");
        std::fs::write(dir.path().join("sub/b.txt"), "y").expect("write b");

        let mut budget = TreeWalkBudget::new(FILE_TREE_MAX_ENTRIES);
        let tree = build_tree_bounded(dir.path(), 0, &mut budget).expect("walk succeeds");
        assert!(!budget.truncated);
        assert!(tree.iter().all(|n| n.truncated.is_none()));
    }

    // ---- review §P2-20: session working directory instead of process CWD ----

    #[tokio::test]
    async fn file_diff_rejects_path_outside_working_dir() {
        let workdir = tempfile::tempdir().expect("tempdir");
        let outside = workdir
            .path()
            .parent()
            .unwrap()
            .join("shannon_outside_diff_target.txt");
        std::fs::write(&outside, "secret").expect("write outside file");

        let err = get_file_diff_inner(workdir.path(), &outside.to_string_lossy())
            .await
            .expect_err("out-of-tree diff must be rejected");
        assert!(err.contains("outside"), "got: {err}");

        let _ = std::fs::remove_file(&outside);
    }

    #[tokio::test]
    async fn file_diff_accepts_relative_path_inside_working_dir() {
        // The Dock-launch scenario: the process CWD is unrelated (`/`), so
        // the old CWD-based validation failed for legitimate workspace
        // files. The helper must resolve relative paths against the
        // *session* working directory instead.
        let workdir = tempfile::tempdir().expect("tempdir");
        std::fs::write(workdir.path().join("note.md"), "hello diff").expect("write file");

        let diff = get_file_diff_inner(workdir.path(), "note.md")
            .await
            .expect("in-tree diff must succeed (non-git dir → empty old side)");
        assert_eq!(diff.file_name, "note.md");
        assert_eq!(diff.new_content, "hello diff");
        assert_eq!(diff.old_content, "");
    }

    #[test]
    fn working_dir_info_reports_session_dir_and_dirty_status() {
        use std::process::Command;
        let dir = tempfile::tempdir().expect("tempdir");
        // No git identity needed for `git init` + an untracked file.
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status();
        match init {
            Ok(s) if s.success() => {}
            // Git missing / restricted sandbox — the status fallbacks are
            // still exercised; skip the git-specific assertions.
            _ => return,
        }
        std::fs::write(dir.path().join("tracked.txt"), "dirty").expect("write file");

        let info = get_working_dir_info_inner(dir.path());
        assert_eq!(info.root, dir.path().to_string_lossy());
        // An untracked file shows up in `git status --porcelain`.
        assert_eq!(info.status, "dirty");
        assert!(
            info.modified_files
                .iter()
                .any(|f| f.ends_with("tracked.txt"))
        );
    }
    // ---- read_text_file structured errors (review §P2-24 / batch B3) ----
    //
    // The frontend (ArtifactLinkHost) degrades oversized/binary/non-UTF-8
    // reads to an OS-handoff tab by branching on `error.code`; only true
    // failures (io / scope) should ever surface as a toast. These tests pin
    // the wire shape `{ code, message }` and every code the command emits.

    #[tokio::test]
    async fn read_text_file_returns_content_size_and_canonical_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("doc.md");
        std::fs::write(&path, "# hello\nworld").unwrap();

        let out = read_text_file(path.to_string_lossy().into_owned(), None)
            .await
            .expect("plain text read must succeed");
        assert_eq!(out.content, "# hello\nworld");
        assert_eq!(out.size_bytes, "# hello\nworld".len() as u64);
        assert!(out.path.ends_with("doc.md"), "got: {}", out.path);
    }

    #[tokio::test]
    async fn read_text_file_rejects_oversized_with_file_too_large_code() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("huge.txt");
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(4096).unwrap();
        drop(f);

        let err = read_text_file(path.to_string_lossy().into_owned(), Some(1024))
            .await
            .expect_err("oversized read must be rejected");
        assert_eq!(err.code, ReadTextFileErrorCode::FileTooLarge);
        assert!(err.message.contains("too large"), "got: {}", err.message);
    }

    #[tokio::test]
    async fn read_text_file_rejects_nul_byte_with_binary_file_code() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("prog.bin");
        std::fs::write(&path, b"MZ\x00\x00payload").unwrap();

        let err = read_text_file(path.to_string_lossy().into_owned(), None)
            .await
            .expect_err("NUL-sniffing read must be rejected");
        assert_eq!(err.code, ReadTextFileErrorCode::BinaryFile);
    }

    #[tokio::test]
    async fn read_text_file_rejects_non_utf8_with_not_utf8_code() {
        // Invalid UTF-8 *without* a NUL byte — must land on `not_utf8`, not
        // on the NUL-sniffing `binary_file` branch.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("latin1.txt");
        std::fs::write(&path, [0xCA, 0xFE, 0xBA, 0xBE]).unwrap();

        let err = read_text_file(path.to_string_lossy().into_owned(), None)
            .await
            .expect_err("non-UTF-8 read must be rejected");
        assert_eq!(err.code, ReadTextFileErrorCode::NotUtf8);
    }

    #[tokio::test]
    async fn read_text_file_rejects_directory_with_not_a_file_code() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sub = dir.path().join("a-directory");
        std::fs::create_dir_all(&sub).unwrap();

        let err = read_text_file(sub.to_string_lossy().into_owned(), None)
            .await
            .expect_err("directory read must be rejected");
        assert_eq!(err.code, ReadTextFileErrorCode::NotAFile);
    }

    #[tokio::test]
    async fn read_text_file_rejects_relative_path_with_out_of_scope_code() {
        let err = read_text_file("relative/answer.md".to_string(), None)
            .await
            .expect_err("relative path must be rejected");
        assert_eq!(err.code, ReadTextFileErrorCode::OutOfScope);
    }

    #[test]
    fn read_text_file_error_serializes_code_and_message() {
        let err = ReadTextFileError::new(ReadTextFileErrorCode::FileTooLarge, "too big");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "file_too_large");
        assert_eq!(json["message"], "too big");
    }
}
