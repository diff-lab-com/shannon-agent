//! Permission request/response commands — the user-confirmation channel for
//! risky tool calls during a chat. The desktop UI listens to
//! `events::PERMISSION_REQUEST` and calls back via `respond_permission`.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).

use crate::commands::AppState;
use crate::events;
use crate::events::event_names;
use tauri::Emitter;
use tokio::sync::oneshot;

#[tauri::command]
pub async fn request_permission(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    tool: String,
    input: serde_json::Value,
    risk: String,
) -> Result<bool, String> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();

    // Store the sender
    {
        let mut pending = state.pending_permissions.lock().await;
        pending.insert(request_id.clone(), tx);
    }

    // Emit event to frontend
    let _ = app_handle.emit(
        event_names::PERMISSION_REQUEST,
        events::PermissionRequest {
            tool: tool.clone(),
            input: input.clone(),
            risk: risk.clone(),
            request_id: request_id.clone(),
        },
    );

    // Wait for response with 30s timeout
    let timeout = tokio::time::Duration::from_secs(30);
    let result = tokio::time::timeout(timeout, rx).await;

    // Clean up
    {
        let mut pending = state.pending_permissions.lock().await;
        pending.remove(&request_id);
    }

    match result {
        Ok(Ok(allowed)) => Ok(allowed),
        Ok(Err(_)) => Ok(false), // Sender dropped
        Err(_) => Ok(false),     // Timeout
    }
}

/// Respond to a permission request.
#[tauri::command]
pub async fn respond_permission(
    state: tauri::State<'_, AppState>,
    request_id: String,
    allow: bool,
) -> Result<(), String> {
    let mut pending = state.pending_permissions.lock().await;
    if let Some(tx) = pending.remove(&request_id) {
        // Send response, ignoring errors if receiver dropped
        let _ = tx.send(allow);
        Ok(())
    } else {
        Err(format!("Permission request not found: {}", request_id))
    }
}
