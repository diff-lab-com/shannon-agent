//! Tauri IPC commands for the SQLite inbox (P0-3 backend).
//!
//! Storage lives in `~/.shannon/inbox.db` via
//! [`shannon_core::inbox_store::InboxStore`]. This module owns:
//!
//! - the five frontend-facing commands (`list_inbox_items`,
//!   `update_inbox_item_status`, `get_inbox_stats`, `rerun_inbox_item`,
//!   `continue_inbox_item_session`) — **command names and payload shapes are
//!   an interface contract with the desktop UI task and must stay verbatim**;
//! - [`spawn_routine_run`], the shared async executor used by both
//!   `rerun_inbox_item` and the loopback `POST /api/routines/:id/trigger`
//!   endpoint. It mirrors the unattended execution path of
//!   `commands::start_background_task` (fresh `QueryEngine`, configured
//!   approval mode, usage ledger writes) and, on completion, writes:
//!     1. the run finish into the legacy JSONL history (same run id, so the
//!        existing History view keeps working until the UI switches over),
//!     2. the inbox item (`source=routine|scheduled_task|trigger`), summary
//!        truncated to ≤500 chars,
//!     3. the `inbox_item_id` back-link on the `routine_runs` row,
//!     4. an `inbox-updated` event so the UI refreshes without polling.
//!
//! The legacy triage commands in `scheduled_commands.rs` are untouched (the
//! UI migration to this store happens in the frontend task).

use shannon_core::inbox_store::{InboxItem, InboxItemNew, InboxStats, InboxStatus};
use shannon_core::query_engine::{QueryContext, QueryEngine, QueryEvent, QueryMetadata};
use shannon_core::scheduled_routines::ScheduledRoutine;
use shannon_core::scheduled_runs::{RunStatus, ScheduledRun};
use shannon_engine::api::client::LlmClient;
use shannon_engine::permissions::{ApprovalMode, PermissionManager, PermissionRuleChecker};
use shannon_engine::state::StateManager;
use tauri::Emitter;
use tokio::sync::RwLock;

use crate::commands::AppState;
use crate::config::DesktopConfig;
use crate::events::event_names;

/// Max length of the generated inbox item summary (brief: ≤500 chars).
const SUMMARY_MAX_CHARS: usize = 500;

/// Sources that can be rerun through the scheduled-task execution path.
const RERUNNABLE_SOURCES: [&str; 3] = [
    shannon_core::inbox_store::SOURCE_ROUTINE,
    shannon_core::inbox_store::SOURCE_SCHEDULED_TASK,
    shannon_core::inbox_store::SOURCE_TRIGGER,
];

// ── Tauri commands (frontend contract — do not rename/reshape) ──────────

/// List inbox items, newest first.
#[tauri::command]
pub async fn list_inbox_items(
    state: tauri::State<'_, AppState>,
    status: Option<String>,
    source: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<InboxItem>, String> {
    let status = status
        .map(|s| InboxStatus::parse(&s))
        .transpose()
        .map_err(|e| e.to_string())?;
    state
        .inbox_store()
        .list(status, source.as_deref(), limit.unwrap_or(100))
        .map_err(|e| e.to_string())
}

/// Transition an inbox item to `pending` / `read` / `archived`.
///
/// Emits `inbox-updated` so badges refresh without polling.
#[tauri::command]
pub async fn update_inbox_item_status(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: i64,
    status: String,
) -> Result<(), String> {
    let status = InboxStatus::parse(&status).map_err(|e| e.to_string())?;
    state
        .inbox_store()
        .update_status(id, status)
        .map_err(|e| e.to_string())?;
    let _ = app_handle.emit(event_names::INBOX_UPDATED, id);
    Ok(())
}

/// Badge counts: `{ pending, today }`.
#[tauri::command]
pub async fn get_inbox_stats(state: tauri::State<'_, AppState>) -> Result<InboxStats, String> {
    state.inbox_store().stats().map_err(|e| e.to_string())
}

/// Re-run the routine / scheduled task behind an inbox item through the
/// existing unattended execution path. Returns the new run id.
#[tauri::command]
pub async fn rerun_inbox_item(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: i64,
) -> Result<String, String> {
    let inbox = state.inbox_store();
    let item = inbox
        .get_item(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("inbox item not found: {id}"))?;

    if !RERUNNABLE_SOURCES.contains(&item.source.as_str()) {
        return Err(format!(
            "inbox item source '{}' cannot be rerun",
            item.source
        ));
    }
    let task_id = item
        .source_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("inbox item {id} has no source task id"))?;

    let routine = state
        .scheduled_task_store()
        .load(task_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("task not found: {task_id}"))?;

    let deps = RoutineRunDeps::from_state(&state);
    spawn_routine_run(&deps, app_handle, routine, &item.source, None).await
}

/// Return the session id linked to an inbox item so the frontend can resume
/// the conversation with the existing session-loading commands.
#[tauri::command]
pub async fn continue_inbox_item_session(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<String, String> {
    let item = state
        .inbox_store()
        .get_item(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("inbox item not found: {id}"))?;
    item.session_id
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("inbox item {id} has no linked session"))
}

// ── Shared execution path ────────────────────────────────────────────────

/// The state slices [`spawn_routine_run`] needs, Arc-cloned so the spawned
/// task owns its inputs. Constructed from [`AppState`] (Tauri commands) or
/// from the loopback trigger endpoint's state.
pub(crate) struct RoutineRunDeps {
    pub(crate) inbox: std::sync::Arc<shannon_core::inbox_store::InboxStore>,
    pub(crate) runs_store: std::sync::Arc<shannon_core::scheduled_runs::ScheduledRunsStore>,
    pub(crate) usage_store: std::sync::Arc<crate::commands_usage::UsageStore>,
    pub(crate) client_config: std::sync::Arc<RwLock<shannon_engine::api::types::LlmClientConfig>>,
    pub(crate) desktop_config: std::sync::Arc<RwLock<DesktopConfig>>,
    pub(crate) tools: std::sync::Arc<shannon_core::tools::ToolRegistry>,
}

impl RoutineRunDeps {
    pub(crate) fn from_state(state: &AppState) -> Self {
        Self {
            inbox: state.inbox_store(),
            runs_store: state.scheduled_runs_store.clone(),
            usage_store: state.usage_store.clone(),
            client_config: state.client_config.clone(),
            desktop_config: state.desktop_config.clone(),
            tools: state.tools.clone(),
        }
    }
}

/// Kick off an unattended routine execution and return its run id.
///
/// Mirrors `commands::start_background_task`: fresh engine, configured
/// approval mode (unattended → persisted deny/allow rules honoured, prompts
/// auto-allowed), usage written to the ledger. On completion the run is
/// finished in both the SQLite `routine_runs` table and the legacy JSONL
/// history (same run id), and an inbox item is appended. Generic over the
/// Tauri runtime so tests can drive it with `mock_app`.
pub(crate) async fn spawn_routine_run<R: tauri::Runtime>(
    deps: &RoutineRunDeps,
    app: tauri::AppHandle<R>,
    routine: ScheduledRoutine,
    inbox_source: &str,
    note: Option<String>,
) -> Result<String, String> {
    let run_id = deps
        .inbox
        .record_run_start(&routine.id, &routine.name)
        .map_err(|e| e.to_string())?;
    let run_id_for_task = run_id.clone();

    // Mirror the run into the legacy JSONL history under the SAME run id so
    // the existing History view (which reads ScheduledRunsStore) keeps
    // working until the UI task switches it to the SQLite store.
    let mut jsonl_run = ScheduledRun::start(&routine.id, &routine.name);
    jsonl_run.run_id = run_id.clone();
    deps.runs_store
        .record(&jsonl_run)
        .map_err(|e| e.to_string())?;

    let client_config = deps.client_config.read().await.clone();
    let approval_mode_str = deps.desktop_config.read().await.approval_mode.clone();
    let tools = deps.tools.clone();
    let inbox = deps.inbox.clone();
    let runs_store = deps.runs_store.clone();
    let usage_store = deps.usage_store.clone();
    let app_for_task = app.clone();
    let model = client_config.model.clone();
    let model_for_usage = model.clone();
    let provider = client_config.provider.to_string();
    let prompt = routine.prompt.clone();
    let task_id = routine.id.clone();
    let task_name = routine.name.clone();
    let inbox_source = inbox_source.to_string();
    let started_ms = chrono::Utc::now().timestamp_millis();

    tokio::spawn(async move {
        let client = LlmClient::new(client_config);

        // Same policy as background tasks: run unattended under the
        // configured approval mode plus persisted rules.
        let mut permissions = PermissionManager::new();
        let mode = approval_mode_str
            .as_deref()
            .and_then(|s| match s {
                "full_auto" => Some(ApprovalMode::FullAuto),
                "auto_edit" => Some(ApprovalMode::AutoEdit),
                "auto" => Some(ApprovalMode::Auto),
                "plan" => Some(ApprovalMode::Plan),
                _ => None,
            })
            .unwrap_or(ApprovalMode::FullAuto);
        permissions.set_approval_mode(mode);
        let mut settings = shannon_core::settings::SettingsManager::new();
        if settings.load_from_files().is_ok() {
            let rules = &settings.settings_mut().permissions;
            permissions.set_rule_checker(PermissionRuleChecker::from_rule_strings(
                &rules.deny,
                &rules.ask,
                &rules.allow,
            ));
        }

        let engine =
            QueryEngine::with_defaults_arc(client, tools, permissions, StateManager::new());

        let session_id = uuid::Uuid::new_v4();
        let context = QueryContext {
            query_id: uuid::Uuid::new_v4(),
            session_id,
            user_message: prompt.clone(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: None,
                model,
                temperature: None,
                top_p: None,
            },
        };

        let mut final_output = String::new();
        let mut failure: Option<String> = None;

        let stream = engine.process_query(context, None).await;
        use futures::StreamExt;
        let mut pin_stream = std::pin::pin!(stream);
        while let Some(event_result) = pin_stream.next().await {
            match event_result {
                Ok(event) => match event {
                    QueryEvent::Text { content, .. } => final_output.push_str(&content),
                    QueryEvent::Usage {
                        input_tokens,
                        output_tokens,
                        cost_usd,
                        cache_creation_tokens,
                        cache_read_tokens,
                        ..
                    } => {
                        // Best-effort ledger write, mirroring background tasks.
                        let _ = usage_store.append(&crate::commands_usage::record_event(
                            &model_for_usage,
                            &provider,
                            crate::commands_usage::UsageTotals {
                                input_tokens,
                                output_tokens,
                                cache_creation_tokens,
                                cache_read_tokens,
                                cost_usd,
                            },
                            None,
                        ));
                    }
                    QueryEvent::Completed { .. } => break,
                    QueryEvent::Failed { error, .. } => {
                        failure = Some(error);
                        break;
                    }
                    _ => {}
                },
                Err(e) => {
                    failure = Some(e.to_string());
                    break;
                }
            }
        }

        let finished_ms = chrono::Utc::now().timestamp_millis();
        let duration_secs = (finished_ms - started_ms).max(0) / 1000;

        let (status, run_error): (&str, Option<String>) = match &failure {
            Some(err) => ("failed", Some(truncate_chars(err, SUMMARY_MAX_CHARS))),
            None => ("succeeded", None),
        };

        // 1. Legacy JSONL history (best-effort — never blocks the inbox).
        let jsonl_status = if failure.is_some() {
            RunStatus::Failed
        } else {
            RunStatus::Succeeded
        };
        if let Err(e) = runs_store.update(&run_id_for_task, |r| {
            r.finish(jsonl_status, run_error.clone())
        }) {
            tracing::warn!(run_id = %run_id_for_task, error = %e, "inbox: legacy run history update failed");
        }

        // 2. Inbox item + 3. run back-link.
        let summary = build_summary(
            note.as_deref(),
            duration_secs,
            &final_output,
            run_error.is_some(),
        );
        let item = inbox
            .append_item(InboxItemNew {
                source: inbox_source,
                source_id: Some(task_id),
                session_id: Some(session_id.to_string()),
                title: task_name,
                summary,
                error: run_error.clone(),
            })
            .map_err(|e| e.to_string());
        let item_id = match item {
            Ok(saved) => Some(saved.id),
            Err(e) => {
                tracing::warn!(run_id = %run_id_for_task, error = %e, "inbox: failed to append run item");
                None
            }
        };
        if let Err(e) =
            inbox.record_run_finish(&run_id_for_task, status, run_error.as_deref(), item_id)
        {
            tracing::warn!(run_id = %run_id_for_task, error = %e, "inbox: failed to finish run record");
        }

        // 4. Refresh signal.
        let _ = app_for_task.emit(event_names::INBOX_UPDATED, run_id_for_task);
    });

    Ok(run_id)
}

/// Truncate to at most `max` chars without splitting a UTF-8 codepoint, and
/// mark the cut with an ellipsis.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

/// Build the inbox summary: optional trigger note, duration, output snippet —
/// capped at [`SUMMARY_MAX_CHARS`] characters.
fn build_summary(note: Option<&str>, duration_secs: i64, output: &str, failed: bool) -> String {
    let snippet = first_line_snapshot(output, failed);
    let mut parts: Vec<String> = Vec::new();
    if let Some(n) = note.map(str::trim).filter(|n| !n.is_empty()) {
        parts.push(truncate_chars(n, 200).replace('\n', " "));
    }
    parts.push(format!("took {duration_secs}s"));
    if !snippet.is_empty() {
        parts.push(snippet);
    }
    truncate_chars(&parts.join(" · "), SUMMARY_MAX_CHARS)
}

/// Condense run output to a short snippet: the first non-empty line (failed
/// runs show the tail, where error summaries usually live).
fn first_line_snapshot(output: &str, failed: bool) -> String {
    let snippet = if failed {
        output
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
    } else {
        output
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
    };
    truncate_chars(snippet, 200).replace('\n', " ")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ── pure helpers ────────────────────────────────────────────────────

    #[test]
    fn truncate_chars_respects_limit_and_utf8() {
        assert_eq!(truncate_chars("hello", 10), "hello");
        assert_eq!(truncate_chars("hello", 5), "hello");
        let cut = truncate_chars("hello world", 5);
        assert!(cut.starts_with("hello"));
        assert!(cut.ends_with('…'));
        // Multi-byte safety.
        let emoji = "🦀".repeat(600);
        let cut = truncate_chars(&emoji, SUMMARY_MAX_CHARS);
        assert_eq!(cut.chars().count(), SUMMARY_MAX_CHARS + 1);
    }

    #[test]
    fn build_summary_includes_note_duration_output() {
        let s = build_summary(Some("from CI"), 12, "First line\nsecond", false);
        assert!(s.contains("from CI"), "{s}");
        assert!(s.contains("took 12s"), "{s}");
        assert!(s.contains("First line"), "{s}");
        assert!(!s.contains("second"), "snippet is one line: {s}");
    }

    #[test]
    fn build_summary_skips_empty_note_and_blank_output() {
        let s = build_summary(Some("   "), 0, "", false);
        assert_eq!(s, "took 0s");
        let s = build_summary(None, 3, "\n \n", false);
        assert_eq!(s, "took 3s");
    }

    #[test]
    fn build_summary_failure_uses_last_line() {
        let s = build_summary(None, 4, "started ok\nfinal error: boom", true);
        assert!(s.contains("boom"), "{s}");
        assert!(!s.contains("started ok"), "{s}");
    }

    #[test]
    fn build_summary_is_capped_at_500_chars() {
        let long = "x".repeat(2_000);
        let s = build_summary(Some(&long), 1, &long, false);
        assert!(s.chars().count() <= SUMMARY_MAX_CHARS + 1);
    }

    #[test]
    fn rerunnable_sources_cover_task_and_trigger() {
        assert!(RERUNNABLE_SOURCES.contains(&shannon_core::inbox_store::SOURCE_ROUTINE));
        assert!(RERUNNABLE_SOURCES.contains(&shannon_core::inbox_store::SOURCE_SCHEDULED_TASK));
        assert!(RERUNNABLE_SOURCES.contains(&shannon_core::inbox_store::SOURCE_TRIGGER));
        assert!(!RERUNNABLE_SOURCES.contains(&shannon_core::inbox_store::SOURCE_GOAL));
    }

    // ── store-backed pieces (no Tauri runtime needed) ───────────────────

    fn item_new_min(title: &str) -> InboxItemNew {
        InboxItemNew {
            source: shannon_core::inbox_store::SOURCE_ROUTINE.into(),
            source_id: Some("t".into()),
            session_id: None,
            title: title.into(),
            summary: String::new(),
            error: None,
        }
    }

    #[test]
    fn session_id_contract_is_preserved_through_the_store() {
        // The `continue_inbox_item_session` command filters/preserves
        // session ids exactly like this — the UI relies on receiving the id
        // string, or an Err when there is none.
        let store = shannon_core::inbox_store::InboxStore::open_in_memory().unwrap();
        let with_session = store
            .append_item(InboxItemNew {
                session_id: Some("0195abcd-0000-7000-8000-000000000000".into()),
                ..item_new_min("with session")
            })
            .unwrap();
        let without = store.append_item(item_new_min("no session")).unwrap();

        let got = store.get_item(with_session.id).unwrap().unwrap();
        assert_eq!(
            got.session_id.as_deref(),
            Some("0195abcd-0000-7000-8000-000000000000")
        );
        let missing = store.get_item(without.id).unwrap().unwrap();
        assert!(missing.session_id.is_none());
    }
}
