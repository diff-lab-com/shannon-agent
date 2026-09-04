//! P0-4 — cost-observability commands: per-session budget + context
//! breakdown + per-session usage aggregation.
//!
//! - [`set_session_budget`] / [`get_session_budget`] persist the session's
//!   optional USD spend cap in the session sidecar (`meta.json`,
//!   backward-compatible `budget_usd` field — `events.jsonl` is untouched).
//! - [`get_session_context_breakdown`] estimates the six-category context
//!   split (system / tools / skills / memory / mcp / conversation) on the
//!   session's restored engine snapshot.
//! - [`get_usage_by_session`] aggregates the usage ledger per `session_id`
//!   for the Usage page's per-session view.
//!
//! The budget itself is *enforced* in `commands.rs::send_message` (pre-turn
//! reject / mid-turn cancel), in `goal_commands::run_turn_loop` (goal
//! sessions — first-limit-wins with the goal's own budget), and surfaced to
//! the frontend via the `budget:warning` / `budget:exceeded` events.

use serde::Serialize;
use shannon_core::session_log::SessionStore;
use tauri::Emitter;

use crate::commands::AppState;
use crate::events::event_names;

/// Share of the budget at which the advisory `budget:warning` fires
/// (the hard stop is at 100%).
pub(crate) const BUDGET_WARNING_FRACTION: f64 = 0.8;

/// Outcome of comparing session spend against the configured cap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BudgetVerdict {
    /// Below the warning threshold.
    Under,
    /// At or above [`BUDGET_WARNING_FRACTION`] of the cap, still under it.
    Warning,
    /// At or above the cap — the turn must not start (or must be aborted).
    Exceeded,
}

/// Pure spend/cap classification shared by the pre-turn check, the
/// mid-turn streaming guard and the goal runner (unit-tested below).
pub(crate) fn budget_verdict(spent_usd: f64, budget_usd: f64) -> BudgetVerdict {
    if spent_usd >= budget_usd {
        BudgetVerdict::Exceeded
    } else if spent_usd >= budget_usd * BUDGET_WARNING_FRACTION {
        BudgetVerdict::Warning
    } else {
        BudgetVerdict::Under
    }
}

/// Read the session's budget cap from its sidecar (`None` = no cap).
pub(crate) fn session_budget_usd(state: &AppState, session_id: uuid::Uuid) -> Option<f64> {
    state.l0_store().sidecar(&session_id).budget_usd
}

/// Cumulative spend the usage ledger attributes to `session_id`.
pub(crate) fn session_spent_usd(state: &AppState, session_id: &str) -> f64 {
    state.usage_store.spent_for_session(session_id)
}

/// Emit `budget:warning` / `budget:exceeded` with the frozen payload.
pub(crate) fn emit_budget_status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    warning: bool,
    session_id: &str,
    spent_usd: f64,
    budget_usd: f64,
) {
    let name = if warning {
        event_names::BUDGET_WARNING
    } else {
        event_names::BUDGET_EXCEEDED
    };
    let _ = app.emit(
        name,
        shannon_types::events::BudgetStatusPayload {
            session_id: session_id.to_string(),
            spent_usd,
            budget_usd,
        },
    );
}

// ── Session budget (frozen contract) ─────────────────────────────────────

/// Set (or clear) the session's USD budget cap. `budget_usd = null` clears
/// the cap. Persisted in the session sidecar; survives restarts.
#[tauri::command]
pub async fn set_session_budget(
    state: tauri::State<'_, AppState>,
    session_id: String,
    budget_usd: Option<f64>,
) -> Result<(), String> {
    let uuid =
        uuid::Uuid::parse_str(session_id.trim()).map_err(|e| format!("invalid sessionId: {e}"))?;
    if let Some(budget) = budget_usd {
        if !(budget.is_finite()) || budget <= 0.0 {
            return Err("budgetUsd must be a positive number (or null to clear)".into());
        }
    }
    let store = state.l0_store();
    let mut sidecar = store.sidecar(&uuid);
    sidecar.budget_usd = budget_usd;
    // Replace (not merge): an explicit `None` must actually clear the row.
    store
        .save_sidecar_replace(&uuid, &sidecar)
        .map_err(|e| format!("failed to persist session budget: {e}"))
}

/// Read the session's budget cap (`null` when none is set).
#[tauri::command]
pub async fn get_session_budget(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<Option<f64>, String> {
    let uuid =
        uuid::Uuid::parse_str(session_id.trim()).map_err(|e| format!("invalid sessionId: {e}"))?;
    Ok(session_budget_usd(&state, uuid))
}

// ── Context breakdown (frozen contract) ──────────────────────────────────

/// One category row of the context breakdown. `key` is one of
/// `system | tools | skills | memory | mcp | conversation`.
#[derive(Debug, Clone, Serialize)]
pub struct ContextBreakdownCategoryDto {
    pub key: String,
    pub tokens: u64,
}

/// Six-category token estimate for a session's current context.
///
/// Frozen wire shape (P0-4): `{ totalTokens, contextWindow, categories }`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBreakdownDto {
    pub total_tokens: u64,
    /// `None` when the model's window is genuinely unknown — no fabricated
    /// fallback number.
    pub context_window: Option<u64>,
    /// Always all six categories in stable order.
    pub categories: Vec<ContextBreakdownCategoryDto>,
}

impl From<shannon_engine::context_breakdown::ContextBreakdown> for ContextBreakdownDto {
    fn from(b: shannon_engine::context_breakdown::ContextBreakdown) -> Self {
        Self {
            total_tokens: b.total_tokens,
            context_window: b.context_window,
            categories: b
                .categories
                .into_iter()
                .map(|c| ContextBreakdownCategoryDto {
                    key: c.key,
                    tokens: c.tokens,
                })
                .collect(),
        }
    }
}

/// Estimate the six-category context breakdown for a session.
///
/// Estimation basis (desktop): the engine stashed on the live session when
/// present (it carries the resolved system prompt + context window), else a
/// minimal throwaway engine; the history is projected from the session's L0
/// log — the same restoration `send_message` uses. The client is never
/// contacted. Ambient assembly-time blocks (smart context, project
/// instructions, repo map) are not approximated, so `system` is a lower
/// bound for the fully-assembled prompt.
#[tauri::command]
pub async fn get_session_context_breakdown(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<ContextBreakdownDto, String> {
    let uuid =
        uuid::Uuid::parse_str(session_id.trim()).map_err(|e| format!("invalid sessionId: {e}"))?;
    let engine = crate::commands_slash::restored_engine(&state, uuid).await?;
    Ok(engine.context_breakdown().into())
}

// ── Per-session usage aggregation (frozen contract) ──────────────────────

/// One session's aggregated usage for the Usage page's per-session view.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsageRow {
    pub session_id: String,
    /// Session title when the sidecar has one; the UI falls back to a short
    /// id otherwise.
    pub title: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost_usd: f64,
    pub requests: u64,
    /// Timestamp (epoch ms) of the session's most recent ledger event.
    pub last_used_at_ms: u64,
}

/// Pure per-session aggregation of ledger records (windowed by the caller).
/// Records written before session attribution existed carry no `session_id`
/// and are invisible here by design — the by-model/day views still count
/// them.
fn aggregate_by_session(
    records: &[crate::commands_usage::UsageRecord],
    titles: &std::collections::HashMap<String, String>,
) -> Vec<SessionUsageRow> {
    let mut order: Vec<String> = Vec::new();
    let mut map: std::collections::HashMap<String, SessionUsageRow> =
        std::collections::HashMap::new();
    for r in records {
        let Some(session_id) = r.session_id.as_deref() else {
            continue;
        };
        if !map.contains_key(session_id) {
            order.push(session_id.to_string());
            map.insert(
                session_id.to_string(),
                SessionUsageRow {
                    session_id: session_id.to_string(),
                    title: titles.get(session_id).cloned(),
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                    cost_usd: 0.0,
                    requests: 0,
                    last_used_at_ms: 0,
                },
            );
        }
        let row = map.get_mut(session_id).expect("row just inserted");
        row.input_tokens += r.input_tokens;
        row.output_tokens += r.output_tokens;
        row.cache_creation_tokens += r.cache_creation_tokens;
        row.cache_read_tokens += r.cache_read_tokens;
        row.cost_usd += r.cost_usd;
        row.requests += 1;
        row.last_used_at_ms = row.last_used_at_ms.max(r.timestamp_ms);
    }
    let mut rows: Vec<SessionUsageRow> = order
        .into_iter()
        .map(|k| map.remove(&k).expect("row present"))
        .collect();
    rows.sort_by(|a, b| b.last_used_at_ms.cmp(&a.last_used_at_ms));
    rows
}

/// Aggregate the usage ledger per session for the last `days` days
/// (clamped to `[1, 365]`), most recently used first. Titles come from the
/// session sidecars when available.
#[tauri::command]
pub async fn get_usage_by_session(
    state: tauri::State<'_, AppState>,
    days: u32,
) -> Result<Vec<SessionUsageRow>, String> {
    let days = days.clamp(1, 365);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let cutoff_ms = now_ms.saturating_sub(u64::from(days) * 86_400_000);

    let records: Vec<crate::commands_usage::UsageRecord> = state
        .usage_store
        .load()
        .into_iter()
        .filter(|r| r.timestamp_ms >= cutoff_ms)
        .collect();

    // Titles: one small sidecar read per attributed session, cached for the
    // aggregation. Unparsable/missing sidecars simply leave `title = null`.
    let store: SessionStore = state.l0_store();
    let mut titles: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for id in records.iter().filter_map(|r| r.session_id.clone()) {
        if titles.contains_key(&id) {
            continue;
        }
        let title = uuid::Uuid::parse_str(&id)
            .ok()
            .and_then(|uuid| store.sidecar(&uuid).title.filter(|t| !t.trim().is_empty()));
        titles.insert(id, title.unwrap_or_default());
    }

    Ok(aggregate_by_session(&records, &titles))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::commands_usage::{UsageRecord, UsageTotals, record_event};

    fn rec(ts_ms: u64, session: Option<&str>, cost: f64) -> UsageRecord {
        let mut r = record_event(
            "claude-sonnet-4-6",
            "anthropic",
            UsageTotals {
                input_tokens: 100,
                output_tokens: 50,
                cache_creation_tokens: 10,
                cache_read_tokens: 5,
                cost_usd: cost,
            },
            session,
        );
        r.timestamp_ms = ts_ms;
        r
    }

    // ── budget_verdict ───────────────────────────────────────────────────

    #[test]
    fn budget_verdict_classifies_under_warning_exceeded() {
        assert_eq!(budget_verdict(0.0, 10.0), BudgetVerdict::Under);
        assert_eq!(budget_verdict(5.0, 10.0), BudgetVerdict::Under);
        // >= 80% warns (boundary inclusive).
        assert_eq!(budget_verdict(8.0, 10.0), BudgetVerdict::Warning);
        assert_eq!(budget_verdict(9.9, 10.0), BudgetVerdict::Warning);
        // >= 100% exceeds (first-limit-wins: boundary inclusive).
        assert_eq!(budget_verdict(10.0, 10.0), BudgetVerdict::Exceeded);
        assert_eq!(budget_verdict(12.0, 10.0), BudgetVerdict::Exceeded);
    }

    #[test]
    fn budget_verdict_tiny_caps_warn_immediately_but_still_stop() {
        // A cap so small that 80% is under one event's cost still stops at
        // the cap — only the warning/exceeded split shifts.
        assert_eq!(budget_verdict(0.01, 0.01), BudgetVerdict::Exceeded);
        assert_eq!(budget_verdict(0.004, 0.005), BudgetVerdict::Warning);
    }

    // ── aggregate_by_session ─────────────────────────────────────────────

    #[test]
    fn aggregate_by_session_sums_and_orders_by_recency() {
        let records = vec![
            rec(1_000, Some("a"), 0.10),
            rec(3_000, Some("b"), 0.30),
            rec(2_000, Some("a"), 0.20),
            rec(4_000, None, 9.99), // legacy pre-attribution line: skipped
        ];
        let titles = std::collections::HashMap::new();
        let rows = aggregate_by_session(&records, &titles);

        assert_eq!(rows.len(), 2, "unattributed records form no session row");
        // Most recently used first: session b (3_000) before a (2_000).
        assert_eq!(rows[0].session_id, "b");
        assert!((rows[0].cost_usd - 0.30).abs() < 1e-9);
        assert_eq!(rows[0].requests, 1);
        assert_eq!(rows[1].session_id, "a");
        assert!((rows[1].cost_usd - 0.30).abs() < 1e-9);
        assert_eq!(rows[1].requests, 2);
        assert_eq!(rows[1].input_tokens, 200);
        assert_eq!(rows[1].cache_read_tokens, 10);
        assert_eq!(rows[1].last_used_at_ms, 2_000);
    }

    #[test]
    fn aggregate_by_session_joins_titles_and_defaults_missing_to_none() {
        let records = vec![rec(1_000, Some("a"), 0.10), rec(2_000, Some("b"), 0.20)];
        let titles: std::collections::HashMap<String, String> =
            [("a".to_string(), "Fix the login bug".to_string())]
                .into_iter()
                .collect();
        let rows = aggregate_by_session(&records, &titles);
        assert_eq!(rows[0].session_id, "b");
        assert_eq!(rows[0].title, None);
        assert_eq!(rows[1].title.as_deref(), Some("Fix the login bug"));
    }

    #[test]
    fn aggregate_by_session_empty_ledger_is_empty() {
        let rows = aggregate_by_session(&[], &std::collections::HashMap::new());
        assert!(rows.is_empty());
    }

    // ── Sidecar persistence (set/get command bodies are thin wrappers) ──

    #[test]
    fn session_budget_round_trips_through_sidecar_and_clears() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp.path().to_path_buf());
        let session = uuid::Uuid::new_v4();

        let mut sidecar = store.sidecar(&session);
        assert_eq!(sidecar.budget_usd, None, "fresh sidecar has no cap");
        sidecar.budget_usd = Some(5.5);
        sidecar.title = Some("Budgeted session".into());
        store.save_sidecar_replace(&session, &sidecar).unwrap();

        assert_eq!(store.sidecar(&session).budget_usd, Some(5.5));
        assert_eq!(
            store.sidecar(&session).title.as_deref(),
            Some("Budgeted session")
        );

        // Clearing replaces the row with `None` (explicit-clear semantics).
        let mut sidecar = store.sidecar(&session);
        sidecar.budget_usd = None;
        store.save_sidecar_replace(&session, &sidecar).unwrap();
        assert_eq!(store.sidecar(&session).budget_usd, None);
        // The unrelated rows survive.
        assert_eq!(
            store.sidecar(&session).title.as_deref(),
            Some("Budgeted session")
        );
    }

    #[test]
    fn pre_p04_sidecar_without_budget_field_parses_as_no_cap() {
        // Backward compatibility: a meta.json written before P0-4 (no
        // `budget_usd` key) loads with `budget_usd = None` — and re-saving
        // keeps `events.jsonl` untouched (the sidecar is a separate file).
        let tmp = tempfile::tempdir().unwrap();
        let session = uuid::Uuid::new_v4();
        std::fs::create_dir_all(tmp.path().join(session.to_string())).unwrap();
        std::fs::write(
            tmp.path().join(session.to_string()).join("meta.json"),
            r#"{"title":"legacy","goal":null}"#,
        )
        .unwrap();

        let store = SessionStore::new(tmp.path().to_path_buf());
        let sidecar = store.sidecar(&session);
        assert_eq!(sidecar.budget_usd, None);
        assert_eq!(sidecar.title.as_deref(), Some("legacy"));
    }

    // ── DTO frozen shape ─────────────────────────────────────────────────
    #[test]
    fn context_breakdown_dto_is_frozen_camel_case() {
        let dto = ContextBreakdownDto {
            total_tokens: 42,
            context_window: Some(200_000),
            categories: vec![ContextBreakdownCategoryDto {
                key: "system".into(),
                tokens: 42,
            }],
        };
        let json = serde_json::to_value(&dto).unwrap();
        assert!(json.get("totalTokens").is_some(), "{json}");
        assert!(json.get("contextWindow").is_some(), "{json}");
        assert!(json.get("categories").is_some(), "{json}");
        assert!(json["categories"][0].get("key").is_some());
        assert!(json["categories"][0].get("tokens").is_some());
    }

    #[test]
    fn session_usage_row_dto_is_frozen_camel_case() {
        let row = SessionUsageRow {
            session_id: "s".into(),
            title: None,
            input_tokens: 1,
            output_tokens: 2,
            cache_creation_tokens: 3,
            cache_read_tokens: 4,
            cost_usd: 0.5,
            requests: 9,
            last_used_at_ms: 1_000,
        };
        let json = serde_json::to_value(&row).unwrap();
        for key in [
            "sessionId",
            "title",
            "inputTokens",
            "outputTokens",
            "cacheCreationTokens",
            "cacheReadTokens",
            "costUsd",
            "requests",
            "lastUsedAtMs",
        ] {
            assert!(json.get(key).is_some(), "missing {key} in {json}");
        }
    }
}
