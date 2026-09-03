//! `/rewind` for the desktop chat — per-turn checkpoints, conversation
//! truncation, and file revert.
//!
//! Recording side (W6-2 B.2 desktop equivalent): `send_message` collects the
//! file paths its `write`/`edit` tool calls touched, captures content
//! snapshots via `FileHistoryManager`, and records a `TurnCheckpoint` after
//! each completed query. `commands_agents`-style helpers live here so
//! `commands.rs` stays focused on the query loop.
//!
//! Rewind side: `rewind_session(session_id, turn_index)` drops turn
//! `turn_index` and everything after it — the L0 log is truncated via
//! `SessionStore::truncate_to_turn`, touched files are reverted with
//! `FileHistoryManager::rewind_before_turn`, the checkpoint list is popped
//! back, and the in-memory message buffer is trimmed to match. Returns the
//! surviving conversation so the UI can swap it in.

use crate::commands::AppState;
use shannon_core::checkpoint::{Checkpoint, CheckpointManager};
use shannon_tools::FileHistoryManager;
use std::path::{Path, PathBuf};

/// Tool names whose execution mutates `file_path` (case-insensitive; the
/// registry registers capitalized names, models emit either case).
fn is_file_write_tool(tool_name: &str) -> bool {
    matches!(
        tool_name.to_ascii_lowercase().as_str(),
        "write" | "edit" | "create" | "multiedit" | "notebookedit"
    )
}

/// Pull the target path out of a file-mutating tool call's input.
pub(crate) fn mutated_file_path(tool_name: &str, tool_input: &serde_json::Value) -> Option<String> {
    if !is_file_write_tool(tool_name) {
        return None;
    }
    tool_input
        .get("file_path")
        .or_else(|| tool_input.get("path"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Resolve a turn-tracker file key to a readable path. Relative keys are
/// resolved against the session working directory so snapshot lookups match
/// what the tools wrote (mirrors the REPL's `capture_turn_snapshots_with`).
fn resolve_snapshot_path(key: &str, working_dir: &Path) -> PathBuf {
    let p = Path::new(key);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        working_dir.join(p)
    }
}

/// Record one completed turn: capture end-of-turn content snapshots for every
/// file the turn touched, then append the checkpoint. Fails soft throughout —
/// checkpoint/rewind is a convenience layer and must never break the chat.
pub(crate) fn record_turn(
    session_id: &str,
    turn_index: usize,
    files: &[String],
    prompt: &str,
    working_dir: &Path,
) {
    let prompt_preview: Option<String> = if prompt.chars().count() > 80 {
        Some(format!("{}...", prompt.chars().take(77).collect::<String>()))
    } else {
        Some(prompt.to_string())
    };

    if let Some(config) = shannon_tools::FileHistoryConfig::from_env() {
        let mut manager = FileHistoryManager::new(config);
        for file in files {
            let fs_path = resolve_snapshot_path(file, working_dir);
            // Bound memory like the REPL capture: skip oversized, missing, or
            // non-UTF-8 files rather than reading them fully in.
            let Ok(meta) = std::fs::metadata(&fs_path) else {
                continue;
            };
            if meta.len() > shannon_tools::file::MAX_SNAPSHOT_BYTES {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&fs_path) else {
                continue;
            };
            if let Err(e) = manager.record_turn_snapshot(Path::new(file), &content, turn_index) {
                tracing::debug!("turn snapshot skipped for {fs_path:?}: {e}");
            }
        }
    }

    let manager = CheckpointManager::for_session(session_id);
    manager.record_turn(
        turn_index,
        Checkpoint {
            description: format!("turn {}", turn_index + 1),
            timestamp: chrono::Utc::now().timestamp(),
        },
        files.to_vec(),
        prompt_preview,
    );
}

/// One rewindable checkpoint as surfaced to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CheckpointInfo {
    pub turn_index: usize,
    pub timestamp: i64,
    pub description: String,
    pub files_changed: Vec<String>,
    pub prompt_preview: Option<String>,
}

#[tauri::command]
pub async fn list_checkpoints(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<Vec<CheckpointInfo>, String> {
    // Validate the session id shape; checkpoints may legitimately be empty.
    let uuid = uuid::Uuid::parse_str(&session_id).map_err(|e| format!("Invalid UUID: {e}"))?;
    let _ = state.l0_store().load(&uuid).map_err(|e| e.to_string())?;
    let manager = CheckpointManager::for_session(&session_id);
    Ok(manager
        .list_checkpoints()
        .into_iter()
        .map(|cp| CheckpointInfo {
            turn_index: cp.turn_index,
            timestamp: cp.checkpoint.timestamp,
            description: cp.checkpoint.description,
            files_changed: cp.files_changed,
            prompt_preview: cp.prompt_preview,
        })
        .collect())
}

/// Rewind `session_id` to before `turn_index`: drop that turn and everything
/// after it from the log, the checkpoint list, the in-memory buffer, and the
/// working tree (files the dropped turns touched are restored to their
/// state before the rewind point, or deleted when the session created them).
/// Returns the surviving conversation, shaped exactly like `load_session`.
#[tauri::command]
pub async fn rewind_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    session_id: String,
    turn_index: usize,
) -> Result<Vec<crate::commands::ChatMessage>, String> {
    let uuid = uuid::Uuid::parse_str(&session_id).map_err(|e| format!("Invalid UUID: {e}"))?;

    // Refuse to rewind a session while a query is in flight on it.
    {
        let session = state.registry.get(crate::session_registry::SessionKey(uuid));
        if let Some(session) = session {
            let querying = session.querying.lock().await;
            if *querying {
                return Err("A query is in progress — wait for it to finish before rewinding".into());
            }
        }
    }

    let manager = CheckpointManager::for_session(&session_id);
    let checkpoints = manager.list_checkpoints();
    let total_turns = checkpoints.len();
    if turn_index >= total_turns {
        return Err(format!(
            "Nothing to rewind: session has {total_turns} recorded turn(s)"
        ));
    }

    // 1) Files touched by the dropped turns → restore pre-rewind state.
    let working_dir = crate::commands_agents::resolve_working_dir(&state).await;
    let mut files: Vec<String> = checkpoints
        .iter()
        .filter(|cp| cp.turn_index >= turn_index)
        .flat_map(|cp| cp.files_changed.iter().cloned())
        .collect();
    files.sort();
    files.dedup();
    if !files.is_empty() {
        if let Some(config) = shannon_tools::FileHistoryConfig::from_env() {
            let mut history = FileHistoryManager::new(config);
            for file in &files {
                let key = Path::new(file);
                let fs_path = resolve_snapshot_path(file, &working_dir);
                match history.rewind_before_turn(key, turn_index) {
                    Ok(shannon_tools::RewindAction::Restore(content)) => {
                        if let Err(e) = std::fs::write(&fs_path, content) {
                            eprintln!("rewind: failed to restore {fs_path:?}: {e}");
                        }
                    }
                    Ok(shannon_tools::RewindAction::Delete) => {
                        if fs_path.exists() {
                            if let Err(e) = std::fs::remove_file(&fs_path) {
                                eprintln!("rewind: failed to delete {fs_path:?}: {e}");
                            }
                        }
                    }
                    Ok(shannon_tools::RewindAction::NoChange) => {}
                    Err(e) => eprintln!("rewind: no history for {file}: {e}"),
                }
            }
        }
    }

    // 2) Truncate the L0 log — the authoritative conversation record.
    let dropped = state
        .l0_store()
        .truncate_to_turn(&uuid, turn_index)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session not found: {session_id}"))?;

    // 3) Pop the checkpoints past the rewind point.
    while manager.len() > turn_index {
        manager.discard_last();
    }
    let _ = dropped;

    // 4) Trim the in-memory buffer of the live session (if loaded).
    {
        let session = state.registry.get(crate::session_registry::SessionKey(uuid));
        if let Some(session) = session {
            let mut messages = session.messages.lock().await;
            let mut seen_users = 0usize;
            let mut cut_at = messages.len();
            for (i, msg) in messages.iter().enumerate() {
                if msg.role == "user" {
                    if seen_users == turn_index {
                        cut_at = i;
                        break;
                    }
                    seen_users += 1;
                }
            }
            messages.truncate(cut_at);
        }
    }

    crate::commands_sessions::load_session(state, app_handle, session_id).await
}
