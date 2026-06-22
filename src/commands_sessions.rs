//! Session lifecycle Tauri commands (extracted from `commands.rs`).
//!
//! Second step of the commands.rs decomposition (R2-A3 / P1.1). The session
//! cluster is the largest cohesive domain — new/list/search/load/export/
//! switch/delete/rename/duplicate/branch + working_dir. StateManager-backed.

use crate::commands::{AppState, ChatMessage, SessionMeta, chrono_timestamp};
use crate::{config, events, events::event_names};
use tauri::Emitter;

/// Create a new session and return its UUID.
#[tauri::command]
pub async fn new_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4();
    let id_str = id.to_string();
    let title = format!("Session {}", id_str.split('-').next().unwrap_or(&id_str));
    let now = chrono_timestamp();

    // Create empty session file using StateManager
    let model = state.model.lock().await.clone();
    let metadata = shannon_core::state::SessionPersistMetadata {
        model,
        turn_count: 0,
        title: Some(title.clone()),
        ..Default::default()
    };

    state
        .state_manager
        .save_session(&id, &[], &metadata)
        .map_err(|e| e.to_string())?;

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

    // Set as current session
    {
        let mut current = state.current_session_id.lock().await;
        *current = Some(id_str.clone());
    }

    // Clear messages for new session
    {
        let mut messages = state.messages.lock().await;
        messages.clear();
    }

    // Emit sessions updated event
    let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());

    Ok(id_str)
}

/// List all sessions.
#[tauri::command]
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
            if let Ok(Some(data)) = state.state_manager.load_session(&uuid) {
                let hit = data.messages.iter().any(|m| match &m.content {
                    shannon_core::api::MessageContent::Text(t) => {
                        t.to_lowercase().contains(&query_lower)
                    }
                    shannon_core::api::MessageContent::Blocks(blocks) => {
                        blocks.iter().any(|b| match b {
                            shannon_core::api::ContentBlock::Text { text } => {
                                text.to_lowercase().contains(&query_lower)
                            }
                            _ => false,
                        })
                    }
                });
                if hit {
                    content_matches.push(info());
                }
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
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {}", e))?;

    // Load from StateManager
    let session_data = state
        .state_manager
        .load_session(&session_uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session not found: {}", id))?;

    // Convert shannon_core Messages to ChatMessages
    let messages: Vec<ChatMessage> = session_data
        .messages
        .into_iter()
        .map(|msg| ChatMessage {
            role: msg.role,
            content: match msg.content {
                shannon_core::api::MessageContent::Text(t) => t,
                shannon_core::api::MessageContent::Blocks(blocks) => {
                    // For blocks, extract text content
                    blocks
                        .iter()
                        .filter_map(|b| match b {
                            shannon_core::api::ContentBlock::Text { text } => Some(text.clone()),
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
    {
        let mut current_messages = state.messages.lock().await;
        *current_messages = messages.clone();
    }

    // Set as current session
    {
        let mut current = state.current_session_id.lock().await;
        *current = Some(id.clone());
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

/// Export a session to Markdown or JSON format.
#[tauri::command]
pub async fn export_session(
    state: tauri::State<'_, AppState>,
    id: String,
    format: String,
) -> Result<String, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {}", e))?;

    let session_data = state
        .state_manager
        .load_session(&session_uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session not found: {}", id))?;

    let title = session_data
        .metadata
        .title
        .as_deref()
        .unwrap_or("Untitled Session");

    match format.as_str() {
        "markdown" | "md" => {
            let mut md = format!("# {}\n\n", title);
            md.push_str(&format!(
                "Exported: {}\n\n---\n\n",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            ));
            for msg in &session_data.messages {
                let role_label = match msg.role.as_str() {
                    "user" => "**You**",
                    "assistant" => "**Assistant**",
                    "system" => "**System**",
                    other => &format!("**{}**", other),
                };
                let content = match &msg.content {
                    shannon_core::api::MessageContent::Text(t) => t.clone(),
                    shannon_core::api::MessageContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| match b {
                            shannon_core::api::ContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                md.push_str(&format!("### {}\n\n{}\n\n---\n\n", role_label, content));
            }
            Ok(md)
        }
        "json" => {
            let messages: Vec<serde_json::Value> = session_data
                .messages
                .iter()
                .map(|msg| {
                    let content = match &msg.content {
                        shannon_core::api::MessageContent::Text(t) => t.clone(),
                        shannon_core::api::MessageContent::Blocks(blocks) => blocks
                            .iter()
                            .filter_map(|b| match b {
                                shannon_core::api::ContentBlock::Text { text } => {
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
            "Unsupported format: {}. Use 'markdown' or 'json'.",
            format
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
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {}", e))?;

    // Save current session before switching
    {
        let current_id = state.current_session_id.lock().await.clone();
        if let Some(ref sid) = current_id {
            let messages = state.messages.lock().await.clone();
            if let Ok(uuid) = uuid::Uuid::parse_str(sid) {
                let model = state.model.lock().await.clone();
                let core_msgs: Vec<shannon_core::api::Message> = messages
                    .iter()
                    .map(|m| shannon_core::api::Message {
                        role: m.role.clone(),
                        content: shannon_core::api::MessageContent::Text(m.content.clone()),
                    })
                    .collect();
                let meta = shannon_core::state::SessionPersistMetadata {
                    model,
                    turn_count: core_msgs.len() / 2,
                    ..Default::default()
                };
                let _ = state.state_manager.save_session(&uuid, &core_msgs, &meta);
            }
        }
    }

    // Load new session
    let messages = match state
        .state_manager
        .load_session(&session_uuid)
        .map_err(|e| e.to_string())?
    {
        Some(data) => data
            .messages
            .into_iter()
            .map(|msg| ChatMessage {
                role: msg.role,
                content: match msg.content {
                    shannon_core::api::MessageContent::Text(t) => t,
                    shannon_core::api::MessageContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| match b {
                            shannon_core::api::ContentBlock::Text { text } => Some(text.clone()),
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

    // Update state
    {
        let mut current = state.current_session_id.lock().await;
        *current = Some(id.clone());
    }
    {
        let mut msgs = state.messages.lock().await;
        *msgs = messages.clone();
    }

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
    let current = state.current_session_id.lock().await.clone();
    let is_current = current.as_deref() == Some(id.as_str());
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

/// Delete a session by ID.
#[tauri::command]
pub async fn delete_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<bool, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {}", e))?;

    // Delete from StateManager
    let deleted = state
        .state_manager
        .delete_persisted_session(&session_uuid)
        .map_err(|e| e.to_string())?;

    if deleted {
        // Remove from sessions list
        let mut sessions = state.sessions.lock().await;
        sessions.retain(|s| s.id != id);

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
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {}", e))?;

    // Update session metadata in sessions list
    let mut sessions = state.sessions.lock().await;
    if let Some(session) = sessions.iter_mut().find(|s| s.id == id) {
        session.title = title.clone();

        // Update persisted session metadata
        let model = state.model.lock().await.clone();
        let messages = state.messages.lock().await.clone();
        let core_msgs: Vec<shannon_core::api::Message> = messages
            .iter()
            .map(|m| shannon_core::api::Message {
                role: m.role.clone(),
                content: shannon_core::api::MessageContent::Text(m.content.clone()),
            })
            .collect();

        let metadata = shannon_core::state::SessionPersistMetadata {
            model,
            turn_count: core_msgs.len() / 2,
            title: Some(title),
            ..Default::default()
        };

        let _ = state
            .state_manager
            .save_session(&session_uuid, &core_msgs, &metadata);

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
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {}", e))?;

    // Find original session
    let sessions = state.sessions.lock().await;
    let original_session = sessions
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("Session not found: {}", id))?;

    // Load original session data
    let session_data = state
        .state_manager
        .load_session(&session_uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session data not found: {}", id))?;

    // Create new session with copied messages
    let new_id = uuid::Uuid::new_v4();
    let new_id_str = new_id.to_string();
    let new_title = format!("Copy of {}", original_session.title);
    let now = chrono_timestamp();

    let model_name = state.model.lock().await.clone();
    let metadata = shannon_core::state::SessionPersistMetadata {
        model: model_name,
        turn_count: session_data.messages.len() / 2,
        title: Some(new_title.clone()),
        ..Default::default()
    };

    state
        .state_manager
        .save_session(&new_id, &session_data.messages, &metadata)
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
        uuid::Uuid::parse_str(&parent_id).map_err(|e| format!("Invalid UUID: {}", e))?;

    // Find parent session
    let sessions = state.sessions.lock().await;
    let parent_session = sessions
        .iter()
        .find(|s| s.id == parent_id)
        .ok_or_else(|| format!("Session not found: {}", parent_id))?;

    // Clone parent session data before dropping sessions
    let parent_title = parent_session.title.clone();
    let parent_working_dir = parent_session.working_dir.clone();

    // Load parent session data
    let session_data = state
        .state_manager
        .load_session(&parent_uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session data not found: {}", parent_id))?;

    // Create new session with messages up to branch point
    let new_id = uuid::Uuid::new_v4();
    let new_id_str = new_id.to_string();
    let new_title = format!("Branch of {}", parent_title);
    let now = chrono_timestamp();

    if branch_point >= session_data.messages.len() {
        return Err(format!(
            "Branch point {} out of bounds: session has {} messages (valid range: 0-{})",
            branch_point,
            session_data.messages.len(),
            session_data.messages.len().saturating_sub(1)
        ));
    }

    // Slice messages to include only up to branch point
    let branch_messages: Vec<shannon_core::api::Message> = session_data
        .messages
        .iter()
        .take(branch_point + 1)
        .cloned()
        .collect();

    let model_name = state.model.lock().await.clone();
    let metadata = shannon_core::state::SessionPersistMetadata {
        model: model_name,
        turn_count: branch_messages.len() / 2,
        title: Some(new_title.clone()),
        parent_session_id: Some(parent_uuid),
        branch_point_message_index: Some(branch_point),
        ..Default::default()
    };

    state
        .state_manager
        .save_session(&new_id, &branch_messages, &metadata)
        .map_err(|e| e.to_string())?;

    // Drop sessions lock before re-acquiring for push
    drop(sessions);

    // Add to sessions list with parent/branch info
    let new_session_meta = SessionMeta {
        id: new_id_str.clone(),
        title: new_title.clone(),
        created_at: now,
        message_count: branch_messages.len(),
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
        id: new_id_str,
        title: new_title,
        created_at: now,
        message_count: branch_messages.len(),
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
