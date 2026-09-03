//! Slash-command backends — the desktop counterparts of the REPL's session
//! commands that have no other Tauri surface yet:
//!   /context → [`get_session_context_stats`] (tokens used vs context window)
//!   /diff    → [`get_session_git_diff`] (working-tree diff of the session's
//!              working directory)
//! /cost lives in `commands_usage` (it owns the ledger), /export and the
//! navigation commands are pure frontend.
//!
//! These are read-only diagnostics: nothing here mutates the session, the
//! L0 log, or the working tree.

use std::path::Path;
use std::process::Command;

use serde::Serialize;

use shannon_core::query_engine::QueryEngine;
use shannon_engine::api::client::LlmClient;
use shannon_engine::permissions::PermissionManager;
use shannon_engine::state::StateManager;

use crate::commands::AppState;

/// Context-window usage of one session, for the /context slash command.
#[derive(Debug, Clone, Serialize)]
pub struct SessionContextStats {
    /// CJK-aware estimate of the projected conversation (incl. system prompt),
    /// the same estimator that drives the engine's compression threshold.
    pub estimated_tokens: usize,
    /// `None` when the window is genuinely unknown (no override, no live
    /// Ollama num_ctx, model absent from the registry) — the UI renders
    /// "unknown" instead of a fabricated fallback.
    pub context_window: Option<usize>,
}

/// Build a throwaway engine bound to `session_id` and restore its L0
/// projection. Reuses the engine stashed on the session when present (it
/// already carries the resolved system prompt / context-window overrides);
/// otherwise constructs a minimal one — the client is never contacted, the
/// engine is only consulted for its local estimators.
async fn restored_engine(state: &AppState, session_id: uuid::Uuid) -> Result<QueryEngine, String> {
    let session = state
        .registry
        .get(crate::session_registry::SessionKey(session_id))
        .ok_or_else(|| format!("unknown session {session_id}"))?;

    let stashed = session.query_engine.lock().await.clone();
    let mut engine = match stashed {
        Some(engine) => engine,
        None => {
            let client_config = state.client_config.read().await.clone();
            let client = LlmClient::new(client_config);
            QueryEngine::with_defaults_arc(
                client,
                state.tools.clone(),
                PermissionManager::new(),
                StateManager::new(),
            )
        }
    };
    engine.set_session_id(session_id);
    engine
        .restore_session(session_id)
        .map_err(|e| format!("failed to project session history: {e}"))?;
    Ok(engine)
}

/// `/context` — token estimate + context window for the session's history.
#[tauri::command]
pub async fn get_session_context_stats(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<SessionContextStats, String> {
    let uuid = uuid::Uuid::parse_str(&session_id)
        .map_err(|_| format!("invalid session id {session_id}"))?;
    let engine = restored_engine(&state, uuid).await?;
    Ok(SessionContextStats {
        estimated_tokens: engine.estimate_conversation_tokens(),
        context_window: engine.resolved_context_window_opt(),
    })
}

/// One file's `--numstat` row (binary files report `None` counts).
#[derive(Debug, Clone, Serialize)]
pub struct GitDiffFile {
    pub path: String,
    pub insertions: u64,
    pub deletions: u64,
}

/// Whole-working-tree diff for the /diff slash command.
#[derive(Debug, Clone, Serialize)]
pub struct GitDiffSummary {
    pub is_repo: bool,
    pub files: Vec<GitDiffFile>,
    /// Unified patch, capped at `MAX_PATCH_BYTES` (larger diffs are
    /// truncated; the UI says so instead of silently clipping).
    pub patch: String,
    pub truncated: bool,
}

/// Patch size cap — a chat-renderable diff, not a git bundle.
const MAX_PATCH_BYTES: usize = 200_000;

fn git_output(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parse `git diff HEAD --numstat` rows: `<ins>\t<del>\t<path>`. Binary
/// entries print `-` for the counts and are kept with zero counts so the
/// file still shows up in the summary; rename rows keep their `{} => {}`
/// path syntax verbatim.
fn parse_numstat(output: &str) -> Vec<GitDiffFile> {
    let count = |s: Option<&str>| s.and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let insertions = count(parts.next());
            let deletions = count(parts.next());
            let path = parts.next()?.to_string();
            Some(GitDiffFile {
                path,
                insertions,
                deletions,
            })
        })
        .collect()
}

/// `/diff` — uncommitted changes (`git diff HEAD`) in the session's working
/// directory. Not a repository (or git unavailable) surfaces as
/// `is_repo: false` rather than an error so the UI can show a calm notice.
#[tauri::command]
pub async fn get_session_git_diff(working_dir: String) -> Result<GitDiffSummary, String> {
    let dir = Path::new(&working_dir).to_path_buf();
    if !dir.is_dir() {
        return Err(format!("working directory does not exist: {working_dir}"));
    }
    // Spawn the short-lived git processes off the async runtime's workers.
    let summary = tokio::task::spawn_blocking(move || {
        let inside = git_output(&dir, &["rev-parse", "--is-inside-work-tree"])
            .map(|s| s.trim() == "true")
            .unwrap_or(false);
        if !inside {
            return GitDiffSummary {
                is_repo: false,
                files: Vec::new(),
                patch: String::new(),
                truncated: false,
            };
        }
        let files = git_output(&dir, &["diff", "HEAD", "--numstat"])
            .map(|s| parse_numstat(&s))
            .unwrap_or_default();
        let (patch, truncated) = match git_output(&dir, &["diff", "HEAD"]) {
            Some(full) => {
                if full.len() > MAX_PATCH_BYTES {
                    let mut cut = MAX_PATCH_BYTES;
                    while !full.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    (full[..cut].to_string(), true)
                } else {
                    (full, false)
                }
            }
            None => (String::new(), false),
        };
        GitDiffSummary {
            is_repo: true,
            files,
            patch,
            truncated,
        }
    })
    .await
    .map_err(|e| format!("git diff task failed: {e}"))?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numstat_rows_parse_into_files() {
        let out = "12\t3\tsrc/lib.rs\n0\t0\timg.png\n-\t-\tbinary.bin\n";
        let files = parse_numstat(out);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "src/lib.rs");
        assert_eq!((files[0].insertions, files[0].deletions), (12, 3));
        assert_eq!((files[1].insertions, files[1].deletions), (0, 0));
        assert_eq!(files[2].path, "binary.bin");
        assert_eq!((files[2].insertions, files[2].deletions), (0, 0));
    }

    #[test]
    fn empty_numstat_yields_no_files() {
        assert!(parse_numstat("").is_empty());
    }

    #[test]
    fn diff_of_non_repo_reports_not_a_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let summary = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(get_session_git_diff_inner(tmp.path()));
        assert!(!summary.is_repo);
        assert!(summary.files.is_empty());
        assert!(summary.patch.is_empty());
    }

    #[test]
    fn diff_of_git_repo_lists_changed_files() {
        if !Command::new("git")
            .args(["--version"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            eprintln!("git unavailable — skipping");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let run = |args: &[&str]| {
            Command::new("git")
                .current_dir(dir)
                .args(args)
                .output()
                .unwrap()
        };
        // Seed the repo with a committed file, then modify it so `HEAD`
        // exists and the working tree actually differs from it.
        std::fs::write(dir.join("a.txt"), "base\n").unwrap();
        run(&["init", "-q"]);
        run(&["-c", "user.email=t@t", "-c", "user.name=t", "add", "-A"]);
        run(&[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "init",
        ]);
        std::fs::write(dir.join("a.txt"), "line1\nline2\n").unwrap();

        let summary = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(get_session_git_diff_inner(dir));
        assert!(summary.is_repo);
        assert_eq!(summary.files.len(), 1);
        assert_eq!(summary.files[0].path, "a.txt");
        assert_eq!(summary.files[0].insertions, 2);
        assert!(summary.patch.contains("diff --git"));
        assert!(!summary.truncated);
    }

    // The command is `async` for the Tauri runtime; tests drive the inner
    // future directly with their own runtime.
    async fn get_session_git_diff_inner(dir: &Path) -> GitDiffSummary {
        let working_dir = dir.to_string_lossy().into_owned();
        super::get_session_git_diff(working_dir).await.unwrap()
    }
}
