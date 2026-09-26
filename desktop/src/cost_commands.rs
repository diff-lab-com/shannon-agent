//! P0-4 — cost-observability commands: per-session budget + context
//! breakdown + per-session usage aggregation.
//!
//! - `set_session_budget` / `get_session_budget` persist the session's
//!   optional USD spend cap in the session sidecar (`meta.json`,
//!   backward-compatible `budget_usd` field — `events.jsonl` is untouched).
//! - `get_session_context_breakdown` estimates the six-category context
//!   split (system / tools / skills / memory / mcp / conversation) on the
//!   session's restored engine snapshot.
//! - `get_usage_by_session` aggregates the usage ledger per `session_id`
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

/// Action the mid-turn guard asks of the caller after folding one Usage
/// event in. `budget_usd` is carried along so the emitter stays a thin call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BudgetTurnAction {
    /// Nothing to do this event.
    Quiet,
    /// First crossing of the 80% line — emit `budget:warning`.
    Warn { spent_usd: f64, budget_usd: f64 },
    /// First crossing of the cap — emit `budget:exceeded` and cancel the
    /// turn. Latched: exactly once per turn.
    Exceeded { spent_usd: f64, budget_usd: f64 },
}

/// Mid-turn budget accumulator — the injectable boundary for
/// `send_message`'s streaming Usage arm (P0-4).
///
/// Folds each `QueryEvent::Usage` cost into the pre-turn spend basis and
/// classifies the running total. Semantics (unit-tested):
/// - `Exceeded` fires **exactly once** per turn (latched) — Usage events
///   already buffered when the cancel lands must not re-emit;
/// - `Warn` fires at most once, at the first crossing of
///   [`BUDGET_WARNING_FRACTION`];
/// - a single event large enough to jump straight past the cap yields
///   `Exceeded` only — the `Exceeded` arm is checked first, so no stray
///   `Warn` precedes it.
#[derive(Debug)]
pub(crate) struct BudgetTurnGuard {
    budget_usd: f64,
    spent_basis: f64,
    turn_cost: f64,
    warned: bool,
    exceeded_emitted: bool,
}

impl BudgetTurnGuard {
    /// Guard for one turn: `spent_basis` is the session's ledger spend
    /// before the turn started, `budget_usd` the configured cap.
    pub(crate) fn new(budget_usd: f64, spent_basis: f64) -> Self {
        Self {
            budget_usd,
            spent_basis,
            turn_cost: 0.0,
            warned: false,
            exceeded_emitted: false,
        }
    }

    /// Fold one Usage event's cost in and classify the running total.
    pub(crate) fn on_usage(&mut self, cost_usd: f64) -> BudgetTurnAction {
        self.turn_cost += cost_usd;
        let spent = self.spent_basis + self.turn_cost;
        match budget_verdict(spent, self.budget_usd) {
            BudgetVerdict::Exceeded if !self.exceeded_emitted => {
                self.exceeded_emitted = true;
                BudgetTurnAction::Exceeded {
                    spent_usd: spent,
                    budget_usd: self.budget_usd,
                }
            }
            BudgetVerdict::Warning if !self.warned => {
                self.warned = true;
                BudgetTurnAction::Warn {
                    spent_usd: spent,
                    budget_usd: self.budget_usd,
                }
            }
            _ => BudgetTurnAction::Quiet,
        }
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

// ── X7 — extension stats (per skill/MCP-tool calls + token cost) ─────────

/// Engine tool-name prefix under which skills ride the ToolRegistry
/// (`skill_<id>`; shannon-engine `context_breakdown::SKILL_TOOL_PREFIX`).
const SKILL_TOOL_PREFIX: &str = "skill_";

/// Engine tool-name prefix (and delimiters) of pooled MCP tools
/// (`mcp__<server>__<tool>`; shannon-mcp `process_pool::adapter`).
const MCP_TOOL_PREFIX: &str = "mcp__";

/// One tool's invocation stats (tool name, call count, token sum).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionToolStatRow {
    pub name: String,
    pub calls: u64,
    pub total_tokens: u64,
}

/// Per-server MCP rollup: server totals plus the per-tool detail under it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionMcpServerStats {
    pub server: String,
    pub calls: u64,
    pub total_tokens: u64,
    /// Remote tool names (the `mcp__<server>__` prefix stripped), most
    /// called first.
    pub tools: Vec<ExtensionToolStatRow>,
}

/// X7 stats for the Extensions page's Installed rows: skills, MCP servers
/// (with per-tool detail) and everything else, bucketed by tool name.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionStatsDto {
    pub days: u32,
    /// Skills bucket — `name` is the skill id (`skill_` prefix stripped, so
    /// the UI matches Installed rows by name).
    pub skills: Vec<ExtensionToolStatRow>,
    pub mcp_servers: Vec<ExtensionMcpServerStats>,
    /// Everything that is neither `skill_*` nor `mcp__*` (built-ins like
    /// `Bash`) — `name` is the raw tool name.
    pub other: Vec<ExtensionToolStatRow>,
}

/// The extension bucket one raw engine tool name resolves to.
enum Bucket {
    /// `skill_<id>` — carries the id with the prefix stripped.
    Skill(String),
    /// `mcp__<server>__<tool>` — carries both halves unprefixed.
    Mcp { server: String, tool: String },
}

/// Split one raw engine tool name into its extension bucket. `None` for
/// anything that is neither `skill_<id>` nor `mcp__<server>__<tool>` (the
/// caller keeps those in `other`, raw name); a bare `skill_` or an
/// `mcp__` name without both delimiters stays in `other` too rather than
/// inventing a bucket entry it cannot attribute.
fn bucket_of(raw: &str) -> Option<Bucket> {
    if let Some(rest) = raw.strip_prefix(SKILL_TOOL_PREFIX) {
        if rest.is_empty() {
            return None;
        }
        return Some(Bucket::Skill(rest.to_string()));
    }
    if let Some(rest) = raw.strip_prefix(MCP_TOOL_PREFIX) {
        if let Some((server, tool)) = rest.split_once("__") {
            if !server.is_empty() && !tool.is_empty() {
                return Some(Bucket::Mcp {
                    server: server.to_string(),
                    tool: tool.to_string(),
                });
            }
        }
    }
    None
}

/// Pure bucketing of the core adapter's per-tool stats (unit-tested).
///
/// Ordering is deterministic everywhere: skills/other most-called first
/// (ties by name), MCP servers most-called first (ties by server name) with
/// their tools sorted the same way.
fn bucket_extension_stats(
    days: u32,
    stats: &[shannon_core::session_log::ToolCallStat],
) -> ExtensionStatsDto {
    let mut skills: Vec<ExtensionToolStatRow> = Vec::new();
    let mut mcp: std::collections::HashMap<String, ExtensionMcpServerStats> =
        std::collections::HashMap::new();
    let mut other: Vec<ExtensionToolStatRow> = Vec::new();

    for stat in stats {
        match bucket_of(&stat.name) {
            Some(Bucket::Skill(id)) => skills.push(ExtensionToolStatRow {
                name: id,
                calls: stat.calls,
                total_tokens: stat.total_tokens,
            }),
            Some(Bucket::Mcp { server, tool }) => {
                let entry = mcp.entry(server).or_insert_with(|| ExtensionMcpServerStats {
                    server: String::new(),
                    calls: 0,
                    total_tokens: 0,
                    tools: Vec::new(),
                });
                entry.calls += stat.calls;
                entry.total_tokens += stat.total_tokens;
                entry.tools.push(ExtensionToolStatRow {
                    name: tool,
                    calls: stat.calls,
                    total_tokens: stat.total_tokens,
                });
            }
            None => other.push(ExtensionToolStatRow {
                name: stat.name.clone(),
                calls: stat.calls,
                total_tokens: stat.total_tokens,
            }),
        }
    }

    let by_calls_desc = |a: &ExtensionToolStatRow, b: &ExtensionToolStatRow| {
        b.calls.cmp(&a.calls).then_with(|| a.name.cmp(&b.name))
    };
    skills.sort_by(by_calls_desc);
    other.sort_by(by_calls_desc);

    let mut servers: Vec<ExtensionMcpServerStats> = mcp
        .into_iter()
        .map(|(server, mut entry)| {
            entry.server = server;
            entry.tools.sort_by(by_calls_desc);
            entry
        })
        .collect();
    servers.sort_by(|a, b| b.calls.cmp(&a.calls).then_with(|| a.server.cmp(&b.server)));

    ExtensionStatsDto {
        days,
        skills,
        mcp_servers: servers,
        other,
    }
}

/// Per-extension invocation stats over the last `days` days (clamped to
/// `[1, 365]`), derived from the session logs — no write-path involvement.
#[tauri::command]
pub async fn get_extension_stats(
    state: tauri::State<'_, AppState>,
    days: u32,
) -> Result<ExtensionStatsDto, String> {
    let days = days.clamp(1, 365);
    // Same sessions container the L0 store reads (the app's sessions dir).
    let query =
        shannon_core::session_log::SessionQuery::new(state.l0_store().container().to_path_buf());
    let stats = query
        .tool_call_stats(u64::from(days))
        .map_err(|e| e.to_string())?;
    Ok(bucket_extension_stats(days, &stats))
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

    // ── BudgetTurnGuard (mid-turn streaming semantics) ──────────────────

    #[test]
    fn guard_warns_once_at_80pct_then_stays_quiet_under_the_cap() {
        let mut g = BudgetTurnGuard::new(10.0, 0.0);
        // 7.0 = 70% → quiet.
        assert_eq!(g.on_usage(7.0), BudgetTurnAction::Quiet);
        // 1.5 more = 8.5 = 85% → first (and only) warning.
        assert_eq!(
            g.on_usage(1.5),
            BudgetTurnAction::Warn {
                spent_usd: 8.5,
                budget_usd: 10.0
            }
        );
        // Further in-budget events stay silent.
        assert_eq!(g.on_usage(0.5), BudgetTurnAction::Quiet);
        assert_eq!(g.on_usage(0.4), BudgetTurnAction::Quiet);
    }

    #[test]
    fn guard_exceeded_fires_exactly_once_and_latches() {
        let mut g = BudgetTurnGuard::new(1.0, 0.5);
        assert_eq!(
            g.on_usage(0.6),
            BudgetTurnAction::Exceeded {
                spent_usd: 1.1,
                budget_usd: 1.0
            }
        );
        // Events already buffered when the cancel lands must not re-emit.
        assert_eq!(g.on_usage(0.1), BudgetTurnAction::Quiet);
        assert_eq!(g.on_usage(0.1), BudgetTurnAction::Quiet);
    }

    #[test]
    fn guard_jump_straight_past_the_cap_never_warns() {
        // Arm-order contract: a single event crossing from below the
        // warning line to >= 100% produces Exceeded only.
        let mut g = BudgetTurnGuard::new(1.0, 0.0);
        let actions = [g.on_usage(2.0), g.on_usage(0.5), g.on_usage(0.5)];
        assert_eq!(
            actions[0],
            BudgetTurnAction::Exceeded {
                spent_usd: 2.0,
                budget_usd: 1.0
            }
        );
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, BudgetTurnAction::Warn { .. })),
            "no warning may precede or follow the exceeded on a jump-past-cap turn"
        );
        assert_eq!(actions[1], BudgetTurnAction::Quiet, "exceeded is latched");
    }

    #[test]
    fn guard_basis_carries_pre_turn_spend() {
        // Basis 0.9 of a 1.0 cap: the very first event (however small, as
        // long as it pushes ≥ 100%) exceeds without warning.
        let mut g = BudgetTurnGuard::new(1.0, 0.95);
        assert!(matches!(
            g.on_usage(0.05),
            BudgetTurnAction::Exceeded { .. }
        ));
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

    // ── X7 — extension stats bucketing ───────────────────────────────────

    fn stat(name: &str, calls: u64, total_tokens: u64) -> shannon_core::session_log::ToolCallStat {
        shannon_core::session_log::ToolCallStat {
            name: name.into(),
            calls,
            total_tokens,
        }
    }

    #[test]
    fn extension_stats_bucket_skill_mcp_and_other() {
        let dto = bucket_extension_stats(
            30,
            &[
                stat("Bash", 40, 0),
                stat("skill_deploy", 12, 3_500),
                stat("skill_commit", 5, 900),
                stat("mcp__notion__search", 7, 1_200),
                stat("mcp__notion__write", 2, 300),
                stat("mcp__github__pr", 9, 2_000),
            ],
        );
        assert_eq!(dto.days, 30);

        // Skills carry the id with the prefix stripped, most-called first.
        assert_eq!(
            dto.skills,
            vec![
                ExtensionToolStatRow { name: "deploy".into(), calls: 12, total_tokens: 3_500 },
                ExtensionToolStatRow { name: "commit".into(), calls: 5, total_tokens: 900 },
            ]
        );

        // MCP: per-server totals plus per-tool detail (remote tool names).
        assert_eq!(dto.mcp_servers.len(), 2);
        assert_eq!(dto.mcp_servers[0].server, "github");
        assert_eq!(dto.mcp_servers[0].calls, 9);
        assert_eq!(dto.mcp_servers[0].total_tokens, 2_000);
        assert_eq!(
            dto.mcp_servers[0].tools,
            vec![ExtensionToolStatRow { name: "pr".into(), calls: 9, total_tokens: 2_000 }]
        );
        assert_eq!(dto.mcp_servers[1].server, "notion");
        assert_eq!(dto.mcp_servers[1].calls, 7 + 2, "server total sums its tools");
        assert_eq!(dto.mcp_servers[1].total_tokens, 1_500);
        assert_eq!(
            dto.mcp_servers[1].tools,
            vec![
                ExtensionToolStatRow { name: "search".into(), calls: 7, total_tokens: 1_200 },
                ExtensionToolStatRow { name: "write".into(), calls: 2, total_tokens: 300 },
            ]
        );

        // Everything else keeps the raw tool name.
        assert_eq!(
            dto.other,
            vec![ExtensionToolStatRow { name: "Bash".into(), calls: 40, total_tokens: 0 }]
        );
    }

    #[test]
    fn extension_stats_keeps_unattributable_names_in_other() {
        // Malformed prefixes must not invent bucket entries they cannot
        // attribute: bare `skill_`, an `mcp__` name without both
        // delimiters, and an empty tool segment all stay in `other`.
        let dto = bucket_extension_stats(
            7,
            &[
                stat("skill_", 1, 0),
                stat("mcp__lonely", 2, 0),
                stat("mcp__srv__", 3, 0),
                stat("mcp__ok__tool", 4, 10),
            ],
        );
        assert!(dto.skills.is_empty());
        assert_eq!(dto.mcp_servers.len(), 1);
        assert_eq!(dto.mcp_servers[0].server, "ok");
        let mut other_names: Vec<&str> = dto.other.iter().map(|r| r.name.as_str()).collect();
        other_names.sort();
        assert_eq!(other_names, vec!["mcp__lonely", "mcp__srv__", "skill_"]);
    }

    #[test]
    fn extension_stats_dto_is_frozen_camel_case() {
        let dto = bucket_extension_stats(30, &[stat("skill_x", 1, 2), stat("mcp__s__t", 3, 4)]);
        let json = serde_json::to_value(&dto).unwrap();
        assert!(json.get("days").is_some(), "{json}");
        assert!(json.get("skills").is_some());
        assert!(json.get("mcpServers").is_some(), "camelCase on the wire: {json}");
        assert!(json.get("other").is_some());
        let server = &json["mcpServers"][0];
        assert!(server.get("totalTokens").is_some(), "{server}");
        assert!(server["tools"][0].get("totalTokens").is_some());
    }

    #[test]
    fn extension_stats_follows_the_real_core_adapter_over_fixture_logs() {
        // End-to-end through the real seam: logs written by the real
        // SessionLogWriter into a tempdir container (never HOME), read back
        // through SessionQuery::tool_call_stats, then bucketed.
        use shannon_core::session_log::{SessionLogWriter, SessionQuery};
        use shannon_types::session_event::{SessionEventBody, ToolCallPayload};

        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        let id = uuid::Uuid::new_v4();
        let mut w = SessionLogWriter::open_layout(&container, &id.to_string()).unwrap();
        w.record(SessionEventBody::ToolCall(ToolCallPayload {
            tool_use_id: "u1".into(),
            tool_name: "skill_deploy".into(),
            arguments: "{}".into(),
        }));
        w.record(SessionEventBody::ToolCall(ToolCallPayload {
            tool_use_id: "u2".into(),
            tool_name: "mcp__notion__search".into(),
            arguments: "{}".into(),
        }));
        w.close().unwrap();

        let query = SessionQuery::new(container);
        let stats = query.tool_call_stats(30).unwrap();
        let dto = bucket_extension_stats(30, &stats);
        assert_eq!(
            dto.skills,
            vec![ExtensionToolStatRow { name: "deploy".into(), calls: 1, total_tokens: 0 }]
        );
        assert_eq!(dto.mcp_servers.len(), 1);
        assert_eq!(dto.mcp_servers[0].server, "notion");
        assert_eq!(dto.mcp_servers[0].tools[0].name, "search");
    }
}
