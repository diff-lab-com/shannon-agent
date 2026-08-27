//! Session lifecycle Tauri commands (extracted from `commands.rs`).
//!
//! Second step of the commands.rs decomposition (R2-A3 / P1.1). The session
//! cluster is the largest cohesive domain — new/list/search/load/export/
//! switch/delete/rename/duplicate/branch + working_dir. StateManager-backed.
//!
//! P0-4: per-session mutable state (`messages`, `querying`,
//! `cancellation_token`, `current_session_id`) lives in `state.registry`
//! keyed by `SessionKey`. The display list (`state.sessions: Vec<SessionMeta>`)
//! stays on `AppState` because the UI consumes it as a flat list keyed by
//! UUID strings.

use crate::commands::{AppState, ChatMessage, SessionMeta, chrono_timestamp};
use crate::scheduled_commands::TaskWorktreeDto;
use crate::session_registry::SessionKey;
use crate::{config, events, events::event_names};
use tauri::Emitter;

/// Create a new session and return its UUID.
///
/// P0-4: materialises the new session in `state.registry`, clears the
/// (about-to-be-stale) message buffer, and promotes the new key to active.
#[tauri::command]
pub async fn new_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4();
    let id_str = id.to_string();
    let title = format!("Session {}", id_str.split('-').next().unwrap_or(&id_str));
    let now = chrono_timestamp();

    // Create the session's L0 log (§4.6): opening + closing a fresh writer
    // records the `session/start` row, which is all a brand-new session has.
    let model = state.client_config.read().await.model.clone();
    shannon_core::session_log::SessionTee::open_in_container(
        state.l0_store().container(),
        &id_str,
        &model,
        None,
    )
    .close();

    // Create session metadata
    let session_meta = SessionMeta {
        id: id_str.clone(),
        title: title.clone(),
        created_at: now,
        message_count: 0,
        working_dir: None,
        parent_id: None,
        branch_point: None,
    };

    // Add to sessions list
    {
        let mut sessions = state.sessions.lock().await;
        sessions.push(session_meta);
    }

    // P0-4: register in the per-session registry and promote to active.
    state.registry.insert(id);
    state.registry.set_active(SessionKey(id));

    // Clear messages for new session
    if let Some(session) = state.registry.get(SessionKey(id)) {
        let mut messages = session.messages.lock().await;
        messages.clear();
    }

    // Emit sessions updated event
    let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());

    Ok(id_str)
}

/// Tier-1 auto-title: derive a session title from the first user message.
///
/// Deterministic truncation — no LLM call. First line only (the session rail
/// must never show newlines), trimmed, capped at 50 chars with an ellipsis.
/// Returns empty for whitespace-only input; callers treat that as "keep the
/// placeholder".
pub(crate) fn derive_title_from_message(message: &str) -> String {
    const MAX_CHARS: usize = 50;
    let first_line = message.lines().next().unwrap_or("").trim();
    if first_line.chars().count() <= MAX_CHARS {
        first_line.to_string()
    } else {
        let truncated: String = first_line.chars().take(MAX_CHARS).collect();
        format!("{truncated}…")
    }
}

/// Promote the first user message of a still-placeholder-titled session to
/// its title (Tier-1 auto-title, 2026-08-26; ed-approved).
///
/// Fires only while the title is the generated `Session {uuid-prefix}`
/// placeholder — a user rename writes a real title and is never
/// overwritten. Mirrors `rename_session`'s persistence path (sessions vec +
/// StateManager save with `Some(title)`); later auto-saves pass
/// `title: None`, which `StateManager::save_session` backfills from disk,
/// so the derived title survives every subsequent save.
pub(crate) async fn auto_title_from_first_message(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    session_id: uuid::Uuid,
    message: &str,
) {
    let title = derive_title_from_message(message);
    if title.is_empty() {
        return;
    }
    let id_str = session_id.to_string();

    let mut sessions = state.sessions.lock().await;
    let Some(session) = sessions.iter_mut().find(|s| s.id == id_str) else {
        return;
    };
    if !session.title.starts_with("Session ") {
        return;
    }
    session.title = title.clone();

    // Persist the curated title in the session sidecar (§4.6) so it is
    // durable even if the app closes before the query completes. The
    // conversation itself is already continuous in events.jsonl.
    drop(sessions);
    let _ = state.l0_store().save_sidecar(
        &session_id,
        &shannon_core::session_log::SessionSidecar {
            title: Some(title),
            ..Default::default()
        },
    );

    let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());
}

/// List all sessions.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn list_sessions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<events::SessionInfo>, String> {
    let sessions = state.sessions.lock().await;
    let result: Vec<events::SessionInfo> = sessions
        .iter()
        .map(|s| events::SessionInfo {
            id: s.id.clone(),
            title: s.title.clone(),
            created_at: s.created_at,
            message_count: s.message_count,
            working_dir: s.working_dir.clone(),
            parent_id: s.parent_id.clone(),
            branch_point: s.branch_point,
        })
        .collect();
    Ok(result)
}

/// Search sessions by title substring or message content.
///
/// Title matches rank first; content matches fill the rest. Only the first
/// `CONTENT_SCAN_LIMIT` sessions without a title match have their messages
/// loaded, so cost stays bounded per keystroke.
#[tauri::command]
pub async fn search_sessions(
    state: tauri::State<'_, AppState>,
    query: String,
) -> Result<Vec<events::SessionInfo>, String> {
    const CONTENT_SCAN_LIMIT: usize = 200;

    let query_lower = query.to_lowercase();
    if query_lower.is_empty() {
        return Ok(Vec::new());
    }

    let sessions = state.sessions.lock().await;
    let mut title_matches: Vec<events::SessionInfo> = Vec::new();
    let mut content_matches: Vec<events::SessionInfo> = Vec::new();

    for s in sessions.iter() {
        let info = || events::SessionInfo {
            id: s.id.clone(),
            title: s.title.clone(),
            created_at: s.created_at,
            message_count: s.message_count,
            working_dir: s.working_dir.clone(),
            parent_id: s.parent_id.clone(),
            branch_point: s.branch_point,
        };

        if s.title.to_lowercase().contains(&query_lower) {
            title_matches.push(info());
            continue;
        }

        if content_matches.len() + title_matches.len() >= CONTENT_SCAN_LIMIT {
            continue;
        }

        if let Ok(uuid) = uuid::Uuid::parse_str(&s.id) {
            // Full-text search on L0 events — the transcript-search successor.
            let hit = state
                .l0_store()
                .search_session(&uuid, &query_lower)
                .map(|hits| !hits.is_empty())
                .unwrap_or(false);
            if hit {
                content_matches.push(info());
            }
        }
    }

    title_matches.extend(content_matches);
    Ok(title_matches)
}

/// Load a session by ID.
#[tauri::command]
pub async fn load_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<Vec<ChatMessage>, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;

    // Load by projecting the session's L0 log.
    let session_data = state
        .l0_store()
        .load(&session_uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session not found: {id}"))?;

    // Convert shannon_core Messages to ChatMessages
    let messages: Vec<ChatMessage> = session_data
        .messages
        .into_iter()
        .map(|msg| ChatMessage {
            role: msg.role,
            content: match msg.content {
                shannon_engine::api::MessageContent::Text(t) => t,
                shannon_engine::api::MessageContent::Blocks(blocks) => {
                    // For blocks, extract text content
                    blocks
                        .iter()
                        .filter_map(|b| match b {
                            shannon_engine::api::ContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            },
            timestamp: chrono_timestamp(),
            file_attachments: None,
        })
        .collect();

    // Update current messages
    let session = state.registry.get_or_create(SessionKey(session_uuid));
    {
        let mut current_messages = session.messages.lock().await;
        *current_messages = messages.clone();
    }

    // P0-4: promote to active session.
    state.registry.set_active(SessionKey(session_uuid));

    // Emit session loaded event
    let event_messages: Vec<events::ChatMessage> = messages
        .iter()
        .map(|m| events::ChatMessage {
            role: m.role.clone(),
            content: m.content.clone(),
            timestamp: m.timestamp,
        })
        .collect();
    let _ = app_handle.emit(
        event_names::SESSION_LOADED,
        events::SessionLoaded {
            messages: event_messages,
        },
    );

    Ok(messages)
}

/// Export a session to Markdown or JSON format.
#[tauri::command]
pub async fn export_session(
    state: tauri::State<'_, AppState>,
    id: String,
    format: String,
) -> Result<String, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;

    let session_data = state
        .l0_store()
        .load(&session_uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session not found: {id}"))?;

    let title = session_data
        .metadata
        .title
        .as_deref()
        .unwrap_or("Untitled Session");

    match format.as_str() {
        "markdown" | "md" => {
            let mut md = format!("# {title}\n\n");
            md.push_str(&format!(
                "Exported: {}\n\n---\n\n",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            ));
            for msg in &session_data.messages {
                let role_label = match msg.role.as_str() {
                    "user" => "**You**",
                    "assistant" => "**Assistant**",
                    "system" => "**System**",
                    other => &format!("**{other}**"),
                };
                let content = match &msg.content {
                    shannon_engine::api::MessageContent::Text(t) => t.clone(),
                    shannon_engine::api::MessageContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| match b {
                            shannon_engine::api::ContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                md.push_str(&format!("### {role_label}\n\n{content}\n\n---\n\n"));
            }
            Ok(md)
        }
        "json" => {
            let messages: Vec<serde_json::Value> = session_data
                .messages
                .iter()
                .map(|msg| {
                    let content = match &msg.content {
                        shannon_engine::api::MessageContent::Text(t) => t.clone(),
                        shannon_engine::api::MessageContent::Blocks(blocks) => blocks
                            .iter()
                            .filter_map(|b| match b {
                                shannon_engine::api::ContentBlock::Text { text } => {
                                    Some(text.clone())
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                    };
                    serde_json::json!({
                        "role": msg.role,
                        "content": content,
                    })
                })
                .collect();
            let export = serde_json::json!({
                "id": id,
                "title": title,
                "exported_at": chrono::Local::now().to_rfc3339(),
                "message_count": messages.len(),
                "messages": messages,
            });
            serde_json::to_string_pretty(&export).map_err(|e| e.to_string())
        }
        _ => Err(format!(
            "Unsupported format: {format}. Use 'markdown' or 'json'."
        )),
    }
}

/// Switch to a different session, saving the current one first.
#[tauri::command]
pub async fn switch_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<Vec<ChatMessage>, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;

    // (§4.6) No explicit save needed before switching: every turn is already
    // durable in events.jsonl via the engine tee.

    // Load new session by projecting its L0 log.
    let messages = match state
        .l0_store()
        .load(&session_uuid)
        .map_err(|e| e.to_string())?
    {
        Some(data) => data
            .messages
            .into_iter()
            .map(|msg| ChatMessage {
                role: msg.role,
                content: match msg.content {
                    shannon_engine::api::MessageContent::Text(t) => t,
                    shannon_engine::api::MessageContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| match b {
                            shannon_engine::api::ContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                },
                timestamp: chrono_timestamp(),
                file_attachments: None,
            })
            .collect(),
        None => Vec::new(),
    };

    // Update state (P0-4: register the new session and promote to active).
    state.registry.insert(session_uuid);
    let new_session = state.registry.get_or_create(SessionKey(session_uuid));
    {
        let mut msgs = new_session.messages.lock().await;
        *msgs = messages.clone();
    }
    state.registry.set_active(SessionKey(session_uuid));

    // Restore working_dir from session metadata if present.
    {
        let sessions = state.sessions.lock().await;
        if let Some(meta) = sessions.iter().find(|s| s.id == id) {
            if let Some(ref wd) = meta.working_dir {
                let _ = std::env::set_current_dir(wd);
                let mut desktop_cfg = state.desktop_config.write().await;
                desktop_cfg.working_dir = Some(wd.clone());
                let _ = app_handle.emit(
                    event_names::CONFIG_UPDATED,
                    events::ConfigUpdatedPayload {
                        key: "working_dir".into(),
                        value: wd.clone(),
                    },
                );
            }
        }
    }

    // Emit session loaded event
    let event_messages: Vec<events::ChatMessage> = messages
        .iter()
        .map(|m| events::ChatMessage {
            role: m.role.clone(),
            content: m.content.clone(),
            timestamp: m.timestamp,
        })
        .collect();
    let _ = app_handle.emit(
        event_names::SESSION_LOADED,
        events::SessionLoaded {
            messages: event_messages,
        },
    );

    Ok(messages)
}

/// Set working directory for a session. Updates in-memory metadata, the
/// process cwd, and the persisted desktop config. Pass an empty string to
/// reset to the Shannon home directory.
#[tauri::command]
pub async fn set_session_working_dir(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
    path: String,
) -> Result<(), String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;
    let wd = if path.trim().is_empty() {
        None
    } else {
        let canonical =
            std::fs::canonicalize(&path).map_err(|e| format!("Invalid path {path}: {e}"))?;
        Some(canonical.to_string_lossy().into_owned())
    };

    // Update session metadata
    {
        let mut sessions = state.sessions.lock().await;
        if let Some(meta) = sessions.iter_mut().find(|s| s.id == id) {
            meta.working_dir = wd.clone();
        }
    }

    // If this is the current session, switch process cwd + desktop config
    let current = state.registry.active_key();
    let is_current = current == Some(SessionKey(session_uuid));
    if is_current {
        if let Some(ref p) = wd {
            let _ = std::env::set_current_dir(p);
        }
        let mut desktop_cfg = state.desktop_config.write().await;
        desktop_cfg.working_dir = wd.clone();
        drop(desktop_cfg);
        let desktop_cfg = state.desktop_config.read().await;
        let _ = config::save_config(&desktop_cfg);
        let _ = app_handle.emit(
            event_names::CONFIG_UPDATED,
            events::ConfigUpdatedPayload {
                key: "working_dir".into(),
                value: wd.clone().unwrap_or_default(),
            },
        );
    }

    let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());
    Ok(())
}

/// Create an isolated git worktree for a session and bind it as the session's
/// working directory. Delegates to [`shannon_core::scheduled_worktree::create_for_task`]
/// — the same helper used by scheduled tasks — so session and task worktrees
/// live under the same base dir (`.shannon/scheduled-worktrees/` by default).
///
/// Safe to call repeatedly: if the worktree path already exists, the helper
/// returns the existing descriptor instead of erroring.
#[tauri::command]
pub async fn create_session_worktree(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
    title: String,
) -> Result<TaskWorktreeDto, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;

    // Verify session exists before creating worktree (avoid orphan worktrees)
    {
        let sessions = state.sessions.lock().await;
        let exists = sessions.iter().any(|s| s.id == id);
        if !exists {
            return Err(format!("Session not found: {id}"));
        }
    }

    let id_str = session_uuid.to_string();
    let base = shannon_core::scheduled_worktree::default_base_dir();
    let wt = shannon_core::scheduled_worktree::create_for_task(&id_str, &title, &base)
        .map_err(|e| e.to_string())?;
    let wt_path = wt.path.to_string_lossy().into_owned();

    // Update session metadata to point at the worktree
    {
        let mut sessions = state.sessions.lock().await;
        if let Some(meta) = sessions.iter_mut().find(|s| s.id == id_str) {
            meta.working_dir = Some(wt_path.clone());
        }
    }

    // If this is the current session, switch process cwd + desktop config
    let current = state.registry.active_key();
    if current == Some(SessionKey(session_uuid)) {
        let _ = std::env::set_current_dir(&wt_path);
        let mut desktop_cfg = state.desktop_config.write().await;
        desktop_cfg.working_dir = Some(wt_path.clone());
        drop(desktop_cfg);
        let desktop_cfg = state.desktop_config.read().await;
        let _ = config::save_config(&desktop_cfg);
        let _ = app_handle.emit(
            event_names::CONFIG_UPDATED,
            events::ConfigUpdatedPayload {
                key: "working_dir".into(),
                value: wt_path.clone(),
            },
        );
    }

    let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());
    Ok(wt.into())
}

/// Delete a session by ID. If the session had a bound worktree (working_dir
/// pointing inside the default worktree base), the worktree is removed too —
/// best-effort, logs failures but does not block session deletion.
#[tauri::command]
pub async fn delete_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<bool, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;

    // Capture working_dir before deleting so we can clean up worktree
    let working_dir = {
        let sessions = state.sessions.lock().await;
        sessions
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.working_dir.clone())
    };

    // Delete removes the session's whole L0 directory (log + sidecar).
    let deleted = state
        .l0_store()
        .delete(&session_uuid)
        .map_err(|e| e.to_string())?;

    if deleted {
        // Remove from sessions list
        {
            let mut sessions = state.sessions.lock().await;
            sessions.retain(|s| s.id != id);
        }

        // Best-effort worktree cleanup: if working_dir lives under the
        // default worktree base dir, remove the worktree. Failures are
        // logged but do not block session deletion — orphan worktrees can
        // be cleaned up later via prune_task_worktrees.
        if let Some(wd) = working_dir {
            let base = shannon_core::scheduled_worktree::default_base_dir();
            let wd_path = std::path::Path::new(&wd);
            if wd_path.starts_with(&base) {
                if let Err(e) = shannon_core::scheduled_worktree::remove(wd_path) {
                    tracing::warn!(
                        worktree = %wd,
                        error = %e,
                        "failed to remove worktree during session deletion;                          use prune_task_worktrees to clean up later"
                    );
                }
            }
        }

        // Emit sessions updated event
        let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());

        Ok(true)
    } else {
        Ok(false)
    }
}

/// Rename a session by ID.
#[tauri::command]
pub async fn rename_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
    title: String,
) -> Result<bool, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;

    // Update session metadata in sessions list
    let mut sessions = state.sessions.lock().await;
    if let Some(session) = sessions.iter_mut().find(|s| s.id == id) {
        session.title = title.clone();

        // Persist the curated title in the sidecar (§4.6); P0-4 note still
        // applies to the in-memory list above — the store never reads it.
        let _ = state.l0_store().save_sidecar(
            &session_uuid,
            &shannon_core::session_log::SessionSidecar {
                title: Some(title),
                ..Default::default()
            },
        );

        // Emit sessions updated event
        let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());

        Ok(true)
    } else {
        Ok(false)
    }
}

/// Duplicate a session by ID.
#[tauri::command]
pub async fn duplicate_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<events::SessionInfo, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;

    // Find original session
    let sessions = state.sessions.lock().await;
    let original_session = sessions
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("Session not found: {id}"))?;

    // Load original session data (projected from its L0 log)
    let session_data = state
        .l0_store()
        .load(&session_uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session data not found: {id}"))?;

    // Create the duplicate by replaying every event into a fresh log.
    let new_id = uuid::Uuid::new_v4();
    let new_id_str = new_id.to_string();
    let new_title = format!("Copy of {}", original_session.title);
    let now = chrono_timestamp();

    {
        use shannon_types::session_event::SessionEventBody;
        let events = state
            .l0_store()
            .read_events(&session_uuid)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let mut w = shannon_core::session_log::SessionLogWriter::open_layout(
            state.l0_store().container(),
            &new_id_str,
        )
        .map_err(|e| e.to_string())?;
        if events.is_empty() {
            // Keep empty duplicates listable: an explicit start row marks them.
            w.record(SessionEventBody::SessionStart(
                shannon_types::session_event::SessionStartPayload {
                    model: state.client_config.read().await.model.clone(),
                    provider: None,
                    cwd: None,
                    app_version: None,
                },
            ));
        }
        for event in events {
            w.record(event.body);
        }
        w.close().map_err(|e| e.to_string())?;
    }

    state
        .l0_store()
        .save_sidecar(
            &new_id,
            &shannon_core::session_log::SessionSidecar {
                title: Some(new_title.clone()),
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;

    // Add to sessions list
    let new_session_meta = SessionMeta {
        id: new_id_str.clone(),
        title: new_title.clone(),
        created_at: now,
        message_count: session_data.messages.len(),
        working_dir: None,
        parent_id: None,
        branch_point: None,
    };
    drop(sessions);
    {
        let mut sessions = state.sessions.lock().await;
        sessions.push(new_session_meta);
    }

    // Emit sessions updated event
    let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());

    Ok(events::SessionInfo {
        id: new_id_str,
        title: new_title,
        created_at: now,
        message_count: session_data.messages.len(),
        working_dir: None,
        parent_id: None,
        branch_point: None,
    })
}

/// Internal helper for branch_session (shared with tests).
pub(crate) async fn branch_session_internal(
    state: &AppState,
    app_handle: Option<&tauri::AppHandle>,
    parent_id: String,
    branch_point: usize,
) -> Result<events::SessionInfo, String> {
    let parent_uuid =
        uuid::Uuid::parse_str(&parent_id).map_err(|e| format!("Invalid UUID: {e}"))?;

    // Find parent session
    let sessions = state.sessions.lock().await;
    let parent_session = sessions
        .iter()
        .find(|s| s.id == parent_id)
        .ok_or_else(|| format!("Session not found: {parent_id}"))?;

    // Clone parent session data before dropping sessions
    let parent_title = parent_session.title.clone();
    let parent_working_dir = parent_session.working_dir.clone();

    // Load parent session data (projected from L0)
    let session_data = state
        .l0_store()
        .load(&parent_uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session data not found: {parent_id}"))?;

    // Create new session carrying messages up to AND INCLUDING the branch
    // point — the desktop convention (`take(branch_point + 1)`).
    let new_title = format!("Branch of {parent_title}");
    let now = chrono_timestamp();

    if branch_point >= session_data.messages.len() {
        return Err(format!(
            "Branch point {} out of bounds: session has {} messages (valid range: 0-{})",
            branch_point,
            session_data.messages.len(),
            session_data.messages.len().saturating_sub(1)
        ));
    }
    let keep_messages = branch_point + 1;

    let stored_branch = state
        .l0_store()
        .create_branch(&parent_uuid, keep_messages, Some(new_title.clone()))
        .map_err(|e| e.to_string())?;

    let branch_message_count = keep_messages;
    let _ = &session_data;

    // Drop sessions lock before re-acquiring for push
    drop(sessions);

    // Add to sessions list with parent/branch info
    let new_session_meta = SessionMeta {
        id: stored_branch.session_id.to_string(),
        title: new_title.clone(),
        created_at: now,
        message_count: branch_message_count,
        working_dir: parent_working_dir.clone(),
        parent_id: Some(parent_id.clone()),
        branch_point: Some(branch_point),
    };
    {
        let mut sessions = state.sessions.lock().await;
        sessions.push(new_session_meta);
    }

    // Emit sessions updated event
    if let Some(handle) = app_handle {
        let _ = handle.emit(event_names::SESSIONS_UPDATED, ());
    }

    Ok(events::SessionInfo {
        id: stored_branch.session_id.to_string(),
        title: new_title,
        created_at: now,
        message_count: branch_message_count,
        working_dir: parent_working_dir,
        parent_id: Some(parent_id),
        branch_point: Some(branch_point),
    })
}

/// Branch a session at a specific message index.
///
/// Creates a new session with messages up to (and including) the branch point,
/// copying the first N messages from the parent session. Sets parent_id and
/// branch_point to track the relationship.
#[tauri::command]
pub async fn branch_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    parent_id: String,
    branch_point: usize,
) -> Result<events::SessionInfo, String> {
    branch_session_internal(&state, Some(&app_handle), parent_id, branch_point).await
}

#[cfg(test)]
mod auto_title_tests {
    use super::derive_title_from_message;

    #[test]
    fn short_message_passes_through() {
        assert_eq!(
            derive_title_from_message("Fix the login bug"),
            "Fix the login bug"
        );
    }

    #[test]
    fn long_message_truncates_to_50_chars_plus_ellipsis() {
        let long = "a".repeat(80);
        let title = derive_title_from_message(&long);
        assert_eq!(title.chars().count(), 51);
        assert!(title.ends_with('…'));
        assert_eq!(&title[..50], "a".repeat(50));
    }

    #[test]
    fn truncation_counts_chars_not_bytes() {
        // 60 CJK chars = 180 UTF-8 bytes; byte-slicing would panic or mojibake.
        let zh = "配".repeat(60);
        let title = derive_title_from_message(&zh);
        assert_eq!(title.chars().count(), 51);
        assert!(title.ends_with('…'));
    }

    #[test]
    fn exactly_50_chars_is_not_truncated() {
        let exact = "b".repeat(50);
        assert_eq!(derive_title_from_message(&exact), exact);
    }

    #[test]
    fn uses_first_line_only() {
        assert_eq!(
            derive_title_from_message("first line\nsecond line\nthird"),
            "first line"
        );
    }

    #[test]
    fn whitespace_only_yields_empty() {
        assert_eq!(derive_title_from_message("   \n\t  "), "");
    }
}
