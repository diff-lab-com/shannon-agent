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
///
/// Runtime-generic (`R: tauri::Runtime`) so tests can drive it against the
/// mock runtime; production always passes `AppHandle` (`Wry`).
#[allow(clippy::too_many_arguments)] // prompt fields are inherent (tool/input/risk + session + reason)
pub(crate) async fn prompt_user<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &tauri::AppHandle<R>,
    tool: String,
    input: serde_json::Value,
    risk: String,
    timeout_secs: u64,
    // P1-1: owner session of the query that triggered the prompt, so
    // multi-window shells can ignore other sessions' prompts. `None` for
    // session-less callers (`request_permission` command).
    session_id: Option<String>,
    // P1-3: why this prompt was raised (rule hit / LLM verdict / default),
    // for the approval dialog's explanation line. `None` when unknown.
    reason: Option<shannon_types::events::PermissionReason>,
    // P3-1: the engine's free-text risk explanation, shown under the rule
    // line. `None` when unknown.
    risk_reason: Option<String>,
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
            session_id: session_id.clone(),
            reason,
            risk_reason,
        },
    );

    // T5 unified needs-attention stream: a session-scoped approval prompt is
    // exactly what the rail's amber dot signals, so the same event writes the
    // `session_approval` inbox entry (dedup: one per session). Best-effort.
    // The display title is computed once and shared with the OS notification
    // below.
    let session_title = match session_id.as_deref() {
        Some(sid) => Some(crate::inbox_session_events::session_display_title(state, sid).await),
        None => None,
    };
    let session_id_for_inbox = session_id.clone();
    if let (Some(sid), Some(title)) = (session_id_for_inbox.as_deref(), session_title.as_deref()) {
        crate::inbox_session_events::record_session_approval(
            state.inbox_store().as_ref(),
            app_handle,
            sid,
            title,
            &tool,
            &risk,
        );
    }

    // B4 (settings-r3 T5): OS-level heads-up while the approval waits — the
    // in-app approval card is invisible when another window has focus. Goes
    // through the shared `Notifier` so the desktop handler applies the
    // master/DND/`on_needs_attention` prefs and the webhook fan-out fires
    // too; deduped on `session_approval` within 5s so a burst of parallel
    // prompts doesn't stack popups. Fired once per prompt: the timeout
    // auto-deny below deliberately does NOT re-notify.
    {
        let notification = shannon_core::notifier::Notification {
            title: match session_title.as_deref() {
                Some(t) if !t.is_empty() => format!("Shannon — {t}"),
                _ => "Shannon".to_string(),
            },
            body: format!("Approval needed: {tool} (risk: {risk})"),
            level: shannon_core::notifier::NotificationLevel::Warning,
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now(),
            source: Some("session_approval".to_string()),
            action_id: None,
            kind: shannon_core::notifier::NotificationKind::NeedsAttention,
        };
        match state.notifier.notify_dedup(&notification, 5_000) {
            Ok(true) => tracing::debug!(tool = %tool, "approval OS notification dispatched"),
            Ok(false) => tracing::debug!(tool = %tool, "approval OS notification deduped"),
            Err(e) => tracing::warn!(error = %e, "approval OS notification dispatch failed"),
        }
    }

    // Wait for the user's response (interactive prompts get a generous
    // timeout; the user may be reading a diff).
    let timeout = tokio::time::Duration::from_secs(timeout_secs);
    let result = tokio::time::timeout(timeout, rx).await;

    // Clean up
    {
        let mut pending = state.pending_permissions.lock().await;
        pending.remove(&request_id);
    }

    // T5: whatever way the prompt settles (answer, timeout auto-deny, dropped
    // channel), it is handled — the inbox entry is resolved as read.
    if let Some(sid) = session_id_for_inbox.as_deref() {
        crate::inbox_session_events::resolve_session_approval(
            state.inbox_store().as_ref(),
            app_handle,
            sid,
        );
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
    let decision = prompt_user(&state, &app_handle, tool, input, risk, 30, None, None, None).await;
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_core::notifier::{CallbackNotifier, Cooldown, Notification, Notifier};
    use std::sync::{Arc, Mutex};

    /// AppState whose inbox writes land in memory and whose notifier fans
    /// out into `captured` (with a live Cooldown so the 5s dedup window is
    /// exercised exactly as in production).
    fn state_with_capture(
        captured: Arc<Mutex<Vec<Notification>>>,
    ) -> (tauri::AppHandle<tauri::test::MockRuntime>, AppState) {
        let app = tauri::test::mock_app();
        let mut state = AppState::new();
        state
            .inbox_store
            .set(Arc::new(
                shannon_core::inbox_store::InboxStore::open_in_memory().expect("in-memory inbox"),
            ))
            .expect("inbox store unset");
        let rec = captured.clone();
        let mut notifier = Notifier::new().with_cooldown(Cooldown::new());
        notifier.add_handler(Box::new(CallbackNotifier::with_name(
            move |n: &Notification| {
                rec.lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(n.clone());
                Ok(())
            },
            "capture",
        )));
        state.notifier = Arc::new(notifier);
        (app.handle().clone(), state)
    }

    #[tokio::test]
    async fn prompt_user_fires_one_needs_attention_notification_and_none_on_timeout() {
        let captured: Arc<Mutex<Vec<Notification>>> = Arc::new(Mutex::new(Vec::new()));
        let (app_handle, state) = state_with_capture(captured.clone());

        // timeout_secs = 0 → the wait times out immediately → auto-deny. The
        // prompt itself still fires exactly one notification up front; the
        // timeout path must NOT send a second one.
        let decision = prompt_user(
            &state,
            &app_handle,
            "bash".to_string(),
            serde_json::json!({ "cmd": "ls" }),
            "high".to_string(),
            0,
            Some("sess-approval-os-1".to_string()),
            None,
            None,
        )
        .await;
        assert_eq!(decision, PermissionDecision::Deny);

        let got = captured.lock().unwrap();
        assert_eq!(
            got.len(),
            1,
            "exactly one OS notification (prompt start), none on timeout; got {got:?}"
        );
        let n = &got[0];
        assert_eq!(
            n.kind,
            shannon_core::notifier::NotificationKind::NeedsAttention
        );
        assert!(matches!(
            n.level,
            shannon_core::notifier::NotificationLevel::Warning
        ));
        assert_eq!(n.source.as_deref(), Some("session_approval"));
        assert!(n.body.contains("bash"), "body names the tool: {}", n.body);
        assert!(n.title.starts_with("Shannon"), "title: {}", n.title);
    }

    #[tokio::test]
    async fn prompt_user_notification_dedups_parallel_prompts_within_window() {
        let captured: Arc<Mutex<Vec<Notification>>> = Arc::new(Mutex::new(Vec::new()));
        let (app_handle, state) = state_with_capture(captured.clone());

        let prompt_a = prompt_user(
            &state,
            &app_handle,
            "bash".to_string(),
            serde_json::json!({ "cmd": "a" }),
            "medium".to_string(),
            30,
            Some("sess-approval-dedup".to_string()),
            None,
            None,
        );
        let prompt_b = prompt_user(
            &state,
            &app_handle,
            "edit_file".to_string(),
            serde_json::json!({ "path": "x" }),
            "low".to_string(),
            30,
            Some("sess-approval-dedup".to_string()),
            None,
            None,
        );

        // Answer both prompts (AllowOnce) as soon as they register.
        let answerer = async {
            loop {
                let pending = {
                    let mut p = state.pending_permissions.lock().await;
                    if p.is_empty() {
                        None
                    } else {
                        let key = p.keys().next().expect("non-empty").clone();
                        p.remove(&key)
                    }
                };
                if let Some(pending) = pending {
                    let _ = pending.tx.send(PermissionDecision::AllowOnce);
                    if state.pending_permissions.lock().await.is_empty() {
                        return;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        };

        let (dec_a, dec_b, ()) = tokio::join!(prompt_a, prompt_b, answerer);
        assert_eq!(dec_a, PermissionDecision::AllowOnce);
        assert_eq!(dec_b, PermissionDecision::AllowOnce);

        let got = captured.lock().unwrap();
        assert_eq!(
            got.len(),
            1,
            "a burst of prompts within the 5s window coalesces into one OS notification"
        );
        assert_eq!(
            got[0].kind,
            shannon_core::notifier::NotificationKind::NeedsAttention
        );
    }
}
