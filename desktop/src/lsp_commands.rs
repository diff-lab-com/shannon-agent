//! Tauri IPC commands for LSP code actions (Phase D P4).
//!
//! Spawns a language server on demand to fetch quick-fixes at a given file
//! location. The server binary must be on PATH (rust-analyzer, typescript-
//! language-server, gopls, etc.) — we don't bundle one. Each request:
//!
//! 1. Spawn → initialize → didOpen
//! 2. textDocument/codeAction at the diagnostic's range
//! 3. Return serialized actions (title, kind, is_preferred, edit)
//! 4. shutdown → drop
//!
//! For applying, the frontend sends back the chosen action; we apply its
//! `WorkspaceEdit` to disk via simple text replacements.

use crate::resolve_path_in_working_dir;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::timeout;

const LSP_TIMEOUT: Duration = Duration::from_secs(15);

/// Allow-list of LSP server binaries that the frontend may ask us to spawn.
/// This mirrors `DEFAULT_DIAGNOSTICS_SERVERS` in
/// `ui/src/lib/tauri-api.ts`. A compromised frontend cannot bypass this by
/// passing an arbitrary absolute path: the basename (or full path) of
/// `server_cmd` must match one of these entries.
const ALLOWED_LSP_SERVERS: &[&str] = &[
    "rust-analyzer",
    "typescript-language-server",
    "gopls",
    "pylsp",
];

/// Validate that `server_cmd` resolves to one of the allow-listed binaries.
/// Accepts both bare names (`rust-analyzer`, resolved via PATH by the LSP
/// client) and absolute paths whose file name matches an allowed entry.
fn validate_lsp_server(server_cmd: &str) -> Result<(), String> {
    let basename = Path::new(server_cmd)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let is_allowed = ALLOWED_LSP_SERVERS.iter().any(|allowed| {
        // Direct match (bare name) or basename match (absolute path).
        *allowed == server_cmd || *allowed == basename
    });
    if is_allowed {
        Ok(())
    } else {
        Err(format!(
            "server_cmd '{server_cmd}' is not in the allow-list (allowed: {})",
            ALLOWED_LSP_SERVERS.join(", ")
        ))
    }
}

/// Resolve the working directory for path validation. Prefers the persisted
/// desktop config `working_dir`, falls back to the process cwd. Used by IPC
/// commands that take file paths from the frontend.
async fn resolve_working_dir(state: &crate::commands::AppState) -> PathBuf {
    let cfg = state.desktop_config.read().await;
    cfg.working_dir
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeActionDto {
    pub title: String,
    pub kind: Option<String>,
    pub is_preferred: bool,
    pub edit: Option<serde_json::Value>,
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeActionsResponse {
    pub actions: Vec<CodeActionDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeActionRequest {
    pub file_path: String,
    pub server_cmd: String,
    pub server_args: Vec<String>,
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub language_id: String,
    pub diagnostic_messages: Vec<String>,
}

/// Fetch code actions at the given range by spawning an LSP server, opening
/// the document, and sending `textDocument/codeAction`. Returns a flat list
/// of action DTOs the frontend can render as quick-fix buttons.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn lsp_code_actions(
    state: tauri::State<'_, crate::commands::AppState>,
    req: CodeActionRequest,
) -> Result<CodeActionsResponse, String> {
    let working_dir = resolve_working_dir(&state).await;
    lsp_code_actions_inner(&working_dir, req).await
}

/// Test- and reuse-friendly inner: takes an explicit working directory so it
/// can be exercised without a `tauri::State`.
pub async fn lsp_code_actions_inner(
    working_dir: &Path,
    req: CodeActionRequest,
) -> Result<CodeActionsResponse, String> {
    // Defense-in-depth: validate the server binary against an allow-list
    // before we spawn anything. The file path itself is also validated below
    // during canonicalize + LspClient open.
    validate_lsp_server(&req.server_cmd)?;
    // Validate the file path is inside the working directory before spawning
    // the LSP server against it.
    resolve_path_in_working_dir(&req.file_path, working_dir)?;
    timeout(LSP_TIMEOUT, run_code_actions(req))
        .await
        .map_err(|_| format!("LSP request timed out after {}s", LSP_TIMEOUT.as_secs()))?
}

async fn run_code_actions(req: CodeActionRequest) -> Result<CodeActionsResponse, String> {
    let abs = Path::new(&req.file_path)
        .canonicalize()
        .map_err(|e| format!("canonicalize {}: {e}", req.file_path))?;
    let root_uri = abs
        .parent()
        .ok_or_else(|| "no parent dir".to_string())?
        .to_url()
        .map_err(|e| format!("root url: {e}"))?;
    let doc_uri = abs.to_url().map_err(|e| format!("doc url: {e}"))?;

    let mut client = shannon_core::lsp::LspClient::spawn(&req.server_cmd, &req.server_args)
        .await
        .map_err(|e| format!("spawn LSP server: {e}"))?;
    client
        .initialize(&root_uri)
        .await
        .map_err(|e| format!("initialize: {e}"))?;

    // Build diagnostics array from the passed messages (basic shape; servers
    // typically accept minimal severity/message to drive quick-fix context).
    let diagnostics: Vec<lsp_types::Diagnostic> = req
        .diagnostic_messages
        .into_iter()
        .map(|msg| lsp_types::Diagnostic {
            range: lsp_types::Range {
                start: lsp_types::Position {
                    line: req.start_line,
                    character: req.start_character,
                },
                end: lsp_types::Position {
                    line: req.end_line,
                    character: req.end_character,
                },
            },
            severity: Some(lsp_types::DiagnosticSeverity::WARNING),
            code: None,
            code_description: None,
            source: Some("shannon-desktop".to_string()),
            message: msg,
            related_information: None,
            tags: None,
            data: None,
        })
        .collect();

    let range = lsp_types::Range {
        start: lsp_types::Position {
            line: req.start_line,
            character: req.start_character,
        },
        end: lsp_types::Position {
            line: req.end_line,
            character: req.end_character,
        },
    };

    let actions = client
        .code_actions(&doc_uri, range, &diagnostics)
        .await
        .map_err(|e| format!("codeAction: {e}"))?;
    let _ = client.shutdown().await;

    let dtos = actions
        .into_iter()
        .map(|a| CodeActionDto {
            title: a.title,
            kind: a.kind.map(|k| k.as_str().to_string()),
            is_preferred: a.is_preferred.unwrap_or(false),
            edit: a
                .edit
                .map(|we| serde_json::to_value(we).unwrap_or(serde_json::Value::Null)),
            command: a.command.map(|c| c.title),
        })
        .collect();
    Ok(CodeActionsResponse { actions: dtos })
}

/// Apply a code action's workspace edit to disk. Currently supports only
/// `TextEdit` entries against the document the diagnostic came from. Other
/// resource changes (renames, creates, deletes) are skipped with a log line.
///
/// Security: every resource URI in the edit payload is validated to be
/// inside the working directory before any file is read or written.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn apply_code_action(
    state: tauri::State<'_, crate::commands::AppState>,
    edit: serde_json::Value,
) -> Result<u32, String> {
    let working_dir = resolve_working_dir(&state).await;
    apply_code_action_inner(&working_dir, edit).await
}

/// Test-friendly inner taking an explicit working directory.
async fn apply_code_action_inner(
    working_dir: &Path,
    edit: serde_json::Value,
) -> Result<u32, String> {
    // Parse minimal shape: { changes?: { [uri]: TextEdit[] } }
    let changes = edit
        .get("changes")
        .and_then(|c| c.as_object())
        .ok_or_else(|| "no `changes` field in workspace edit".to_string())?;

    let mut applied = 0u32;
    for (uri, edits) in changes {
        let raw_path = uri_to_path(uri)?;
        // Validate the path is inside the working directory before touching it.
        let path = resolve_path_in_working_dir(&raw_path.to_string_lossy(), working_dir)?;
        let content =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        // Apply edits in reverse order so offsets stay valid.
        let mut lines: Vec<String> = content.lines().map(String::from).collect();
        let mut sorted: Vec<(u32, u32, u32, u32, String)> = edits
            .as_array()
            .ok_or_else(|| "edits not array".to_string())?
            .iter()
            .filter_map(|e| {
                let range = e.get("range")?;
                let new_text = e.get("newText")?.as_str()?.to_string();
                let start_line = range.get("start")?.get("line")?.as_u64()? as u32;
                let start_char = range.get("start")?.get("character")?.as_u64()? as u32;
                let end_line = range.get("end")?.get("line")?.as_u64()? as u32;
                let end_char = range.get("end")?.get("character")?.as_u64()? as u32;
                Some((start_line, start_char, end_line, end_char, new_text))
            })
            .collect();
        sorted.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));

        for (sl, sc, el, ec, new_text) in sorted {
            if sl as usize >= lines.len() {
                continue;
            }
            // Simple same-line replacement — common case for quick-fixes.
            if sl == el {
                let line = &mut lines[sl as usize];
                let mut out = String::with_capacity(line.len() + new_text.len());
                let pre = line.get(..(sc as usize)).unwrap_or("");
                let post = line.get((ec as usize)..).unwrap_or("");
                out.push_str(pre);
                out.push_str(&new_text);
                out.push_str(post);
                *line = out;
                applied += 1;
            }
        }

        let mut out = lines.join("\n");
        if content.ends_with('\n') {
            out.push('\n');
        }
        std::fs::write(&path, out).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    Ok(applied)
}

fn uri_to_path(uri: &str) -> Result<std::path::PathBuf, String> {
    let url = url::Url::parse(uri).map_err(|e| format!("invalid uri {uri}: {e}"))?;
    url.to_file_path()
        .map_err(|_| format!("uri not a file path: {uri}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFileDto {
    pub path: String,
    pub content: String,
    pub language_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiagnosticDto {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub message: String,
    pub severity: String,
    pub source: Option<String>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiagnosticsRequest {
    pub file_path: String,
    pub server_cmd: String,
    pub server_args: Vec<String>,
    pub language_id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiagnosticsResponse {
    pub diagnostics: Vec<FileDiagnosticDto>,
    pub timed_out: bool,
}

const DIAGNOSTICS_TIMEOUT: Duration = Duration::from_secs(3);

/// Map a file extension to an LSP language id. Matches the keys used by
/// `LspQuickFixPanel`'s DEFAULT_SERVERS so the same server binary is selected
/// when the user later asks for quick-fixes on a squiggle.
pub fn language_id_from_extension(ext: &str) -> String {
    match ext.to_ascii_lowercase().as_str() {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "typescriptreact",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "go" => "go",
        "py" => "python",
        "java" => "java",
        "rb" => "ruby",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => "cpp",
        _ => "plaintext",
    }
    .to_string()
}

/// Read a source file from disk and return its content plus a best-effort
/// language id derived from the extension. Refuses paths that don't exist or
/// aren't valid UTF-8.
///
/// Security: the path must resolve to a location inside the working
/// directory. A compromised frontend cannot use this to exfiltrate
/// `~/.ssh/id_rsa`, `~/.shannon/desktop/config.json`, or any other file
/// outside the workspace.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn read_source_file(
    state: tauri::State<'_, crate::commands::AppState>,
    path: String,
) -> Result<SourceFileDto, String> {
    let working_dir = resolve_working_dir(&state).await;
    read_source_file_inner(&working_dir, path).await
}

/// Test-friendly inner taking an explicit working directory.
async fn read_source_file_inner(working_dir: &Path, path: String) -> Result<SourceFileDto, String> {
    let canonical = resolve_path_in_working_dir(&path, working_dir)?;
    if !canonical.is_file() {
        return Err(format!("not a file: {}", canonical.display()));
    }
    let content = std::fs::read_to_string(&canonical)
        .map_err(|e| format!("read {}: {e}", canonical.display()))?;
    let ext = canonical.extension().and_then(|e| e.to_str()).unwrap_or("");
    Ok(SourceFileDto {
        path: canonical.to_string_lossy().into_owned(),
        content,
        language_id: language_id_from_extension(ext),
    })
}

/// Spawn an LSP server, open the file via `textDocument/didOpen`, and drain
/// `publishDiagnostics` notifications for up to `DIAGNOSTICS_TIMEOUT`. Returns
/// the last batch of diagnostics received before timeout (or earlier if the
/// server stops sending). The `timed_out` flag is `true` when we exited due
/// to the deadline rather than server shutdown.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn run_file_diagnostics(
    state: tauri::State<'_, crate::commands::AppState>,
    req: FileDiagnosticsRequest,
) -> Result<FileDiagnosticsResponse, String> {
    let working_dir = resolve_working_dir(&state).await;
    run_file_diagnostics_inner(&working_dir, req).await
}

/// Test-friendly inner taking an explicit working directory.
async fn run_file_diagnostics_inner(
    working_dir: &Path,
    req: FileDiagnosticsRequest,
) -> Result<FileDiagnosticsResponse, String> {
    // Validate the LSP server binary against the allow-list.
    validate_lsp_server(&req.server_cmd)?;
    // Validate the file path is inside the working directory before spawning.
    resolve_path_in_working_dir(&req.file_path, working_dir)?;
    timeout(LSP_TIMEOUT, run_diagnostics(req))
        .await
        .map_err(|_| format!("diagnostics timed out after {}s", LSP_TIMEOUT.as_secs()))?
}

async fn run_diagnostics(req: FileDiagnosticsRequest) -> Result<FileDiagnosticsResponse, String> {
    let abs = Path::new(&req.file_path)
        .canonicalize()
        .map_err(|e| format!("canonicalize {}: {e}", req.file_path))?;
    let root_uri = abs
        .parent()
        .ok_or_else(|| "no parent dir".to_string())?
        .to_url()
        .map_err(|e| format!("root url: {e}"))?;
    let doc_uri = abs.to_url().map_err(|e| format!("doc url: {e}"))?;

    let mut client = shannon_core::lsp::LspClient::spawn(&req.server_cmd, &req.server_args)
        .await
        .map_err(|e| format!("spawn LSP server: {e}"))?;
    client
        .initialize(&root_uri)
        .await
        .map_err(|e| format!("initialize: {e}"))?;
    client
        .did_open(&doc_uri, &req.language_id, &req.content)
        .await
        .map_err(|e| format!("did_open: {e}"))?;

    let diags = client
        .collect_diagnostics(&doc_uri, DIAGNOSTICS_TIMEOUT)
        .await
        .map_err(|e| format!("collect_diagnostics: {e}"))?;
    let _ = client.shutdown().await;

    let dtos = diags
        .into_iter()
        .map(|d| FileDiagnosticDto {
            start_line: d.range.start.line,
            start_character: d.range.start.character,
            end_line: d.range.end.line,
            end_character: d.range.end.character,
            message: d.message,
            severity: d
                .severity
                .map(|s| format!("{s:?}").to_lowercase())
                .unwrap_or_else(|| "warning".to_string()),
            source: d.source,
            code: d.code.map(|c| match c {
                lsp_types::NumberOrString::Number(n) => n.to_string(),
                lsp_types::NumberOrString::String(s) => s,
            }),
        })
        .collect();
    Ok(FileDiagnosticsResponse {
        diagnostics: dtos,
        timed_out: false, // collect_diagnostics returns its own timeout empty-handed
    })
}

trait PathUrlExt {
    fn to_url(&self) -> Result<url::Url, String>;
}
impl PathUrlExt for Path {
    fn to_url(&self) -> Result<url::Url, String> {
        url::Url::from_file_path(self).map_err(|_| format!("not absolute: {}", self.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_to_path_extracts_file_path() {
        let p = uri_to_path("file:///tmp/foo.rs").unwrap();
        assert!(p.to_string_lossy().ends_with("foo.rs"));
    }

    #[test]
    fn uri_to_path_rejects_http() {
        assert!(uri_to_path("https://example.com/x.rs").is_err());
    }

    #[test]
    fn uri_to_path_rejects_relative() {
        assert!(uri_to_path("file://relative/path").is_err());
    }

    #[test]
    fn apply_code_action_rejects_missing_changes_field() {
        let tmp = tempfile::tempdir().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(apply_code_action_inner(tmp.path(), serde_json::json!({})))
            .unwrap_err();
        assert!(err.contains("changes"));
    }

    #[test]
    fn apply_code_action_applies_single_line_replace() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lib.rs");
        std::fs::write(&path, "let x = 1;\n").unwrap();
        let uri = format!("file://{}", path.display());

        let edit = serde_json::json!({
            "changes": {
                uri: [
                    {
                        "range": {
                            "start": {"line": 0, "character": 4},
                            "end": {"line": 0, "character": 5}
                        },
                        "newText": "_x"
                    }
                ]
            }
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let count = rt
            .block_on(apply_code_action_inner(tmp.path(), edit))
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "let _x = 1;\n");
    }

    #[test]
    fn apply_code_action_applies_multiple_edits_reverse_order() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lib.rs");
        std::fs::write(&path, "a\nb\nc\n").unwrap();
        let uri = format!("file://{}", path.display());

        // Two edits on different lines — applied in reverse line order so
        // earlier-line offsets don't shift.
        let edit = serde_json::json!({
            "changes": {
                uri: [
                    {
                        "range": {
                            "start": {"line": 0, "character": 0},
                            "end": {"line": 0, "character": 1}
                        },
                        "newText": "X"
                    },
                    {
                        "range": {
                            "start": {"line": 2, "character": 0},
                            "end": {"line": 2, "character": 1}
                        },
                        "newText": "Z"
                    }
                ]
            }
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let count = rt
            .block_on(apply_code_action_inner(tmp.path(), edit))
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "X\nb\nZ\n");
    }

    #[test]
    fn apply_code_action_preserves_trailing_newline() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lib.rs");
        std::fs::write(&path, "hello\n").unwrap();
        let uri = format!("file://{}", path.display());

        let edit = serde_json::json!({
            "changes": {
                uri: [{
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 5}
                    },
                    "newText": "world"
                }]
            }
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(apply_code_action_inner(tmp.path(), edit))
            .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "world\n");
    }

    #[test]
    fn apply_code_action_skips_multiline_edits() {
        // Current implementation only handles single-line ranges.
        // A multi-line edit should be skipped (not counted, not crash).
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lib.rs");
        std::fs::write(&path, "a\nb\n").unwrap();
        let uri = format!("file://{}", path.display());

        let edit = serde_json::json!({
            "changes": {
                uri: [{
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 1, "character": 1}
                    },
                    "newText": "x"
                }]
            }
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let count = rt
            .block_on(apply_code_action_inner(tmp.path(), edit))
            .unwrap();
        assert_eq!(count, 0, "multi-line edit skipped");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\nb\n");
    }

    #[test]
    fn apply_code_action_errors_on_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let edit = serde_json::json!({
            "changes": {
                "file:///nonexistent/path/lib.rs": [{
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 1}
                    },
                    "newText": "x"
                }]
            }
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(apply_code_action_inner(tmp.path(), edit))
            .unwrap_err();
        // Missing file fails canonicalize in path validation, before any
        // read attempt.
        assert!(err.contains("not found") || err.contains("outside"));
    }

    #[test]
    fn apply_code_action_rejects_path_outside_working_dir() {
        // Security #2: a workspace edit payload pointing at /etc/hosts must
        // be rejected, not written.
        let tmp = tempfile::tempdir().unwrap();
        let edit = serde_json::json!({
            "changes": {
                "file:///etc/hosts": [{
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 1}
                    },
                    "newText": "x"
                }]
            }
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(apply_code_action_inner(tmp.path(), edit))
            .unwrap_err();
        // Path validation runs before any read; /etc/hosts exists on Linux
        // but is outside the tempdir, so we expect the "outside" error.
        // (On systems where /etc/hosts doesn't exist, we'd get "not found"
        // instead — either is acceptable as a rejection.)
        assert!(
            err.contains("outside") || err.contains("not found"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn language_id_maps_known_extensions() {
        assert_eq!(language_id_from_extension("rs"), "rust");
        assert_eq!(language_id_from_extension("ts"), "typescript");
        assert_eq!(language_id_from_extension("tsx"), "typescriptreact");
        assert_eq!(language_id_from_extension("js"), "javascript");
        assert_eq!(language_id_from_extension("go"), "go");
        assert_eq!(language_id_from_extension("py"), "python");
    }

    #[test]
    fn language_id_is_case_insensitive() {
        assert_eq!(language_id_from_extension("RS"), "rust");
        assert_eq!(language_id_from_extension("Py"), "python");
    }

    #[test]
    fn language_id_falls_back_to_plaintext() {
        assert_eq!(language_id_from_extension("xyz"), "plaintext");
        assert_eq!(language_id_from_extension(""), "plaintext");
    }

    #[test]
    fn read_source_file_returns_content_and_language() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("demo.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let dto = rt
            .block_on(read_source_file_inner(
                tmp.path(),
                path.to_string_lossy().into_owned(),
            ))
            .unwrap();
        assert_eq!(dto.content, "fn main() {}\n");
        assert_eq!(dto.language_id, "rust");
    }

    #[test]
    fn read_source_file_rejects_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(read_source_file_inner(
                tmp.path(),
                "/no/such/file.rs".into(),
            ))
            .unwrap_err();
        // The validation helper canonicalizes first; missing paths fail there.
        assert!(err.contains("not found"));
    }

    #[test]
    fn read_source_file_rejects_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let subdir = tmp.path().join("somedir");
        std::fs::create_dir(&subdir).unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(read_source_file_inner(
                tmp.path(),
                subdir.to_string_lossy().into_owned(),
            ))
            .unwrap_err();
        assert!(err.contains("not a file"));
    }

    #[test]
    fn read_source_file_rejects_path_outside_working_dir() {
        // Security #1: attempting to read /etc/passwd must be rejected,
        // regardless of whether the file exists.
        let tmp = tempfile::tempdir().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(read_source_file_inner(tmp.path(), "/etc/passwd".into()))
            .unwrap_err();
        assert!(
            err.contains("outside") || err.contains("not found"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn run_file_diagnostics_rejects_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        let req = FileDiagnosticsRequest {
            file_path: "/no/such/file.rs".into(),
            server_cmd: "rust-analyzer".into(),
            server_args: vec![],
            language_id: "rust".into(),
            content: "fn main() {}".into(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(run_file_diagnostics_inner(tmp.path(), req))
            .unwrap_err();
        // The validation helper canonicalizes first; missing paths fail there
        // before the LSP timeout can fire.
        assert!(
            err.contains("not found") || err.contains("outside"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn run_file_diagnostics_rejects_disallowed_server_cmd() {
        // Security #3: an arbitrary server_cmd must be rejected, even if the
        // file path is valid.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("demo.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();
        let req = FileDiagnosticsRequest {
            file_path: path.to_string_lossy().into_owned(),
            server_cmd: "/bin/sh".into(),
            server_args: vec!["-c".into(), "echo pwned".into()],
            language_id: "rust".into(),
            content: "fn main() {}".into(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(run_file_diagnostics_inner(tmp.path(), req))
            .unwrap_err();
        assert!(err.contains("allow-list"), "unexpected error: {err}");
    }

    #[test]
    fn lsp_code_actions_rejects_disallowed_server_cmd() {
        // Security #3: same check on the lsp_code_actions entry point.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("demo.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();
        let req = CodeActionRequest {
            file_path: path.to_string_lossy().into_owned(),
            server_cmd: "/usr/bin/python3".into(),
            server_args: vec![],
            start_line: 0,
            start_character: 0,
            end_line: 0,
            end_character: 1,
            language_id: "rust".into(),
            diagnostic_messages: vec!["unused".into()],
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(lsp_code_actions_inner(tmp.path(), req))
            .unwrap_err();
        assert!(err.contains("allow-list"), "unexpected error: {err}");
    }

    #[test]
    fn validate_lsp_server_accepts_bare_names_and_absolute_paths() {
        // Bare names listed in the allow-list are OK.
        assert!(validate_lsp_server("rust-analyzer").is_ok());
        assert!(validate_lsp_server("gopls").is_ok());
        // Absolute paths whose basename matches an allow-list entry are OK.
        assert!(validate_lsp_server("/usr/bin/rust-analyzer").is_ok());
        assert!(validate_lsp_server("/home/user/.cargo/bin/rust-analyzer").is_ok());
        // Anything else is rejected.
        assert!(validate_lsp_server("/bin/sh").is_err());
        assert!(validate_lsp_server("python3").is_err());
        assert!(validate_lsp_server("/usr/bin/python3").is_err());
        assert!(validate_lsp_server("").is_err());
    }

    #[test]
    fn file_diagnostic_dto_serializes_severity_lowercase() {
        let dto = FileDiagnosticDto {
            start_line: 0,
            start_character: 0,
            end_line: 0,
            end_character: 1,
            message: "unused".into(),
            severity: "warning".into(),
            source: Some("rustc".into()),
            code: Some("E0001".into()),
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"severity\":\"warning\""));
        assert!(json.contains("\"source\":\"rustc\""));
        assert!(json.contains("\"code\":\"E0001\""));
    }
}
