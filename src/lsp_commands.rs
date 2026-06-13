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

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use tokio::time::timeout;

const LSP_TIMEOUT: Duration = Duration::from_secs(15);

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
pub async fn lsp_code_actions(req: CodeActionRequest) -> Result<CodeActionsResponse, String> {
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
    let doc_uri = abs
        .to_url()
        .map_err(|e| format!("doc url: {e}"))?;

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
#[tauri::command]
pub async fn apply_code_action(edit: serde_json::Value) -> Result<u32, String> {
    // Parse minimal shape: { changes?: { [uri]: TextEdit[] } }
    let changes = edit
        .get("changes")
        .and_then(|c| c.as_object())
        .ok_or_else(|| "no `changes` field in workspace edit".to_string())?;

    let mut applied = 0u32;
    for (uri, edits) in changes {
        let path = uri_to_path(uri)?;
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
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

use url::Url;
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
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(apply_code_action(serde_json::json!({})))
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
        let count = rt.block_on(apply_code_action(edit)).unwrap();
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
        let count = rt.block_on(apply_code_action(edit)).unwrap();
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
        rt.block_on(apply_code_action(edit)).unwrap();
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
        let count = rt.block_on(apply_code_action(edit)).unwrap();
        assert_eq!(count, 0, "multi-line edit skipped");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\nb\n");
    }

    #[test]
    fn apply_code_action_errors_on_missing_file() {
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
        let err = rt.block_on(apply_code_action(edit)).unwrap_err();
        assert!(err.contains("read"));
    }
}
