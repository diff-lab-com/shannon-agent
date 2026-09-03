//! Permission request/response commands — the user-confirmation channel for
//! risky tool calls during a chat. The desktop UI listens to
//! `events::PERMISSION_REQUEST` and calls back via `respond_permission`.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).

use crate::commands::AppState;
use crate::events;
use crate::events::event_names;
use shannon_core::settings::SettingsManager;
use tauri::Emitter;
use tokio::sync::oneshot;

/// A pending permission prompt: the oneshot channel back to the requester,
/// plus the tool name so `"always allow"` can persist an allow rule for it.
pub(crate) struct PendingPermission {
    pub(crate) tx: oneshot::Sender<PermissionDecision>,
    pub(crate) tool: String,
}

/// The scoped outcome of a permission prompt. `AllowOnce`/`AlwaysAllow` both
/// mean "go ahead"; `AlwaysAllow` additionally persists a rule (done by
/// `respond_permission`), and the engine maps it to
/// `PermissionChoice::AlwaysAllow` so its in-session memory also learns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PermissionDecision {
    AllowOnce,
    AlwaysAllow,
    Deny,
}

/// Common body of the `request_permission` command and the engine-driven
/// prompt forwarder in `send_message`: register a pending request, surface
/// it on the wire, and wait for the user's (scoped) answer.
pub(crate) async fn prompt_user(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    tool: String,
    input: serde_json::Value,
    risk: String,
    timeout_secs: u64,
) -> PermissionDecision {
    let request_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();

    // Store the sender
    {
        let mut pending = state.pending_permissions.lock().await;
        pending.insert(
            request_id.clone(),
            PendingPermission {
                tx,
                tool: tool.clone(),
            },
        );
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

    // Wait for the user's response (interactive prompts get a generous
    // timeout; the user may be reading a diff).
    let timeout = tokio::time::Duration::from_secs(timeout_secs);
    let result = tokio::time::timeout(timeout, rx).await;

    // Clean up
    {
        let mut pending = state.pending_permissions.lock().await;
        pending.remove(&request_id);
    }

    match result {
        Ok(Ok(decision)) => decision,
        Ok(Err(_)) => PermissionDecision::Deny, // Sender dropped
        Err(_) => PermissionDecision::Deny,     // Timeout
    }
}

#[tauri::command]
pub async fn request_permission(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    tool: String,
    input: serde_json::Value,
    risk: String,
) -> Result<bool, String> {
    let decision = prompt_user(&state, &app_handle, tool, input, risk, 30).await;
    Ok(decision != PermissionDecision::Deny)
}

/// Respond to a permission request.
///
/// `scope` extends the decision: `"once"` (default) answers only this
/// request; `"always_tool"` also persists an allow rule for the tool in the
/// user's `~/.shannon/settings.json` (`permissions.allow`, evaluated as
/// Deny > Ask > Allow by the engine's rule checker). Deny never persists.
#[tauri::command]
pub async fn respond_permission(
    state: tauri::State<'_, AppState>,
    request_id: String,
    allow: bool,
    scope: Option<String>,
) -> Result<(), String> {
    let pending = {
        let mut map = state.pending_permissions.lock().await;
        map.remove(&request_id)
    };
    let Some(pending) = pending else {
        return Err(format!("Permission request not found: {request_id}"));
    };

    let decision = match (allow, scope.as_deref()) {
        (true, Some("always_tool")) => PermissionDecision::AlwaysAllow,
        (true, _) => PermissionDecision::AllowOnce,
        (false, _) => PermissionDecision::Deny,
    };

    // Persist before answering: the executor may proceed the moment the
    // oneshot resolves, and the rule should already be on disk. Best-effort
    // on save failure — the one-shot answer still goes through.
    if decision == PermissionDecision::AlwaysAllow {
        let mut manager = SettingsManager::new();
        if let Err(e) = manager.load_from_files() {
            eprintln!("always-allow: failed to load settings: {e}");
        } else {
            {
                let rules = &mut manager.settings_mut().permissions;
                if !rules.allow.iter().any(|p| p == &pending.tool) {
                    rules.allow.push(pending.tool.clone());
                }
            }
            if let Err(e) = manager.save() {
                eprintln!("always-allow: failed to save settings: {e}");
            }
        }
    }

    // Send response, ignoring errors if receiver dropped
    let _ = pending.tx.send(decision);
    Ok(())
}
