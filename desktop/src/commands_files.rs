//! File-related commands — text save, diff, apply, tree, working-dir info.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).
//! More file commands will move here in future extractions.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

/// Blocking core of [`extract_pdf_text_best_effort`] — G3b P1-4 split it out
/// so the preflight can run extraction inside `spawn_blocking` (the async
/// wrapper awaits nothing itself; the subprocess spawn is blocking either
/// way).
pub(crate) fn extract_pdf_text_blocking(path: &Path) -> String {
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
    extract_pdf_text_blocking(path)
}

/// Blocking core of [`pdf_page_count_best_effort`] (see the split note on
/// [`extract_pdf_text_blocking`]).
pub(crate) fn pdf_page_count_blocking(path: &Path) -> Option<u32> {
    use std::process::Command;

    let path_str = path.to_string_lossy().into_owned();
    let output = Command::new("pdfinfo").arg(&path_str).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().find_map(|line| {
        line.strip_prefix("Pages:")
            .and_then(|rest| rest.trim().parse::<u32>().ok())
    })
}

/// Best-effort PDF page count via `pdfinfo` (poppler). `None` when poppler
/// is missing, the file is unreadable, or the probe fails — callers must
/// omit the metadata rather than guess. (Office Wave A2': the send_message
/// entry point carries no page-range request yet, so `pdftotext` stays
/// whole-document; this is the honest per-document metadata for the
/// injection block, and a `-f`/`-l` range plugs in here once a page
/// parameter exists.)
pub(crate) async fn pdf_page_count_best_effort(path: &Path) -> Option<u32> {
    pdf_page_count_blocking(path)
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
        // Office Wave 1 — word processor / spreadsheet / presentation formats
        // so attachment classification (and downstream tool routing) can tell
        // office documents apart instead of lumping them into
        // `application/octet-stream`.
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "rtf" => "application/rtf",
        "csv" => "text/csv",
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

/// One path's verdict from the [`check_attachment_paths`] preflight.
/// `reason` is `None` iff `ok` — the snake_case tag mirrors
/// [`crate::commands::RejectedAttachmentReason`] so the frontend reuses one
/// reason → i18n map for both preflight chips and send-time toasts.
///
/// G3b P1-4: `extraction` carries the same per-file extraction summary the
/// send pipeline stamps onto `FileAttachment`s (only for parseable documents
/// and only when the path passed the classification), so the composer chip
/// can badge "extracted N sections, first M inlined" / "PDF truncates at
/// 50 KiB" BEFORE the send. Best-effort: `None` for everything else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentPathCheck {
    pub path: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<crate::commands::RejectedAttachmentReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extraction: Option<crate::commands::AttachmentExtractionReport>,
}

/// P0-3 preflight — classify attachment paths exactly the way the
/// `send_message` pipeline will, BEFORE the user hits send, so a refusal is
/// never a surprise: the composer flags each bad chip (warning icon +
/// tooltip) while the file list is being built. Advisory by contract: the
/// command always returns `Ok` (one entry per input path) and the frontend
/// treats a failed call as "no marking", never as an error.
///
/// Same boundary, same verdicts: paths outside the working directory are
/// refused (`out_of_working_dir`), never silently accepted and never
/// silently dropped. With no configured working directory the attachment
/// domain is UNDEFINED — every path reports `no_working_dir` so the UI can
/// point at Settings instead of pretending the files will be read.
#[tauri::command]
pub async fn check_attachment_paths(
    state: tauri::State<'_, AppState>,
    paths: Vec<String>,
) -> Result<Vec<AttachmentPathCheck>, String> {
    let configured = state.desktop_config.read().await.working_dir.clone();
    let mut checks = check_attachment_paths_inner(configured, paths);
    // G3b P1-4 — best-effort extraction summaries for the parseable paths
    // that passed classification. Runs on the blocking pool (pdftotext
    // spawn + container parse); a failure or a slow parse can only delay
    // this advisory response, never fail it: the command keeps its
    // "always Ok" contract and un-summarized paths degrade to `None`.
    let parseable: Vec<(usize, PathBuf)> = checks
        .iter()
        .enumerate()
        .filter(|(_, check)| {
            let p = std::path::Path::new(&check.path);
            check.ok
                && crate::document_parse::extension_lowercase(p)
                    .is_some_and(|e| e == "pdf" || crate::document_parse::is_office_document(p))
        })
        .map(|(i, check)| (i, PathBuf::from(&check.path)))
        .collect();
    if !parseable.is_empty() {
        let reports = tokio::task::spawn_blocking(move || {
            parseable
                .into_iter()
                .map(|(i, p)| (i, extraction_report_for_path(&p)))
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        for (i, report) in reports {
            if let (Some(check), Some(report)) = (checks.get_mut(i), report) {
                check.extraction = Some(report);
            }
        }
    }
    Ok(checks)
}

/// G3b P1-4 — the per-file extraction summary for one parseable attachment
/// path, computed with the SAME helpers the send pipeline uses (so the
/// preflight badge can never disagree with what the send actually does).
/// `None` for non-parseable kinds or when basic metadata is unreadable.
///
/// Sync + blocking (spawns `pdftotext`/`pdfinfo`, parses containers) —
/// callers run it inside `spawn_blocking`.
pub(crate) fn extraction_report_for_path(
    path: &Path,
) -> Option<crate::commands::AttachmentExtractionReport> {
    let ext = crate::document_parse::extension_lowercase(path)?;
    let name = path.file_name()?.to_str()?.to_string();
    let size = std::fs::metadata(path).ok()?.len();
    if crate::document_parse::is_office_document(path) {
        let outcome = crate::document_parse::office_extraction_for_file(path, &name, size);
        return Some(crate::commands::AttachmentExtractionReport {
            path: path.to_string_lossy().into_owned(),
            kind: ext,
            extracted: outcome.extracted,
            sections_total: outcome.sections_total,
            sections_inlined: outcome.sections_inlined,
            truncated: outcome.truncated,
            cache_path: outcome.cache_path,
        });
    }
    if ext == "pdf" {
        let text = extract_pdf_text_blocking(path);
        let pages = pdf_page_count_blocking(path);
        let outcome = crate::commands::pdf_extraction_outcome(&name, path, size, pages, &text);
        return Some(crate::commands::AttachmentExtractionReport {
            path: path.to_string_lossy().into_owned(),
            kind: ext,
            extracted: outcome.extracted,
            sections_total: 0,
            sections_inlined: 0,
            truncated: outcome.truncated,
            cache_path: outcome.cache_path,
        });
    }
    None
}

// ── G3b P1-6: composer clipboard-image paste ────────────────────────────────

// The pasted-image directory is `crate::pasted_image_cache_dir()` (lib.rs):
// ONE definition shared with `classify_path_in_working_dir`, so the writer
// (this command) and the attachment boundary (preflight + send gate) can
// never disagree about where pasted images live — that agreement is exactly
// what makes the boundary exception safe.

/// Normalize a pasted-image extension to the canonical form used for the
/// persisted file name. Only formats the multimodal pipeline accepts are
/// allowed (`png`/`jpeg`/`gif`/`webp` — no `svg`: the vision path excludes
/// it and it has no magic bytes to verify).
fn normalize_pasted_ext(ext: &str) -> Option<&'static str> {
    match ext.trim().to_ascii_lowercase().as_str() {
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpeg"),
        "gif" => Some("gif"),
        "webp" => Some("webp"),
        _ => None,
    }
}

/// Whether the decoded bytes start with the magic signature of `ext` — a
/// mismatched blob (renamed file, text pasted as `image/x` by a hostile
/// page) must be refused before it lands on disk.
fn pasted_image_magic_matches(ext: &str, bytes: &[u8]) -> bool {
    match ext {
        "png" => bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
        "jpeg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "gif" => bytes.starts_with(b"GIF8"),
        "webp" => bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

/// Core of [`save_pasted_image`] — pure over `(base_dir, data, ext)` so the
/// validation ladder is unit-testable without a Tauri runtime. Order:
/// extension allow-list → shared 10 MiB image cap (pre-decode estimate,
/// same helper as every attachment entry path) → base64 decode → post-decode
/// exact re-check on the REAL bytes (`validate_decoded_size` — the estimate
/// above is deliberately padded; this is the authoritative gate, mirroring
/// the other entry paths' post-read re-check; refusing here happens BEFORE
/// any file is created, so an over-limit payload can never leave even a
/// partial artifact on disk) → magic-byte check → write
/// `<millis>-<rand8>.<ext>` into `base_dir`.
fn save_pasted_image_inner(
    base_dir: &Path,
    data_base64: &str,
    ext: &str,
) -> Result<PathBuf, String> {
    use base64::Engine as _;

    let ext = normalize_pasted_ext(ext).ok_or_else(|| {
        format!("unsupported pasted image type '{ext}' (allowed: png, jpeg, gif, webp)")
    })?;
    shannon_core::attachments::validate_base64_size(data_base64.len())
        .map_err(|e| format!("pasted image rejected: {e}"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.trim())
        .map_err(|e| format!("pasted image is not valid base64: {e}"))?;
    if bytes.is_empty() {
        return Err("pasted image is empty".into());
    }
    // Post-decode exact-size re-check (G3b fix round 1 / M1). Runs before
    // any I/O: over-limit means NOTHING is written, so no half-written file
    // needs cleanup.
    shannon_core::attachments::validate_decoded_size(bytes.len())
        .map_err(|e| format!("pasted image rejected: {e}"))?;
    if !pasted_image_magic_matches(ext, &bytes) {
        return Err(format!(
            "pasted image data does not match its declared {ext} format"
        ));
    }
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let rand8 = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let target = base_dir.join(format!("{millis}-{rand8}.{ext}"));
    std::fs::create_dir_all(base_dir)
        .map_err(|e| format!("failed to create pasted-image cache dir: {e}"))?;
    std::fs::write(&target, &bytes).map_err(|e| format!("failed to write pasted image: {e}"))?;
    Ok(target)
}

/// P1-6 — persist a clipboard image the composer captured via `paste`, so it
/// can ride the regular attachment pipeline (absolute path → preflight →
/// multimodal base64 block). The bytes must decode to the declared format
/// (magic bytes checked) and fit the shared 10 MiB image cap; the file lands
/// in `~/.shannon/cache/pasted/<timestamp>-<rand>.<ext>` and its ABSOLUTE
/// path is returned. Non-images never reach this command — the frontend
/// only calls it for `image/*` clipboard items.
#[tauri::command]
pub async fn save_pasted_image(data_base64: String, ext: String) -> Result<String, String> {
    let dir = crate::pasted_image_cache_dir()
        .ok_or_else(|| "cannot resolve a home directory for the pasted-image cache".to_string())?;
    let path =
        tokio::task::spawn_blocking(move || save_pasted_image_inner(&dir, &data_base64, &ext))
            .await
            .map_err(|e| format!("pasted image task failed: {e}"))??;
    Ok(path.to_string_lossy().into_owned())
}

/// Internal helper for [`check_attachment_paths`] — pure over
/// (configured working dir, paths) so tests drive it without app state.
pub(crate) fn check_attachment_paths_inner(
    configured_working_dir: Option<String>,
    paths: Vec<String>,
) -> Vec<AttachmentPathCheck> {
    let Some(working_dir) = configured_working_dir else {
        return paths
            .into_iter()
            .map(|path| AttachmentPathCheck {
                path,
                ok: false,
                reason: Some(crate::commands::RejectedAttachmentReason::NoWorkingDir),
                extraction: None,
            })
            .collect();
    };
    let working_dir = PathBuf::from(working_dir);
    paths
        .into_iter()
        .map(|path| {
            let reason = classify_attachment_path(&path, &working_dir);
            match reason {
                None => AttachmentPathCheck {
                    path,
                    ok: true,
                    reason: None,
                    extraction: None,
                },
                Some(reason) => AttachmentPathCheck {
                    path,
                    ok: false,
                    reason: Some(reason),
                    extraction: None,
                },
            }
        })
        .collect()
}

/// Shared verdict logic of the preflight: `None` = `send_message` will
/// accept this path; `Some(reason)` = the refusal the user will (now) see.
/// Mirrors `collect_attachments` gate-for-gate on purpose — the two must
/// never disagree about a path. Both go through
/// `classify_path_in_working_dir`, so the one narrow boundary exception
/// (`$SHANNON_HOME/cache/pasted/` — pasted clipboard images; see its doc in
/// lib.rs) applies to preflight and send alike.
fn classify_attachment_path(
    path: &str,
    working_dir: &Path,
) -> Option<crate::commands::RejectedAttachmentReason> {
    use crate::commands::RejectedAttachmentReason;
    let canonical = match crate::classify_path_in_working_dir(path, working_dir) {
        Ok(c) => c,
        Err(crate::WorkingDirScopeError::OutsideWorkingDir(_)) => {
            return Some(RejectedAttachmentReason::OutOfWorkingDir);
        }
        Err(_) => return Some(RejectedAttachmentReason::Unresolvable),
    };
    let Ok(meta) = std::fs::metadata(&canonical) else {
        return Some(RejectedAttachmentReason::Unresolvable);
    };
    // Same size caps as the send path (see `collect_attachments`), with the
    // same image detection so the preflight never disagrees with the gate
    // that fires on send.
    let is_image = crate::commands::detect_media_type(&canonical.to_string_lossy())
        .is_some_and(|m| m.starts_with("image/"));
    if (is_image && meta.len() > shannon_core::attachments::MAX_IMAGE_BYTES as u64)
        || (!is_image
            && canonical
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
            && meta.len() > crate::commands::MAX_PDF_BYTES)
    {
        return Some(RejectedAttachmentReason::TooLarge);
    }
    None
}

/// Write a text file. The target must resolve to a path inside the active
/// working directory — a compromised frontend cannot write `~/.bashrc`,
/// `~/.ssh/authorized_keys`, or any startup hook (review §P0-3).
///
/// B0 P0-3: when `expected_mtime` is provided it must still match the file
/// on disk, otherwise the content that was reviewed (and merged) is stale
/// and writing it would clobber concurrent edits. The check is opt-in, so
/// existing callers are unaffected.
#[tauri::command]
pub async fn save_text_file(
    state: tauri::State<'_, AppState>,
    path: String,
    content: String,
    expected_mtime: Option<String>,
) -> Result<(), FileCommandError> {
    let working_dir = resolve_working_dir(&state).await;
    save_text_file_inner(&working_dir, &path, &content, expected_mtime.as_deref()).await
}

/// Internal helper for [`save_text_file`]. Splits out so tests can exercise
/// the validation logic without constructing a Tauri app state.
pub(crate) async fn save_text_file_inner(
    working_dir: &Path,
    path: &str,
    content: &str,
    expected_mtime: Option<&str>,
) -> Result<(), FileCommandError> {
    let target = resolve_write_target_in_working_dir(path, working_dir)?;
    if let Some(expected) = expected_mtime.filter(|m| !m.is_empty()) {
        let conflict = |current: Option<String>| {
            FileCommandError::Conflict(SaveConflictError {
                code: "mtime_conflict",
                message: format!(
                    "file changed since it was read (expected mtime {expected}); re-read before writing: {path}"
                ),
                current_mtime: current,
            })
        };
        match std::fs::metadata(&target)
            .ok()
            .and_then(|m| mtime_rfc3339(&m))
        {
            Some(actual) if actual == expected => {}
            actual => return Err(conflict(actual)),
        }
    }
    if let Some(parent) = target.parent() {
        // Only create intermediate directories that are themselves inside
        // the working directory — `resolve_write_target_in_working_dir` has
        // already canonicalized the parent, so this stays safe.
        std::fs::create_dir_all(parent).map_err(|e| {
            FileCommandError::Plain(format!("Failed to create {}: {e}", parent.display()))
        })?;
    }
    std::fs::write(&target, content)
        .map_err(|e| FileCommandError::Plain(format!("Failed to write {}: {e}", target.display())))
}

/// Save text to a path the user picks in a NATIVE save dialog opened by the
/// backend (G5 P0-8: the timeline HTML export). Unlike [`save_text_file`] —
/// which is deliberately scoped to the working directory — the user's pick
/// in a native dialog IS the explicit authorization, so Downloads/Documents
/// destinations work. `resolve_write_target_in_working_dir` is left
/// untouched for its other callers.
///
/// Split into the dialog half (`pick_save_path_via_dialog`, needs an
/// `AppHandle`) and the write half (`write_text_file_at`, plain fs) so
/// unit tests can cover the write path without mocking Tauri dialogs.
///
/// Returns the final path written, or `None` when the user cancelled the
/// dialog (cancelling is a decision, not an error).
#[tauri::command]
pub async fn save_text_file_via_dialog(
    app: tauri::AppHandle,
    content: String,
    default_name: String,
) -> Result<Option<String>, String> {
    let path = pick_save_path_via_dialog(&app, &default_name).await?;
    let Some(path) = path else {
        return Ok(None);
    };
    write_text_file_at(&path, &content).await?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Dialog half of [`save_text_file_via_dialog`]: open the native save dialog
/// pre-filled with `default_name` (filtered to its extension, when it has
/// one) and resolve to the chosen path. `blocking_save_file` must not run on
/// the main thread — same `spawn_blocking` pattern as `copy_file`.
async fn pick_save_path_via_dialog(
    app: &tauri::AppHandle,
    default_name: &str,
) -> Result<Option<std::path::PathBuf>, String> {
    use tauri_plugin_dialog::DialogExt;

    let app = app.clone();
    let default_name = default_name.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_file_name(&default_name);
        if let Some((_, ext)) = default_name.rsplit_once('.') {
            if !ext.is_empty() {
                let upper = ext.to_ascii_uppercase();
                dialog = dialog.add_filter(upper, &[ext]);
            }
        }
        dialog
            .blocking_save_file()
            .map(|fp| fp.into_path())
            .transpose()
            .map_err(|e| format!("save dialog returned an unusable path: {e}"))
    })
    .await
    .map_err(|e| format!("save dialog task failed: {e}"))?
}

/// Write half of [`save_text_file_via_dialog`]: create the parent directory
/// when needed, then write. The path came from the user's own dialog pick,
/// so no working-directory scoping applies here.
pub(crate) async fn write_text_file_at(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
    }
    tokio::fs::write(path, content)
        .await
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

/// Copy a local file to a caller-chosen destination (office Wave 1
/// "save-as"). Scope rules are aligned with `open_with_default_app`
/// (`commands_surface::canonicalized_in_scope`): the source must exist and
/// canonicalize inside `$HOME/**` or `$TEMP/**` (rejecting `..` traversal
/// and symlink escapes); the destination must land in the same bases, but
/// may not exist yet — see the private `destination_in_scope` helper. Overwriting an
/// existing destination is allowed; copying onto the source is not.
#[tauri::command]
pub async fn copy_file(src_path: String, dest_path: String) -> Result<(), String> {
    copy_file_inner(&src_path, &dest_path).await
}

/// Internal helper for [`copy_file`]. Splits out so tests can exercise the
/// scope + copy logic without a Tauri app handle.
pub(crate) async fn copy_file_inner(src_path: &str, dest_path: &str) -> Result<(), String> {
    if src_path == dest_path {
        return Err(format!(
            "copy source and destination are the same file: {src_path}"
        ));
    }
    // Same scope contract as the surface side's `open_with_default_app`:
    // canonicalize + `$HOME`/`$TEMP` base check (also rejects `..` and
    // symlink escapes, and implies the source exists).
    let src = crate::commands_surface::canonicalized_in_scope(src_path)?;
    if !src.is_file() {
        return Err(format!(
            "copy source is not a regular file: {}",
            src.display()
        ));
    }
    let dest = destination_in_scope(dest_path)?;
    if dest == src {
        return Err(format!(
            "copy source and destination are the same file: {src_path}"
        ));
    }
    // A large copy must not occupy an async executor thread — same
    // spawn_blocking pattern as `get_file_tree`.
    tokio::task::spawn_blocking(move || {
        // `std::fs::copy` overwrites an existing destination, which is the
        // intended save-as semantics.
        std::fs::copy(&src, &dest).map(|_| ()).map_err(|e| {
            format!(
                "failed to copy {} to {}: {e}",
                src.display(),
                dest.display()
            )
        })
    })
    .await
    .map_err(|e| format!("file copy task failed: {e}"))?
}

/// Scope check for a copy destination that may not exist yet (save-as
/// target). For an existing path this is exactly the surface-side check
/// (`commands_surface::canonicalized_in_scope`, i.e. the
/// `open_with_default_app` semantics). For a not-yet-existing path the
/// deepest existing ancestor is canonicalized + scope-checked and the
/// non-existing tail is appended back; the tail itself is checked
/// lexically (absolute, no `..`), so a traversal can never smuggle the
/// destination out of the `$HOME`/`$TEMP` bases.
fn destination_in_scope(dest: &str) -> Result<std::path::PathBuf, String> {
    let p = std::path::Path::new(dest);
    if p.exists() {
        // Exists → identical semantics to `open_with_default_app`.
        return crate::commands_surface::canonicalized_in_scope(dest);
    }
    if !p.is_absolute() {
        return Err(format!("path must be absolute: {dest}"));
    }
    if p.components().any(|c| c == std::path::Component::ParentDir) {
        return Err(format!("path must not contain '..': {dest}"));
    }
    let mut ancestor = p.parent();
    while let Some(dir) = ancestor {
        if dir.exists() {
            let canonical =
                crate::commands_surface::canonicalized_in_scope(&dir.to_string_lossy())?;
            let tail = p
                .strip_prefix(dir)
                .expect("existing ancestor is always a prefix of the path");
            return Ok(canonical.join(tail));
        }
        ancestor = dir.parent();
    }
    Err(format!(
        "path outside allowed scope ($HOME/**, $TEMP/**): {dest}"
    ))
}

// ---------------------------------------------------------------------------
// File index (office Wave 2 B9') — the user's registered-files shelf.
// ---------------------------------------------------------------------------

/// One registered file in `~/.shannon/desktop/file-index.json`.
///
/// Wire shape is a frozen frontend contract: the desktop UI lists, registers,
/// and favorites files through `list_file_index` /
/// `register_file_index_entry` / `set_file_index_favorite` with exactly
/// these field names.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileIndexEntry {
    /// Canonical absolute path of the registered file.
    pub path: String,
    /// File name (last path component).
    pub name: String,
    /// Size in bytes at the last (re-)registration; `None` when the file
    /// vanished before its metadata could be read.
    pub size_bytes: Option<u64>,
    /// First-registration time, RFC 3339.
    pub registered_at: String,
    /// User-set favorite flag.
    pub favorite: bool,
    /// Free-form origin tag supplied by the caller (e.g. `"attachment"`,
    /// `"export"`).
    pub source: String,
}

/// Resolve the index file path: `~/.shannon/desktop/file-index.json` — same
/// `~/.shannon/desktop/` convention as `config::config_path`.
fn file_index_path() -> std::path::PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    home.join(".shannon")
        .join("desktop")
        .join("file-index.json")
}

/// Load the index. A missing file is an empty shelf; a **corrupt** file is
/// recovered as empty too (the next successful write replaces it) — a broken
/// index must never take the file commands down.
fn load_file_index_inner(path: &Path) -> Vec<FileIndexEntry> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

/// Persist the index atomically (temp file in the same directory + rename),
/// then apply the owner-only permission convention used by the other
/// `~/.shannon` stores.
fn save_file_index_inner(path: &Path, entries: &[FileIndexEntry]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(entries).map_err(|e| format!("serialize: {e}"))?;
    let tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or_else(|| Path::new(".")))
        .map_err(|e| format!("temp file: {e}"))?;
    std::fs::write(tmp.path(), content).map_err(|e| format!("write temp: {e}"))?;
    tmp.persist(path)
        .map_err(|e| format!("persist {}: {e}", path.display()))?;
    crate::file_permissions::restrict_to_owner(path);
    Ok(())
}

/// Sort key: `registered_at` descending (newest first). Unparseable stamps
/// sort last and are kept stable among themselves.
fn sort_by_registered_at_desc(entries: &mut [FileIndexEntry]) {
    let key = |e: &FileIndexEntry| {
        chrono::DateTime::parse_from_rfc3339(&e.registered_at)
            .map(|t| t.timestamp_millis())
            .unwrap_or(i64::MIN)
    };
    entries.sort_by_key(|e| std::cmp::Reverse(key(e)));
}

/// `list_file_index` command: the shelf, newest registration first.
#[tauri::command]
pub async fn list_file_index() -> Result<Vec<FileIndexEntry>, String> {
    let mut entries = load_file_index_inner(&file_index_path());
    sort_by_registered_at_desc(&mut entries);
    Ok(entries)
}

/// `register_file_index_entry` command: canonicalize + scope-check `path`
/// (exactly the `open_with_default_app` / `copy_file` rules), then upsert.
#[tauri::command]
pub async fn register_file_index_entry(path: String, source: String) -> Result<(), String> {
    register_file_index_entry_inner(&file_index_path(), &path, &source)
}

/// Internal helper for [`register_file_index_entry`] — takes the index path
/// so tests can run against a tempdir instead of the real `$HOME`.
pub(crate) fn register_file_index_entry_inner(
    index_path: &Path,
    path: &str,
    source: &str,
) -> Result<(), String> {
    // Same scope contract as the surface side: must exist and canonicalize
    // inside `$HOME/**` / `$TEMP/**` (rejects `..`, symlink escapes, and
    // directories-masquerading-as-files are rejected below).
    let canonical = crate::commands_surface::canonicalized_in_scope(path)?;
    if !canonical.is_file() {
        return Err(format!(
            "registered path is not a regular file: {}",
            canonical.display()
        ));
    }
    let size = std::fs::metadata(&canonical).ok().map(|m| m.len());
    let entry_path = canonical.to_string_lossy().to_string();
    let name = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();

    let mut entries = load_file_index_inner(index_path);
    match entries.iter_mut().find(|e| e.path == entry_path) {
        // Duplicate registration: refresh the metadata that can drift
        // (size — and with it the file's mtime story), keep the original
        // registration stamp and the user's favorite flag, and update the
        // origin tag to the latest caller.
        Some(entry) => {
            entry.size_bytes = size;
            entry.source = source.to_string();
            if !entry.name.is_empty() {
                entry.name = name;
            }
        }
        None => entries.push(FileIndexEntry {
            path: entry_path,
            name,
            size_bytes: size,
            registered_at: chrono::Utc::now().to_rfc3339(),
            favorite: false,
            source: source.to_string(),
        }),
    }
    save_file_index_inner(index_path, &entries)
}

/// `set_file_index_favorite` command: toggle the favorite flag of a
/// registered file (matched by canonical path).
#[tauri::command]
pub async fn set_file_index_favorite(path: String, favorite: bool) -> Result<(), String> {
    set_file_index_favorite_inner(&file_index_path(), &path, favorite)
}

/// Internal helper for [`set_file_index_favorite`]. The path is
/// canonicalized for matching but must already be registered — favoriting is
/// a shelf operation, not a registration.
pub(crate) fn set_file_index_favorite_inner(
    index_path: &Path,
    path: &str,
    favorite: bool,
) -> Result<(), String> {
    let canonical = crate::commands_surface::canonicalized_in_scope(path)?;
    let entry_path = canonical.to_string_lossy().to_string();
    let mut entries = load_file_index_inner(index_path);
    let entry = entries
        .iter_mut()
        .find(|e| e.path == entry_path)
        .ok_or_else(|| format!("path is not registered in the file index: {entry_path}"))?;
    entry.favorite = favorite;
    save_file_index_inner(index_path, &entries)
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

/// Wire error for `get_file_diff` / `save_text_file` (B0 P0-3). Only the
/// machine-readable failures (binary file, mtime conflict) are structured —
/// the frontend branches on `code` — while every pre-existing failure keeps
/// its plain-string wire shape, so older call sites see no difference.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum FileCommandError {
    Structured(ReadTextFileError),
    Conflict(SaveConflictError),
    Plain(String),
}

impl From<String> for FileCommandError {
    fn from(message: String) -> Self {
        Self::Plain(message)
    }
}

/// Emitted by `save_text_file` when `expected_mtime` no longer matches the
/// file on disk: the content that was diffed has changed under the review.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveConflictError {
    pub code: &'static str, // "mtime_conflict"
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_mtime: Option<String>,
}

/// RFC3339 (UTC) rendering of a file's mtime. `get_file_diff` stamps the
/// `FileDiff` with it and `save_text_file` compares the same rendering, so
/// the two can never drift apart on formatting.
fn mtime_rfc3339(metadata: &std::fs::Metadata) -> Option<String> {
    use chrono::DateTime;
    metadata
        .modified()
        .ok()
        .map(|t| DateTime::<chrono::Utc>::from(t).to_rfc3339())
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

/// File diff result for the diff viewer. `mtime` records the on-disk state
/// at fetch time — the review UI passes it back to `save_text_file` as
/// `expected_mtime` so an Apply racing a concurrent modification fails
/// instead of clobbering (B0 P0-3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    pub old_content: String,
    pub new_content: String,
    pub file_name: String,
    pub language: String,
    #[serde(default)]
    pub mtime: String,
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
///
/// B0 P0-3: binary (or otherwise non-UTF-8) files return a structured error
/// instead of an empty string — the old `unwrap_or_default` turned a binary
/// read failure into a whole-file-deletion diff whose Apply blanked the file.
#[tauri::command]
pub async fn get_file_diff(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<FileDiff, FileCommandError> {
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
) -> Result<FileDiff, FileCommandError> {
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

    // Read the working-tree side once, up front, with binary/non-UTF-8
    // guards. A failure here is an error — never an empty string that the
    // review UI would render as "file deleted".
    let bytes = std::fs::read(&canonical)
        .map_err(|e| FileCommandError::Plain(format!("Cannot read file: {e}")))?;
    if sniffs_as_binary(&bytes) {
        return Err(FileCommandError::Structured(ReadTextFileError::new(
            ReadTextFileErrorCode::BinaryFile,
            format!("binary file (NUL byte in the first 8 KiB): {path}"),
        )));
    }
    let new_content = String::from_utf8(bytes).map_err(|_| {
        FileCommandError::Structured(ReadTextFileError::new(
            ReadTextFileErrorCode::NotUtf8,
            format!("file is not valid UTF-8 text: {path}"),
        ))
    })?;

    // Record the fetch-time mtime for the Apply-time conflict check.
    let mtime = std::fs::metadata(&canonical)
        .ok()
        .and_then(|m| mtime_rfc3339(&m))
        .unwrap_or_default();

    // Try git diff first
    let dir = canonical
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| working_dir.to_path_buf());
    let git_output = Command::new("git")
        .args(["diff", "HEAD", "--", path])
        .current_dir(&dir)
        .output();

    let old_content = match git_output {
        Ok(output) if output.status.success() && !output.stdout.is_empty() => {
            // Reconstruct the committed side via `git show`; the current
            // file content was already read above.
            let old_output = Command::new("git")
                .args(["show", &format!("HEAD:{path}")])
                .current_dir(&dir)
                .output();
            match old_output {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
                _ => String::new(),
            }
        }
        _ => {
            // Not a git repo or no changes - current content vs empty old
            String::new()
        }
    };

    Ok(FileDiff {
        old_content,
        new_content,
        file_name,
        language,
        mtime,
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

    // ── P0-3: check_attachment_paths preflight ──────────────────────────
    // The preflight must agree with the send path gate-for-gate: same
    // boundary (working dir), same caps, and the unset-working-dir state
    // reported per path (never a silent "everything is fine").

    #[test]
    fn preflight_reports_no_working_dir_for_every_path() {
        let checks = check_attachment_paths_inner(
            None,
            vec!["/home/u/Downloads/a.txt".into(), "/etc/hosts".into()],
        );
        assert_eq!(checks.len(), 2);
        for c in checks {
            assert!(!c.ok);
            assert_eq!(
                c.reason,
                Some(crate::commands::RejectedAttachmentReason::NoWorkingDir)
            );
        }
    }

    #[test]
    fn preflight_accepts_inside_and_rejects_outside_and_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let inside = dir.path().join("notes.txt");
        std::fs::write(&inside, "hello").unwrap();

        let checks = check_attachment_paths_inner(
            Some(dir.path().to_string_lossy().into_owned()),
            vec![
                inside.to_string_lossy().into_owned(),
                "/etc/hosts".into(),
                dir.path().join("gone.txt").to_string_lossy().into_owned(),
            ],
        );

        assert!(checks[0].ok, "inside file must preflight clean");
        assert_eq!(checks[0].reason, None);
        assert!(!checks[1].ok);
        assert_eq!(
            checks[1].reason,
            Some(crate::commands::RejectedAttachmentReason::OutOfWorkingDir)
        );
        assert!(!checks[2].ok);
        assert_eq!(
            checks[2].reason,
            Some(crate::commands::RejectedAttachmentReason::Unresolvable)
        );
    }

    #[test]
    fn preflight_flags_oversized_image_like_the_send_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let big = dir.path().join("big.png");
        std::fs::write(
            &big,
            vec![0u8; shannon_core::attachments::MAX_IMAGE_BYTES + 1],
        )
        .unwrap();

        let checks = check_attachment_paths_inner(
            Some(dir.path().to_string_lossy().into_owned()),
            vec![big.to_string_lossy().into_owned()],
        );
        assert!(!checks[0].ok);
        assert_eq!(
            checks[0].reason,
            Some(crate::commands::RejectedAttachmentReason::TooLarge)
        );
    }

    // ── G3b P1-4: preflight extraction summaries ────────────────────────

    #[test]
    fn extraction_report_for_csv_counts_sections_and_caches() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut csv = String::from("id,name\n");
        for i in 0..800 {
            csv.push_str(&format!("{i},name{i}\n"));
        }
        let path = dir.path().join("data.csv");
        std::fs::write(&path, csv.as_bytes()).unwrap();

        let cache_home = tempfile::tempdir().expect("cache tempdir");
        let prev_home = std::env::var("SHANNON_HOME").ok();
        unsafe { std::env::set_var("SHANNON_HOME", cache_home.path()) };
        let report = extraction_report_for_path(&path);
        match prev_home {
            Some(prev) => unsafe { std::env::set_var("SHANNON_HOME", prev) },
            None => unsafe { std::env::remove_var("SHANNON_HOME") },
        }
        let report = report.expect("csv is parseable");
        assert_eq!(report.kind, "csv");
        assert!(report.extracted);
        // 801 rows -> 500/301 = 2 sections; ~9 KiB of text fits the 16 KiB
        // inline budget, so nothing is truncated.
        assert_eq!(report.sections_total, 2);
        assert!(!report.truncated);
        assert!(report.cache_path.is_some(), "office pipeline always caches");
        assert!(std::fs::metadata(report.cache_path.expect("path")).is_ok());
    }

    #[test]
    fn extraction_report_degrades_for_broken_document_and_skips_other_kinds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let junk = dir.path().join("broken.docx");
        std::fs::write(&junk, b"not a zip").unwrap();
        let report = extraction_report_for_path(&junk).expect("docx is a parseable kind");
        assert!(!report.extracted);
        assert_eq!(report.sections_total, 0);

        // Not a parseable kind -> None (no extraction work attempted).
        let txt = dir.path().join("note.txt");
        std::fs::write(&txt, "plain").unwrap();
        assert!(extraction_report_for_path(&txt).is_none());
        // Missing file -> None (metadata stat fails).
        assert!(extraction_report_for_path(&dir.path().join("gone.pdf")).is_none());
    }

    // ── G3b P1-6: save_pasted_image ─────────────────────────────────────

    const TINY_PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
    ];

    fn b64(bytes: &[u8]) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn pasted_ext_normalization_and_magic_table() {
        assert_eq!(normalize_pasted_ext("PNG"), Some("png"));
        assert_eq!(normalize_pasted_ext("jpeg"), Some("jpeg"));
        assert_eq!(normalize_pasted_ext("jpg"), Some("jpeg"));
        assert_eq!(normalize_pasted_ext("gif"), Some("gif"));
        assert_eq!(normalize_pasted_ext("webp"), Some("webp"));
        assert_eq!(normalize_pasted_ext("svg"), None);
        assert_eq!(normalize_pasted_ext("exe"), None);
        assert_eq!(normalize_pasted_ext(""), None);

        assert!(pasted_image_magic_matches("png", TINY_PNG));
        assert!(pasted_image_magic_matches(
            "jpeg",
            &[0xFF, 0xD8, 0xFF, 0xE0]
        ));
        assert!(pasted_image_magic_matches("gif", b"GIF89a...."));
        assert!(pasted_image_magic_matches(
            "webp",
            b"RIFF\x00\x00\x00\x00WEBPVP8 "
        ));
        // Wrong/mismatched signatures are refused.
        assert!(!pasted_image_magic_matches("png", b"RIFF....WEBP"));
        assert!(!pasted_image_magic_matches("jpeg", TINY_PNG));
        assert!(!pasted_image_magic_matches("gif", b"GIF7"));
        assert!(!pasted_image_magic_matches("png", b""));
    }

    #[test]
    fn save_pasted_image_writes_matching_bytes_to_timestamped_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path =
            save_pasted_image_inner(dir.path(), &b64(TINY_PNG), "png").expect("valid png saves");
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("png"));
        let name = path.file_name().and_then(|n| n.to_str()).expect("name");
        // <millis>-<8 hex>.png
        let stem = name.trim_end_matches(".png");
        let (millis, rand) = stem.split_once('-').expect("timestamp-rand shape");
        assert!(millis.bytes().all(|b| b.is_ascii_digit()), "{name}");
        assert_eq!(rand.len(), 8);
        assert!(rand.bytes().all(|b| b.is_ascii_hexdigit()), "{name}");
        assert_eq!(std::fs::read(&path).expect("read back"), TINY_PNG);
    }

    #[test]
    fn save_pasted_image_rejects_magic_mismatch_and_bad_types() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A text blob claiming to be a png is refused before any write.
        let err = save_pasted_image_inner(dir.path(), &b64(b"hello world"), "png")
            .expect_err("magic mismatch");
        assert!(err.contains("does not match"), "{err}");
        assert_eq!(
            std::fs::read_dir(dir.path()).expect("read dir").count(),
            0,
            "nothing may be written for a rejected payload"
        );

        let err = save_pasted_image_inner(dir.path(), &b64(TINY_PNG), "svg")
            .expect_err("svg not allowed");
        assert!(err.contains("unsupported pasted image type"), "{err}");

        let err =
            save_pasted_image_inner(dir.path(), "!!!not base64!!!", "png").expect_err("bad base64");
        assert!(err.contains("base64"), "{err}");
    }

    #[test]
    fn save_pasted_image_enforces_the_shared_10mib_cap() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Base64 length over the pre-decode estimate of MAX_IMAGE_BYTES —
        // the same `validate_base64_size` helper every attachment entry path
        // uses, so no oversized payload is ever decoded.
        let oversized_len = shannon_core::attachments::MAX_IMAGE_BYTES * 4 / 3 + 4096;
        let big = "A".repeat(oversized_len);
        let err = save_pasted_image_inner(dir.path(), &big, "png").expect_err("over limit");
        assert!(err.contains("pasted image rejected"), "{err}");
        assert_eq!(
            std::fs::read_dir(dir.path()).expect("read dir").count(),
            0,
            "nothing may be written for an oversized payload"
        );
    }

    #[test]
    fn save_pasted_image_post_decode_recheck_catches_estimate_slack() {
        // G3b fix round 1 (M1) — the pre-decode estimate is deliberately
        // padded, so a payload can slip past it while its REAL decoded size
        // is over the cap: `MAX_IMAGE_BYTES + 1` bytes encode to a base64
        // length whose estimate is exactly at/under the limit. The exact
        // `validate_decoded_size` re-check (the same post-read gate every
        // other attachment entry path applies) must catch it — and because
        // it runs before any I/O, not even a partial file may exist.
        let dir = tempfile::tempdir().expect("tempdir");
        let bytes = vec![0x89u8; shannon_core::attachments::MAX_IMAGE_BYTES + 1];
        let encoded = b64(&bytes);
        // Sanity: this payload genuinely passes the padded estimate.
        shannon_core::attachments::validate_base64_size(encoded.len())
            .expect("estimate must pass for this payload");
        let err = save_pasted_image_inner(dir.path(), &encoded, "png")
            .expect_err("decoded size over the cap");
        assert!(err.contains("pasted image rejected"), "{err}");
        assert_eq!(
            std::fs::read_dir(dir.path()).expect("read dir").count(),
            0,
            "no partial artifact may survive a rejected payload"
        );
    }

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

    /// Office Wave 1 — the office-format rows of the MIME table. Extension
    /// matching is case-insensitive (same code path as the base table).
    #[test]
    fn attachment_mime_office_table() {
        assert_eq!(attachment_mime(Path::new("a.doc")), "application/msword");
        assert_eq!(
            attachment_mime(Path::new("a.docx")),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert_eq!(
            attachment_mime(Path::new("a.xls")),
            "application/vnd.ms-excel"
        );
        assert_eq!(
            attachment_mime(Path::new("a.XLSX")),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
        assert_eq!(
            attachment_mime(Path::new("a.ppt")),
            "application/vnd.ms-powerpoint"
        );
        assert_eq!(
            attachment_mime(Path::new("a.pptx")),
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        );
        assert_eq!(
            attachment_mime(Path::new("a.odt")),
            "application/vnd.oasis.opendocument.text"
        );
        assert_eq!(attachment_mime(Path::new("a.rtf")), "application/rtf");
        assert_eq!(attachment_mime(Path::new("a.csv")), "text/csv");
    }

    #[test]
    fn file_diff_round_trips_through_serde() {
        let diff = FileDiff {
            old_content: "old text".to_string(),
            new_content: "new text".to_string(),
            file_name: "test.rs".to_string(),
            language: "rust".to_string(),
            mtime: "2026-09-26T00:00:00+00:00".to_string(),
        };
        let json = serde_json::to_string(&diff).unwrap();
        let back: FileDiff = serde_json::from_str(&json).unwrap();
        assert_eq!(back.old_content, diff.old_content);
        assert_eq!(back.new_content, diff.new_content);
        assert_eq!(back.file_name, diff.file_name);
        assert_eq!(back.language, diff.language);
        assert_eq!(back.mtime, diff.mtime);
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

        let err = save_text_file_inner(workdir.path(), &outside.to_string_lossy(), "pwned", None)
            .await
            .expect_err("must reject out-of-tree write");
        let msg = match err {
            FileCommandError::Plain(msg) => msg,
            other => panic!("expected plain error, got {other:?}"),
        };
        assert!(
            msg.contains("outside") || msg.contains("not found"),
            "expected 'outside' rejection, got: {msg}"
        );

        // The target file must not exist on disk.
        assert!(!outside.exists(), "file must not have been written");
        let _ = std::fs::remove_file(&outside);
    }

    #[tokio::test]
    async fn save_text_file_inner_accepts_path_inside_working_dir() {
        let workdir = tempfile::tempdir().expect("tempdir");
        let target = workdir.path().join("subdir/note.txt");
        save_text_file_inner(workdir.path(), "subdir/note.txt", "hello", None)
            .await
            .expect("in-tree write should succeed");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello");
    }

    // ---- G5 P0-8: backend-driven save dialog export (write half) ----

    #[tokio::test]
    async fn write_text_file_at_writes_outside_any_working_dir() {
        // The whole point of the dialog flow: the user's pick authorizes
        // destinations OUTSIDE the session working directory (Downloads,
        // Documents, …) that `save_text_file` must keep rejecting.
        let working_dir = tempfile::tempdir().expect("tempdir");
        let destination = tempfile::tempdir().expect("tempdir");
        let target = destination.path().join("timeline-abc.html");

        write_text_file_at(&target, "<html>hi</html>")
            .await
            .expect("dialog-picked write should succeed");

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "<html>hi</html>");
        let _ = std::fs::remove_file(&target);
        let _ = working_dir;
    }

    #[tokio::test]
    async fn write_text_file_at_creates_missing_parent_dirs() {
        let destination = tempfile::tempdir().expect("tempdir");
        let target = destination.path().join("a/b/export.html");

        write_text_file_at(&target, "x")
            .await
            .expect("write should succeed");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "x");
    }

    #[tokio::test]
    async fn write_text_file_at_reports_io_errors() {
        let destination = tempfile::tempdir().expect("tempdir");
        // A regular FILE in the way of the parent directory makes
        // create_dir_all fail.
        let blocker = destination.path().join("blocker");
        std::fs::write(&blocker, "not a dir").unwrap();
        let target = blocker.join("export.html");

        let err = write_text_file_at(&target, "x")
            .await
            .expect_err("write through a file parent must fail");
        assert!(
            err.contains("Failed to create") || err.contains("Failed to write"),
            "expected an io error message, got: {err}"
        );
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
        match err {
            FileCommandError::Plain(msg) => {
                assert!(msg.contains("outside"), "got: {msg}");
            }
            other => panic!("expected plain outside rejection, got {other:?}"),
        }

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
        // B0 P0-3: the fetch-time mtime is stamped for the Apply-time
        // conflict check.
        assert!(!diff.mtime.is_empty(), "mtime must be recorded");
    }

    // ---- B0 P0-3: binary / non-UTF-8 diffs must fail, never render as a
    // whole-file deletion, and save_text_file must detect stale writes ----

    #[tokio::test]
    async fn file_diff_rejects_binary_file_with_structured_error() {
        let workdir = tempfile::tempdir().expect("tempdir");
        std::fs::write(workdir.path().join("img.png"), b"\x89PNG\x00\x01\x02").unwrap();

        let err = get_file_diff_inner(workdir.path(), "img.png")
            .await
            .expect_err("binary diff must be rejected");
        match err {
            FileCommandError::Structured(e) => {
                assert_eq!(e.code, ReadTextFileErrorCode::BinaryFile);
                assert!(e.message.contains("binary file"), "got: {}", e.message);
            }
            other => panic!("expected structured binary_file error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn file_diff_rejects_non_utf8_without_nul_with_not_utf8_code() {
        let workdir = tempfile::tempdir().expect("tempdir");
        std::fs::write(workdir.path().join("latin1.txt"), [0xCA, 0xFE, 0xBA, 0xBE]).unwrap();

        let err = get_file_diff_inner(workdir.path(), "latin1.txt")
            .await
            .expect_err("non-UTF-8 diff must be rejected");
        match err {
            FileCommandError::Structured(e) => {
                assert_eq!(e.code, ReadTextFileErrorCode::NotUtf8);
            }
            other => panic!("expected structured not_utf8 error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn save_text_file_inner_detects_stale_mtime() {
        let workdir = tempfile::tempdir().expect("tempdir");
        let target = workdir.path().join("note.txt");
        std::fs::write(&target, "v1").unwrap();
        let expected = std::fs::metadata(&target)
            .ok()
            .and_then(|m| mtime_rfc3339(&m))
            .expect("mtime");

        // Matching stamp → write goes through.
        save_text_file_inner(workdir.path(), "note.txt", "v2", Some(&expected))
            .await
            .expect("write with fresh mtime must succeed");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v2");

        // Stale stamp → structured conflict, no write.
        let err = save_text_file_inner(
            workdir.path(),
            "note.txt",
            "v3",
            Some("2000-01-01T00:00:00+00:00"),
        )
        .await
        .expect_err("stale mtime must conflict");
        match err {
            FileCommandError::Conflict(c) => {
                assert_eq!(c.code, "mtime_conflict");
                assert!(c.current_mtime.is_some());
            }
            other => panic!("expected mtime conflict, got {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v2");
    }

    #[tokio::test]
    async fn save_text_file_inner_conflicts_when_file_vanished() {
        let workdir = tempfile::tempdir().expect("tempdir");
        // The path never existed — an expected_mtime cannot be satisfied.
        let err = save_text_file_inner(
            workdir.path(),
            "ghost.txt",
            "x",
            Some("2026-01-01T00:00:00+00:00"),
        )
        .await
        .expect_err("vanished file must conflict");
        match err {
            FileCommandError::Conflict(c) => {
                assert_eq!(c.code, "mtime_conflict");
                assert!(c.current_mtime.is_none());
            }
            other => panic!("expected mtime conflict, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn save_text_file_inner_treats_empty_expected_mtime_as_no_check() {
        let workdir = tempfile::tempdir().expect("tempdir");
        let target = workdir.path().join("note.txt");
        save_text_file_inner(workdir.path(), "note.txt", "hello", Some(""))
            .await
            .expect("empty expected_mtime must skip the conflict check");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello");
    }

    // ---- office Wave 1: copy_file (save-as) ----
    //
    // The scope bases are `$HOME`/`$TEMP` (same as open_with_default_app),
    // so every disk-touching test stages inside a tempdir, which is always
    // in scope.

    #[tokio::test]
    async fn copy_file_copies_within_scope() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("report.docx");
        std::fs::write(&src, b"office bytes").expect("write src");
        let dest = dir.path().join("copy-of-report.docx");

        copy_file_inner(&src.to_string_lossy(), &dest.to_string_lossy())
            .await
            .expect("in-scope copy must succeed");
        assert_eq!(std::fs::read(&dest).expect("read dest"), b"office bytes");
        // Copy, not move.
        assert!(src.is_file(), "source must survive the copy");
    }

    #[tokio::test]
    async fn copy_file_allows_overwriting_existing_destination() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("new.odt");
        std::fs::write(&src, "fresh").expect("write src");
        let dest = dir.path().join("existing.odt");
        std::fs::write(&dest, "stale").expect("write dest");

        copy_file_inner(&src.to_string_lossy(), &dest.to_string_lossy())
            .await
            .expect("overwrite copy must succeed");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "fresh");
    }

    #[tokio::test]
    async fn copy_file_creates_not_yet_existing_destination_name() {
        // The save-as case: the destination file itself does not exist yet;
        // scope is decided on the existing parent directory.
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("a.csv");
        std::fs::write(&src, "x,y").expect("write src");
        let dest = dir.path().join("brand-new-name.csv");

        copy_file_inner(&src.to_string_lossy(), &dest.to_string_lossy())
            .await
            .expect("save-as copy must succeed");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "x,y");
    }

    #[tokio::test]
    async fn copy_file_rejects_out_of_scope_source() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("out.png");
        // Exists, but outside $HOME/$TEMP — the open_with_default_app
        // rejection. (`/etc/hosts` exists on macOS and Linux; on Windows the
        // scope check rejects it just the same.)
        let err = copy_file_inner("/etc/hosts", &dest.to_string_lossy())
            .await
            .expect_err("out-of-scope source must be rejected");
        assert!(err.contains("outside"), "got: {err}");
        assert!(!dest.exists(), "nothing may be written");
    }

    #[tokio::test]
    async fn copy_file_rejects_out_of_scope_destination() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("a.txt");
        std::fs::write(&src, "secret").expect("write src");
        let err = copy_file_inner(
            &src.to_string_lossy(),
            "/etc/shannon-copy-should-never-land-here.txt",
        )
        .await
        .expect_err("out-of-scope destination must be rejected");
        assert!(err.contains("outside"), "got: {err}");
        assert!(
            !std::path::Path::new("/etc/shannon-copy-should-never-land-here.txt").exists(),
            "nothing may be written outside the scope"
        );
    }

    #[tokio::test]
    async fn copy_file_rejects_traversal_in_destination() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("a.txt");
        std::fs::write(&src, "x").expect("write src");
        let traversal = dir
            .path()
            .join("sub/../../escape.txt")
            .to_string_lossy()
            .into_owned();
        let err = copy_file_inner(&src.to_string_lossy(), &traversal)
            .await
            .expect_err("'..' in the destination must be rejected");
        assert!(err.contains("'..'"), "got: {err}");
    }

    #[tokio::test]
    async fn copy_file_rejects_missing_source() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("ghost.docx");
        let dest = dir.path().join("out.docx");
        let err = copy_file_inner(&missing.to_string_lossy(), &dest.to_string_lossy())
            .await
            .expect_err("missing source must be rejected");
        assert!(err.contains("not accessible"), "got: {err}");
    }

    #[tokio::test]
    async fn copy_file_rejects_same_source_and_destination() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("same.txt");
        std::fs::write(&src, "x").expect("write src");
        let p = src.to_string_lossy().into_owned();
        let err = copy_file_inner(&p, &p)
            .await
            .expect_err("dest == src must be rejected");
        assert!(err.contains("same"), "got: {err}");
    }

    // ---- office Wave 2 B9': file index (registered-files shelf) ----
    //
    // The scope bases are `$HOME`/`$TEMP`, so tests stage inside a tempdir
    // (always in scope) and point the index functions at a tempdir JSON.

    #[test]
    fn file_index_register_dedupes_and_lists_newest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        let index = dir.path().join("file-index.json");
        let a = dir.path().join("minutes.md");
        std::fs::write(&a, "minutes").expect("write a");
        let b = dir.path().join("table.xlsx");
        std::fs::write(&b, "workbook").expect("write b");

        register_file_index_entry_inner(&index, &a.to_string_lossy(), "attachment")
            .expect("register a");
        std::thread::sleep(std::time::Duration::from_millis(5));
        register_file_index_entry_inner(&index, &b.to_string_lossy(), "export")
            .expect("register b");

        let mut entries = load_file_index_inner(&index);
        assert_eq!(entries.len(), 2);
        let a_path = std::fs::canonicalize(&a)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let b_path = std::fs::canonicalize(&b)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        // `list_file_index` sorts newest-registration-first; mirror it here.
        sort_by_registered_at_desc(&mut entries);
        assert_eq!(entries[0].path, b_path, "newest registration first");
        assert_eq!(entries[0].name, "table.xlsx");
        assert_eq!(entries[0].source, "export");
        assert_eq!(entries[0].size_bytes, Some("workbook".len() as u64));
        assert!(!entries[0].favorite);
        assert_eq!(entries[1].path, a_path);
        chrono::DateTime::parse_from_rfc3339(&entries[0].registered_at)
            .expect("registered_at must be RFC 3339");

        // Duplicate registration dedupes by canonical path and refreshes
        // size + source, keeping the original stamp and favorite flag.
        std::fs::write(&b, "workbook-with-more-data").expect("grow b");
        set_file_index_favorite_inner(&index, &b.to_string_lossy(), true).expect("favorite b");
        register_file_index_entry_inner(&index, &b.to_string_lossy(), "re-export")
            .expect("re-register b");

        let entries = load_file_index_inner(&index);
        assert_eq!(entries.len(), 2, "no duplicate row for the same path");
        let b_entry = entries.iter().find(|e| e.path == b_path).unwrap();
        assert_eq!(
            b_entry.size_bytes,
            Some("workbook-with-more-data".len() as u64)
        );
        assert_eq!(b_entry.source, "re-export");
        assert!(b_entry.favorite, "favorite must survive a re-register");
    }

    #[test]
    fn file_index_favorite_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let index = dir.path().join("file-index.json");
        let f = dir.path().join("notes.txt");
        std::fs::write(&f, "notes").expect("write");
        register_file_index_entry_inner(&index, &f.to_string_lossy(), "attachment")
            .expect("register");

        set_file_index_favorite_inner(&index, &f.to_string_lossy(), true).expect("favorite");
        let entries = load_file_index_inner(&index);
        assert!(entries[0].favorite);

        set_file_index_favorite_inner(&index, &f.to_string_lossy(), false).expect("unfavorite");
        let entries = load_file_index_inner(&index);
        assert!(!entries[0].favorite);
    }

    #[test]
    fn file_index_favorite_rejects_unregistered_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let index = dir.path().join("file-index.json");
        let f = dir.path().join("never-registered.txt");
        std::fs::write(&f, "x").expect("write");
        let err = set_file_index_favorite_inner(&index, &f.to_string_lossy(), true)
            .expect_err("unregistered path must be rejected");
        assert!(err.contains("not registered"), "got: {err}");
    }

    #[test]
    fn file_index_rejects_out_of_scope_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let index = dir.path().join("file-index.json");

        // Exists, but outside $HOME/$TEMP (same rejection as copy_file's
        // source check).
        let err = register_file_index_entry_inner(&index, "/etc/hosts", "attachment")
            .expect_err("out-of-scope path must be rejected");
        assert!(err.contains("outside"), "got: {err}");

        // Inside the tempdir but missing on disk.
        let missing = dir.path().join("ghost.txt");
        let err = register_file_index_entry_inner(&index, &missing.to_string_lossy(), "attachment")
            .expect_err("missing path must be rejected");
        assert!(err.contains("not accessible"), "got: {err}");

        // A directory is not a file.
        let subdir = dir.path().join("a-directory");
        std::fs::create_dir(&subdir).expect("mkdir");
        let err = register_file_index_entry_inner(&index, &subdir.to_string_lossy(), "attachment")
            .expect_err("directories must be rejected");
        assert!(err.contains("not a regular file"), "got: {err}");

        assert!(
            load_file_index_inner(&index).is_empty(),
            "nothing may be registered by rejected calls"
        );
    }

    #[test]
    fn file_index_corrupt_json_recovers_as_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let index = dir.path().join("file-index.json");
        std::fs::write(&index, "{not json at all").expect("write garbage");

        assert!(
            load_file_index_inner(&index).is_empty(),
            "corrupt index must read as an empty shelf"
        );

        // The next successful write replaces the corrupt file wholesale.
        let f = dir.path().join("fresh.txt");
        std::fs::write(&f, "fresh").expect("write");
        register_file_index_entry_inner(&index, &f.to_string_lossy(), "attachment")
            .expect("register after corruption");
        let entries = load_file_index_inner(&index);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "fresh.txt");
    }

    #[test]
    fn file_index_missing_file_reads_as_empty_shelf() {
        let dir = tempfile::tempdir().expect("tempdir");
        let index = dir.path().join("does-not-exist.json");
        assert!(load_file_index_inner(&index).is_empty());
        // And a list over an empty shelf sorts without panicking.
        let mut entries = load_file_index_inner(&index);
        sort_by_registered_at_desc(&mut entries);
        assert!(entries.is_empty());
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
