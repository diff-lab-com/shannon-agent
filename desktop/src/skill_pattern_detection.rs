//! Skill-pattern detection — scans recent sessions for recurring tool-call
//! sequences and writes new SkillCandidate entries to the candidates JSONL.
//!
//! Detection runs on-demand via the `trigger_skill_pattern_detection`
//! command and can be wired into a daily routine by the scheduled-tasks
//! layer. The algorithm:
//!
//! 1. List sessions in the real L0 layout
//!    (`~/.shannon/sessions/<uuid>/events.jsonl`) active within `days_back`
//!    through `shannon_core::session_log::SessionQuery` — the read-side
//!    single source it shares with the dream pass's excerpt gathering
//!    (adversarial review §2.1 F1: the retired flat `sessions/*.json` walk
//!    silently scanned zero real sessions). Archived sessions (curation
//!    sidecar) are excluded at this input layer.
//! 2. For each session, project its `tool/call` events as (tool_name +
//!    sorted argument keys) pairs — values are never read.
//! 3. Compute a normalized signature per session (tool_name + sorted arg
//!    keys, joined by →).
//! 4. Group identical signatures across sessions.
//! 5. Signatures seen in `min_sessions`+ distinct sessions and
//!    `min_occurrences`+ total occurrences become new candidates.
//!
//! New candidates are appended to the existing JSONL via
//! `crate::commands_skill_candidates::append_candidate_in`; existing
//! candidate ids are left untouched so approval flows aren't disrupted.

use std::collections::HashMap;
use std::path::PathBuf;

use tauri::Emitter;

use shannon_core::session_log::{SessionQuery, SessionRef};

use crate::commands_skill_candidates::{SkillCandidate, SourceToolCall, append_candidate_in};

/// Threshold sessions for a pattern to qualify as a candidate.
const DEFAULT_MIN_SESSIONS: usize = 2;
/// Threshold total occurrences across sessions.
const DEFAULT_MIN_OCCURRENCES: u32 = 3;
/// Session-window default (days) for on-demand detection runs — the
/// `trigger_skill_pattern_detection` command and the `/detect-skills`
/// slash backend both fall back to it when no explicit window is given.
pub(crate) const DEFAULT_DETECT_DAYS_BACK: u32 = 7;

/// Build a stable signature from a tool name + sorted argument keys.
/// Only the **keys** participate — values are dropped on purpose so
/// that file paths, tokens, or other user secrets never leak into
/// candidate JSONL or the hash that deduplicates candidates.
fn signature_of(tool_name: &str, arg_keys: &[String]) -> String {
    let mut keys: Vec<&str> = arg_keys.iter().map(|s| s.as_str()).collect();
    keys.sort();
    format!("{tool_name}({})", keys.join(","))
}

/// Aggregate: how often a signature appeared, and which sessions it appeared in.
#[derive(Default)]
struct SignatureAgg {
    sessions: std::collections::HashSet<String>,
    total: u32,
    sample_tool: String,
    sample_arg_keys: Vec<String>,
    sample_session_ids: Vec<String>,
}

/// Run pattern detection. Returns the candidates appended by this run.
///
/// `sessions_dir` is the sessions **container** (`~/.shannon/sessions/` —
/// the directory of per-session `<uuid>/` subdirectories, as returned by
/// [`default_sessions_dir`]); injected for testability. Candidates are
/// appended under the real `~/.shannon/desktop/` (resolved here); tests that
/// need isolation call `run_detection_in` with a tempdir so they never mutate
/// the process-global `HOME` env var.
pub fn run_detection(
    sessions_dir: &std::path::Path,
    days_back: u32,
    min_sessions: usize,
    min_occurrences: u32,
) -> Result<Vec<SkillCandidate>, String> {
    let candidates_dir = crate::commands_skill_candidates::desktop_dir()?;
    run_detection_in(
        &candidates_dir,
        sessions_dir,
        days_back,
        min_sessions,
        min_occurrences,
    )
}

/// Implementation of [`run_detection`] against an explicit `candidates_dir`
/// (the `desktop` dir the candidates JSONL lives in). Production resolves it
/// from `~/.shannon/desktop`; tests pass a tempdir. Returns the appended
/// candidates (T5: the caller mirrors them into the unified inbox).
fn run_detection_in(
    candidates_dir: &std::path::Path,
    sessions_dir: &std::path::Path,
    days_back: u32,
    min_sessions: usize,
    min_occurrences: u32,
) -> Result<Vec<SkillCandidate>, String> {
    let query = SessionQuery::new(sessions_dir);
    // Archived sessions are excluded at the input layer (include_archived =
    // false): a pattern is only worth distilling while its source sessions
    // are live inputs.
    let recent: Vec<SessionRef> = query
        .list_recent(days_back, false)
        .map_err(|e| format!("session query: {e}"))?;
    let mut aggregates: HashMap<String, SignatureAgg> = HashMap::new();

    for session in recent {
        let session_id = session.session_id.to_string();
        // One unreadable log is skipped (warned), never fatal — matching the
        // old loader's skip-on-parse-failure semantics.
        let Ok(calls) = query.tool_calls(&session.session_id) else {
            tracing::warn!(
                session = %session_id,
                "skill detection: skipping unreadable session log"
            );
            continue;
        };
        for call in calls {
            let sig = signature_of(&call.tool_name, &call.arg_keys);
            let agg = aggregates.entry(sig).or_insert_with(|| SignatureAgg {
                sample_tool: call.tool_name.clone(),
                sample_arg_keys: call.arg_keys.clone(),
                ..SignatureAgg::default()
            });
            agg.total += 1;
            if agg.sessions.insert(session_id.clone()) {
                agg.sample_session_ids.push(session_id.clone());
            }
        }
    }

    let mut appended: Vec<SkillCandidate> = Vec::new();
    let now = chrono::Utc::now().to_rfc3339();
    for (_sig, agg) in aggregates.iter() {
        if agg.sessions.len() < min_sessions || agg.total < min_occurrences {
            continue;
        }
        let id = format!(
            "sig-{:x}",
            xxhash_simple(&format!("{}|{:?}", agg.sample_tool, agg.sample_session_ids))
        );
        let candidate = SkillCandidate {
            id,
            detected_at: now.clone(),
            occurrence_count: agg.total,
            example_session_ids: agg.sample_session_ids.iter().take(5).cloned().collect(),
            proposed_name: agg.sample_tool.clone(),
            proposed_trigger: format!(
                "Detected {} call recurring across {} session(s)",
                agg.sample_tool,
                agg.sessions.len()
            ),
            procedure: vec![format!(
                "Invoke {} with the same argument shape",
                agg.sample_tool
            )],
            source_tool_calls: vec![SourceToolCall {
                tool: agg.sample_tool.clone(),
                args_summary: agg
                    .sample_arg_keys
                    .iter()
                    .map(|k| (k.clone(), serde_json::Value::Null))
                    .collect(),
            }],
            refined: false,
        };
        append_candidate_in(candidates_dir, candidate.clone())?;
        appended.push(candidate);
    }
    Ok(appended)
}

/// Cheap non-cryptographic hash for deriving stable candidate ids.
fn xxhash_simple(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for byte in s.as_bytes() {
        h ^= u64::from(*byte);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Default sessions container: ~/.shannon/sessions/ (one `<uuid>/` directory
/// per session — the L0 layout).
pub fn default_sessions_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    Ok(home.join(".shannon").join("sessions"))
}

#[tauri::command]
pub async fn trigger_skill_pattern_detection(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::commands::AppState>,
    days_back: Option<u32>,
) -> Result<usize, String> {
    let dir = default_sessions_dir()?;
    let days = days_back.unwrap_or(DEFAULT_DETECT_DAYS_BACK);
    let inbox = state.inbox_store();
    let appended = detect_and_record(&app, &inbox, &dir, days).await?;
    Ok(appended.len())
}

/// Core of [`trigger_skill_pattern_detection`] — and the L3 leg of the dream
/// pass (`commands_dream`): privacy gate (config `skill_detection_enabled`)
/// → heuristic detection over the sessions container → one `skill_candidate`
/// inbox card per newly appended candidate → a `skill-candidates-changed`
/// push so the badge refreshes. Purely heuristic (zero LLM cost), so callers
/// may run it without the dream pass's throttles.
///
/// Returns the candidates appended by this run (empty when the privacy gate
/// is off — session files are never read in that case).
///
/// Generic over the runtime like the rest of the record/emit helpers it
/// calls, so the dream pass's `execute_dream_pass_in` seam can invoke it
/// from tests on a mock runtime too.
pub(crate) async fn detect_and_record<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    inbox: &shannon_core::inbox_store::InboxStore,
    sessions_dir: &std::path::Path,
    days: u32,
) -> Result<Vec<SkillCandidate>, String> {
    // Privacy opt-out: when the user has disabled skill detection in
    // Settings, the detector returns nothing without touching session files.
    let cfg = crate::config::load_config();
    if !cfg.skill_detection_enabled {
        return Ok(Vec::new());
    }
    let appended = run_detection(
        sessions_dir,
        days,
        DEFAULT_MIN_SESSIONS,
        DEFAULT_MIN_OCCURRENCES,
    )?;
    // T5 unified needs-attention stream: each newly appended candidate gets
    // (or refreshes — dedup keys on the candidate id) a `skill_candidate`
    // inbox entry, written at the same place the `skill-candidates-changed`
    // event fires. Best-effort per candidate.
    for candidate in &appended {
        crate::inbox_session_events::record_skill_candidate(inbox, app, candidate);
    }
    if !appended.is_empty() {
        // Push instead of poll: the Header badge refreshes on the event
        // instead of sweeping the store every 30s.
        let _ = app.emit(
            "skill-candidates-changed",
            serde_json::json!({ "detected": appended.len() }),
        );
    }
    Ok(appended)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shannon_core::session_log::{SessionCuration, SessionLogWriter};
    use shannon_types::session_event::{
        SessionEventBody, SessionStartPayload, ToolCallPayload, TurnEndPayload, TurnStartPayload,
        UserMessagePayload,
    };
    use tempfile::tempdir;
    use uuid::Uuid;

    /// Seed one session through the real writer: a framed turn with one user
    /// prompt and the given tool calls (each argument value is a dummy —
    /// only argument keys participate in detection).
    fn seed_session(dir: &std::path::Path, tool_calls: &[(&str, &[&str])]) -> Uuid {
        let id = Uuid::new_v4();
        let mut w = SessionLogWriter::open_layout(dir, &id.to_string()).unwrap();
        w.record(SessionEventBody::SessionStart(SessionStartPayload {
            model: "test-model".into(),
            provider: None,
            cwd: Some("/proj".into()),
            app_version: None,
            ..Default::default()
        }));
        w.record(SessionEventBody::TurnStart(TurnStartPayload {
            query_id: None,
        }));
        w.record(SessionEventBody::UserMessage(UserMessagePayload {
            source: UserMessagePayload::SOURCE_USER.into(),
            content: "go".into(),
            attachment_count: 0,
        }));
        for (tool, keys) in tool_calls {
            let mut input = serde_json::Map::new();
            for k in *keys {
                input.insert((*k).to_string(), serde_json::Value::String("x".into()));
            }
            w.record(SessionEventBody::ToolCall(ToolCallPayload {
                tool_use_id: format!("u-{}", Uuid::new_v4()),
                tool_name: (*tool).to_string(),
                arguments: serde_json::Value::Object(input).to_string(),
            }));
        }
        w.record(SessionEventBody::TurnEnd(TurnEndPayload {
            reason: TurnEndPayload::REASON_COMPLETED.into(),
            usage: None,
            error: None,
        }));
        w.close().unwrap();
        id
    }

    #[test]
    fn signature_includes_sorted_arg_keys() {
        let keys = vec!["b".to_string(), "a".to_string()];
        assert_eq!(signature_of("bash", &keys), "bash(a,b)");
    }

    #[test]
    fn run_detection_appends_candidates_for_recurring_patterns() {
        let dir = tempdir().unwrap();
        // Three sessions, each with the same tool signature.
        for _ in 0..3 {
            seed_session(dir.path(), &[("bash", &["cmd"])]);
        }

        // Candidates land in an isolated tempdir — no HOME mutation. The old
        // form set HOME to a throwaway dir, which is process-global and raced
        // with unrelated tests reading dirs::home_dir() under parallel --lib.
        let candidates_dir = tempdir().unwrap();
        let result = run_detection_in(candidates_dir.path(), dir.path(), 7, 2, 3);
        let appended = result.expect("detection ran");
        assert_eq!(appended.len(), 1, "expected exactly one new candidate");
        // T5: the returned candidates carry the fields the inbox mirror uses.
        let candidate = &appended[0];
        assert!(candidate.id.starts_with("sig-"));
        assert_eq!(candidate.proposed_name, "bash");
        assert_eq!(candidate.occurrence_count, 3);
        assert_eq!(candidate.example_session_ids.len(), 3);
        let summary = &candidate.source_tool_calls[0].args_summary;
        assert_eq!(summary.len(), 1);
        assert_eq!(
            summary.get("cmd"),
            Some(&serde_json::Value::Null),
            "argument keys only — values never surface"
        );
    }

    #[test]
    fn run_detection_skips_patterns_below_threshold() {
        let dir = tempdir().unwrap();
        seed_session(dir.path(), &[("bash", &["cmd"])]);

        let candidates_dir = tempdir().unwrap();
        let result = run_detection_in(candidates_dir.path(), dir.path(), 7, 2, 3);
        let appended = result.expect("detection ran");
        assert_eq!(
            appended.len(),
            0,
            "single-session pattern should not promote"
        );
    }

    #[test]
    fn run_detection_ignores_archived_sessions() {
        let dir = tempdir().unwrap();
        // Three sessions share the signature — but one of them is archived,
        // leaving two live sessions: exactly `min_sessions`, still detected.
        for _ in 0..2 {
            seed_session(dir.path(), &[("bash", &["cmd"])]);
        }
        let archived = seed_session(dir.path(), &[("bash", &["cmd"])]);
        let query = SessionQuery::new(dir.path());
        query
            .save_curation(&archived, &SessionCuration { archived: true })
            .unwrap();

        let candidates_dir = tempdir().unwrap();
        let appended =
            run_detection_in(candidates_dir.path(), dir.path(), 7, 3, 3).expect("detection ran");
        assert_eq!(
            appended.len(),
            0,
            "the archived session must not count toward the thresholds"
        );

        // Lifting the archive filter (include_archived=true is the Task 2
        // escape hatch) brings the third session back into the input window.
        let refs = query.list_recent(7, true).unwrap();
        assert_eq!(refs.len(), 3);
    }

    #[test]
    fn run_detection_respects_the_session_window() {
        let dir = tempdir().unwrap();
        for _ in 0..3 {
            seed_session(dir.path(), &[("bash", &["cmd"])]);
        }
        let candidates_dir = tempdir().unwrap();
        // days_back = 0 puts the window cutoff at "now" — every session was
        // written strictly before the call, so the input set is empty.
        let appended =
            run_detection_in(candidates_dir.path(), dir.path(), 0, 2, 3).expect("detection ran");
        assert_eq!(appended.len(), 0, "a zero-day window sees no sessions");
    }

    #[test]
    fn run_detection_returns_zero_when_sessions_dir_missing() {
        let candidates_dir = tempdir().unwrap();
        let bogus = PathBuf::from("/tmp/shannon-nope-does-not-exist-12345");
        let result = run_detection_in(candidates_dir.path(), &bogus, 7, 2, 3);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[test]
    fn run_detection_skips_unreadable_logs_without_failing() {
        let dir = tempdir().unwrap();
        for _ in 0..3 {
            seed_session(dir.path(), &[("bash", &["cmd"])]);
        }
        // A fourth session's log goes corrupt after writing: the listing
        // degrades to directory mtimes and the per-session read skips it,
        // so the run neither fails nor loses the real pattern.
        let corrupt = seed_session(dir.path(), &[("bash", &["cmd"])]);
        let log = dir.path().join(corrupt.to_string()).join("events.jsonl");
        let mut raw = std::fs::read_to_string(&log).unwrap();
        raw.push_str("{not json\n");
        std::fs::write(&log, raw).unwrap();

        let candidates_dir = tempdir().unwrap();
        let appended = run_detection_in(candidates_dir.path(), dir.path(), 7, 2, 3)
            .expect("a corrupt log must not fail the run");
        assert_eq!(appended.len(), 1, "the three healthy sessions still detect");
    }

    #[test]
    fn xxhash_simple_is_deterministic() {
        let a = xxhash_simple("hello");
        let b = xxhash_simple("hello");
        let c = xxhash_simple("world");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
