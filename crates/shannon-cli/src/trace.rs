//! # `shannon trace` — inspect, replay, diff, and export L0 session logs
//!
//! The session event log (`<container>/<uuid>/events.jsonl`) is the single
//! authoritative record (§4.6). These subcommands are its human surface:
//!
//! - `trace show <session> [--turn N] [--tool X] [--permission]` — filtered,
//!   human-readable listing of durable rows.
//! - `trace replay <session>` — time-compressed re-render of the whole
//!   session (streaming chunks folded into their steps).
//! - `trace diff <a> <b>` — seq/kind/payload-digest comparison of two logs.
//! - `trace export <session> [--out DIR]` — events + derived analytics +
//!   metadata bundle for evaluation and sharing.
//!
//! Rendering is a pure function over event bodies so replay output is
//! byte-identical between in-process streams and disk reads (the §4.6 ①
//! invariant) and stable under `insta` snapshots.

use std::path::{Path, PathBuf};

use shannon_core::session_log::{
    SessionLogReader, SessionStore, project_analytics_jsonl, session_log_container_path,
};
use shannon_types::session_event::{SessionEvent, SessionEventBody};

// ============================================================================
// Resolution helpers
// ============================================================================

/// Resolve the sessions container for trace commands.
///
/// Precedence mirrors the persistence stack: `--dir` wins, then
/// `SHANNON_SESSIONS_DIR`, then `SHANNON_HOME/sessions`, else
/// `~/.shannon/sessions`.
pub fn resolve_container(dir: Option<&Path>) -> PathBuf {
    if let Some(d) = dir {
        return d.to_path_buf();
    }
    if let Ok(redirect) = std::env::var("SHANNON_SESSIONS_DIR") {
        if !redirect.trim().is_empty() {
            return PathBuf::from(redirect);
        }
    }
    SessionStore::default_container()
}

/// Resolve a user-supplied session reference to `(uuid, events_path)`.
///
/// Accepts a full UUID, or the literal `latest` / `-` for the most recent
/// log. Errors name the missing file rather than scanning silently.
pub fn resolve_session(container: &Path, reference: &str) -> anyhow::Result<(String, PathBuf)> {
    let store = SessionStore::new(container);
    let id: String = match reference {
        "latest" | "-" => store
            .list()?
            .into_iter()
            .next()
            .map(|info| info.session_id.to_string())
            .ok_or_else(|| anyhow::anyhow!("no sessions found in {}", container.display()))?,
        other => {
            // Accept any prefix long enough to be unambiguous.
            uuid::Uuid::parse_str(other)
                .map(|u| u.to_string())
                .or_else(|_| {
                    let matches = store
                        .list()?
                        .into_iter()
                        .filter(|info| info.session_id.to_string().starts_with(other));
                    let mut found = Vec::new();
                    for info in matches {
                        found.push(info.session_id.to_string());
                        if found.len() > 1 {
                            break;
                        }
                    }
                    match found.len() {
                        1 => Ok(found.remove(0)),
                        0 => Err(anyhow::anyhow!(
                            "session '{other}' not found in {}",
                            container.display()
                        )),
                        _ => Err(anyhow::anyhow!(
                            "session prefix '{other}' is ambiguous; pass more characters"
                        )),
                    }
                })?
        }
    };
    let path = session_log_container_path(container, &id);
    if !path.exists() {
        return Err(anyhow::anyhow!("no events.jsonl for {id} at {path:?}"));
    }
    Ok((id.clone(), path))
}

fn read_events(path: &Path) -> anyhow::Result<Vec<SessionEvent>> {
    SessionLogReader::open(path)
        .and_then(|r| r.read_events(false))
        .map_err(|e| anyhow::anyhow!("{e}"))
}

// ============================================================================
// Rendering
// ============================================================================

/// One rendered line of a session's visible flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderRow {
    /// Sequence number of the originating row.
    pub seq: u64,
    /// Envelope turn number.
    pub turn: u64,
    /// Fully composed display line (stable formatting, no colors).
    pub line: String,
}

struct AssistantFold {
    first_seq: u64,
    turn: u64,
    text: String,
}

fn truncate_oneline(s: &str, max: usize) -> String {
    let single = s.replace(['\n', '\r'], " ");
    if single.chars().count() <= max {
        single
    } else {
        let mut end = max.saturating_sub(1);
        while !single.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &single[..end])
    }
}

fn format_usage(usage: &shannon_types::session_event::TokenUsage) -> String {
    match usage.cost_usd {
        Some(cost) => format!(
            "in={} out={} cache_w={} cache_r=${:.4}",
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_creation_tokens + usage.cache_read_tokens,
            cost
        ),
        None => format!(
            "in={} out={} cache={}",
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_creation_tokens + usage.cache_read_tokens,
        ),
    }
}

/// Fold an ordered body stream into [`RenderRow`]s.
///
/// This is the *shared* renderer behind `show`, `replay`, and the snapshot
/// tests: streaming deltas accumulate into one assistant row per step, and
/// every audit kind (permissions, hooks, errors, turn boundaries) renders
/// exactly one line. Pure and deterministic — same rows in, same lines out.
pub fn render_rows(events: &[SessionEvent]) -> Vec<RenderRow> {
    let mut rows = Vec::new();
    let mut assistant: Option<AssistantFold> = None;

    fn flush(fold: &mut Option<AssistantFold>, rows: &mut Vec<RenderRow>) {
        if let Some(fold) = fold.take() {
            if !fold.text.is_empty() {
                rows.push(RenderRow {
                    seq: fold.first_seq,
                    turn: fold.turn,
                    line: format!("assistant ▸ {}", truncate_oneline(&fold.text, 200)),
                });
            }
        }
    }

    for event in events {
        match &event.body {
            SessionEventBody::SessionStart(p) => {
                flush(&mut assistant, &mut rows);
                rows.push(RenderRow {
                    seq: event.seq,
                    turn: event.turn,
                    line: format!(
                        "session ▸ model={}{}",
                        p.model,
                        p.provider
                            .as_deref()
                            .map_or_else(String::new, |pr| format!(", provider={pr}"))
                    ),
                });
            }
            SessionEventBody::TurnStart(_) => {
                flush(&mut assistant, &mut rows);
                rows.push(RenderRow {
                    seq: event.seq,
                    turn: event.turn,
                    line: "turn ▸ start".into(),
                });
            }
            SessionEventBody::UserMessage(p) => {
                flush(&mut assistant, &mut rows);
                rows.push(RenderRow {
                    seq: event.seq,
                    turn: event.turn,
                    line: format!("user ▸ {}", truncate_oneline(&p.content, 200)),
                });
            }
            SessionEventBody::AssistantChunk(p) => {
                if p.thinking {
                    continue;
                }
                match &mut assistant {
                    Some(fold) => fold.text.push_str(&p.delta),
                    None => {
                        assistant = Some(AssistantFold {
                            first_seq: event.seq,
                            turn: event.turn,
                            text: p.delta.clone(),
                        })
                    }
                }
            }
            SessionEventBody::AssistantMessage(p) => {
                flush(&mut assistant, &mut rows);
                rows.push(RenderRow {
                    seq: event.seq,
                    turn: event.turn,
                    line: format!("assistant ▸ {}", truncate_oneline(&p.content, 200)),
                });
            }
            SessionEventBody::ToolCall(p) => {
                flush(&mut assistant, &mut rows);
                rows.push(RenderRow {
                    seq: event.seq,
                    turn: event.turn,
                    line: format!(
                        "tool ▸ {} ({})",
                        p.tool_name,
                        truncate_oneline(&p.arguments, 160)
                    ),
                });
            }
            SessionEventBody::ToolResult(p) => {
                rows.push(RenderRow {
                    seq: event.seq,
                    turn: event.turn,
                    line: format!(
                        "{} ▸ [{}] {}",
                        if p.is_error { "tool-err" } else { "tool-ok" },
                        p.tool_name,
                        truncate_oneline(&p.output, 160)
                    ),
                });
            }
            SessionEventBody::PermissionDecision(p) => {
                rows.push(RenderRow {
                    seq: event.seq,
                    turn: event.turn,
                    line: format!(
                        "permission ▸ {} tool={}",
                        p.decision,
                        p.tool_name.as_deref().unwrap_or("?")
                    ),
                });
            }
            SessionEventBody::HookFired(p) => {
                rows.push(RenderRow {
                    seq: event.seq,
                    turn: event.turn,
                    line: format!(
                        "hook ▸ {} → {} ({})",
                        p.event,
                        p.hook,
                        p.outcome.as_deref().unwrap_or("?")
                    ),
                });
            }
            SessionEventBody::TurnEnd(p) => {
                flush(&mut assistant, &mut rows);
                let usage_note = p
                    .usage
                    .as_ref()
                    .map(format_usage)
                    .unwrap_or_else(|| "no usage recorded".into());
                rows.push(RenderRow {
                    seq: event.seq,
                    turn: event.turn,
                    line: format!("turn ▸ end reason={} ({})", p.reason, usage_note),
                });
            }
            SessionEventBody::Error(p) => {
                flush(&mut assistant, &mut rows);
                rows.push(RenderRow {
                    seq: event.seq,
                    turn: event.turn,
                    line: format!("error ▸ {}: {}", p.category, p.message),
                });
            }
            SessionEventBody::RequestHeader(p) => {
                flush(&mut assistant, &mut rows);
                rows.push(RenderRow {
                    seq: event.seq,
                    turn: event.turn,
                    line: format!(
                        "request ▸ header model={} tools={} reason={}",
                        p.model,
                        p.tools.len(),
                        p.reason.as_deref().unwrap_or("?")
                    ),
                });
            }
            SessionEventBody::TodoWrite(_)
            | SessionEventBody::SurfaceAppend(_)
            | SessionEventBody::SurfaceReplace(_) => {
                // Internal bookkeeping kinds do not render yet.
            }
            SessionEventBody::RequestContext(_) | SessionEventBody::SessionEndSeed(_) => {}
            SessionEventBody::Custom(_) => {}
        }
    }
    flush(&mut assistant, &mut rows);
    rows
}

/// Render `trace show` — optionally narrowed by turn, tool, or permissions.
pub fn cmd_show(
    container: &Path,
    reference: &str,
    turn: Option<u64>,
    tool: Option<String>,
    permission_only: bool,
) -> anyhow::Result<String> {
    let (_id, path) = resolve_session(container, reference)?;
    let events = read_events(&path)?;
    let tool_filter = tool.as_deref();

    let out: Vec<RenderRow> = render_rows(&events)
        .into_iter()
        .filter(|row| turn.is_none_or(|t| row.turn == t))
        .filter(|row| {
            if permission_only {
                row.line.starts_with("permission ")
            } else {
                true
            }
        })
        .filter(|row| match tool_filter {
            Some(name) => row.line.contains("tool ▸") && row.line.contains(name),
            None => true,
        })
        .collect();

    if out.is_empty() {
        return Ok("(no matching rows)".into());
    }
    Ok(out
        .into_iter()
        .map(|r| format!("#{:03} t{} {}", r.seq, r.turn, r.line))
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Render `trace replay` — the whole session with chunk folding applied.
pub fn cmd_replay(container: &Path, reference: &str) -> anyhow::Result<String> {
    let (_id, path) = resolve_session(container, reference)?;
    let events = read_events(&path)?;
    Ok(render_rows(&events)
        .into_iter()
        .map(|r| format!("#{:03} t{} {}", r.seq, r.turn, r.line))
        .collect::<Vec<_>>()
        .join("\n"))
}

// ============================================================================
// Diff
// ============================================================================

/// One divergent position between two logs.
#[derive(Debug, Clone)]
pub struct DiffEntry {
    pub seq: u64,
    pub kind_left: Option<String>,
    pub kind_right: Option<String>,
    /// 12-hex payload digest (sha256 prefix) per side when present.
    pub digest_left: Option<String>,
    pub digest_right: Option<String>,
}

#[derive(Debug, Default)]
pub struct DiffReport {
    pub entries: Vec<DiffEntry>,
    pub total_left: usize,
    pub total_right: usize,
}

fn payload_digest(event: &SessionEvent) -> String {
    use sha2::{Digest, Sha256};
    let sans_volatile =
        serde_json::json!({ "kind": event.kind().as_str(), "body": strip_envelope(&event.body) });
    let bytes = serde_json::to_vec(&sans_volatile).unwrap_or_default();
    let hash = Sha256::digest(bytes);
    hex_prefix(&hash)
}

fn strip_envelope(body: &SessionEventBody) -> serde_json::Value {
    // Redact volatile wire captures so two logs differing only in request
    // bytes compare on structure + args, not timestamps/model versions.
    match body {
        SessionEventBody::RequestHeader(p) => serde_json::json!({
            "model": p.model,
            "tools": p.tools.iter().map(|t| t.name.clone()).collect::<Vec<_>>(),
            "reason": p.reason,
        }),
        other => serde_json::to_value(other).unwrap_or(serde_json::Value::Null),
    }
}

fn hex_prefix(digest: &[u8]) -> String {
    digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// Compare two logs at seq/kind/payload-digest granularity.
pub fn diff_events(left: &[SessionEvent], right: &[SessionEvent]) -> DiffReport {
    let mut report = DiffReport {
        total_left: left.len(),
        total_right: right.len(),
        ..Default::default()
    };
    let max = left.len().max(right.len());
    for seq in 0..max {
        let l = left.get(seq);
        let r = right.get(seq);
        let mismatch = match (l, r) {
            (None, None) => false,
            (Some(a), Some(b)) => a.kind() != b.kind() || payload_digest(a) != payload_digest(b),
            _ => true,
        };
        if mismatch {
            report.entries.push(DiffEntry {
                seq: seq as u64,
                kind_left: l.map(|e| e.kind().as_str().to_string()),
                kind_right: r.map(|e| e.kind().as_str().to_string()),
                digest_left: l.map(payload_digest),
                digest_right: r.map(payload_digest),
            });
        }
    }
    report
}

/// Render `trace diff`.
pub fn cmd_diff(container: &Path, left_ref: &str, right_ref: &str) -> anyhow::Result<String> {
    let (_, path_l) = resolve_session(container, left_ref)?;
    let (_, path_r) = resolve_session(container, right_ref)?;
    let left = read_events(&path_l)?;
    let right = read_events(&path_r)?;
    let report = diff_events(&left, &right);

    let mut lines = vec![format!(
        "left={} rows={}; right={} rows={}; divergences={}",
        left_ref,
        report.total_left,
        right_ref,
        report.total_right,
        report.entries.len()
    )];
    for entry in report.entries.iter().take(50) {
        lines.push(format!(
            "#{:03} {:?} | {:?}  digests {} | {}",
            entry.seq,
            entry.kind_left.as_deref().unwrap_or("<none>"),
            entry.kind_right.as_deref().unwrap_or("<none>"),
            entry.digest_left.as_deref().unwrap_or("<none>"),
            entry.digest_right.as_deref().unwrap_or("<none>")
        ));
    }
    if report.entries.len() > 50 {
        lines.push(format!("… and {} more", report.entries.len() - 50));
    }
    if report.entries.is_empty() {
        lines.push("logs are equivalent at this granularity".into());
    }
    Ok(lines.join("\n"))
}

// ============================================================================
// Export
// ============================================================================

/// Write everything evaluation or sharing needs into `out_dir/<session>/`:
/// the authoritative `events.jsonl`, the derived analytics aggregate row,
/// a compact `summary.json`, and `meta.json` when present.
pub fn cmd_export(
    container: &Path,
    reference: &str,
    out_dir: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    let store = SessionStore::new(container);
    let (id, path) = resolve_session(container, reference)?;
    let events = read_events(&path)?;

    let destination = out_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::temp_dir().join("shannon-trace-export"));
    let target = destination.join(&id);
    std::fs::create_dir_all(&target)?;

    // 1. Authoritative events.
    std::fs::copy(&path, target.join("events.jsonl"))?;

    // 2. Derived analytics aggregate (JSONL).
    let mut analytics = project_analytics_jsonl(&events);
    if !analytics.ends_with('\n') {
        analytics.push('\n');
    }
    std::fs::write(target.join("analytics.jsonl"), analytics)?;

    // 3. Sidecar meta when present.
    let meta_path = session_meta_path(container, &id);
    if meta_path.exists() {
        std::fs::copy(&meta_path, target.join("meta.json"))?;
    }

    // 4. Compact summary (projection-level facts, no conversation text).
    let summary = build_summary(store.load(&uuid::Uuid::parse_str(&id)?)?.as_ref(), &events)?;
    std::fs::write(target.join("summary.json"), summary)?;

    eprintln!("exported {id} → {}", target.display());
    Ok(target)
}

fn session_meta_path(container: &Path, id: &str) -> PathBuf {
    shannon_core::session_log::session_meta_container_path(container, id)
}

fn build_summary(
    stored: Option<&shannon_core::session_log::StoredSession>,
    events: &[SessionEvent],
) -> anyhow::Result<String> {
    use shannon_types::session_event::TokenUsage;

    let mut totals = TokenUsage {
        input_tokens: 0,
        output_tokens: 0,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
        cost_usd: None,
    };
    let mut turns = 0usize;
    for event in events {
        if let SessionEventBody::TurnEnd(p) = &event.body {
            turns += 1;
            if let Some(u) = &p.usage {
                totals.input_tokens += u.input_tokens;
                totals.output_tokens += u.output_tokens;
                totals.cache_creation_tokens += u.cache_creation_tokens;
                totals.cache_read_tokens += u.cache_read_tokens;
                totals.cost_usd = match (totals.cost_usd, u.cost_usd) {
                    (Some(a), Some(b)) => Some(a + b),
                    (Some(a), None) | (None, Some(a)) => Some(a),
                    _ => totals.cost_usd,
                };
            }
        }
    }

    let value = serde_json::json!({
        "messages": stored.map(|s| s.messages.len()).unwrap_or(0),
        "turn_count": stored.map(|s| s.metadata.turn_count).unwrap_or(turns),
        "model": stored.and_then(|s| (!s.metadata.model.is_empty()).then(|| s.metadata.model.clone())),
        "project_path": stored.and_then(|s| s.metadata.project_path.clone()),
        "title": stored.and_then(|s| s.metadata.title.clone()),
        "created_at": stored.map(|s| s.metadata.created_at.to_rfc3339()),
        "updated_at": stored.map(|s| s.metadata.updated_at.to_rfc3339()),
        "token_totals": {
            "input": totals.input_tokens,
            "output": totals.output_tokens,
            "cache_write": totals.cache_creation_tokens,
            "cache_read": totals.cache_read_tokens,
            "cost_usd": totals.cost_usd,
        },
        "event_count": events.len(),
    });
    Ok(serde_json::to_string_pretty(&value)?)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_types::session_event::{
        PermissionDecisionPayload, TokenUsage, ToolCallPayload, ToolResultPayload, TurnEndPayload,
        UserMessagePayload,
    };

    fn seed(container: &Path, id: &str) -> Vec<SessionEvent> {
        use shannon_core::session_log::SessionLogWriter;
        std::fs::create_dir_all(container).unwrap();
        let mut w = SessionLogWriter::open_layout(container, id).unwrap();
        w.record(SessionEventBody::UserMessage(UserMessagePayload {
            source: UserMessagePayload::SOURCE_USER.into(),
            content: "run ls".into(),
        }));
        w.record(SessionEventBody::ToolCall(ToolCallPayload {
            tool_use_id: "t1".into(),
            tool_name: "Bash".into(),
            arguments: r#"{"command":"ls"}"#.into(),
        }));
        w.record(SessionEventBody::PermissionDecision(
            PermissionDecisionPayload {
                tool_name: Some("Bash".into()),
                request: None,
                decision: "allow".into(),
                reason: Some("safe".into()),
                mode: Some("auto".into()),
            },
        ));
        w.record(SessionEventBody::ToolResult(ToolResultPayload {
            tool_use_id: "t1".into(),
            tool_name: "Bash".into(),
            output: "src".into(),
            is_error: false,
            duration_ms: Some(2),
            meta: serde_json::Value::Null,
        }));
        w.record(SessionEventBody::TurnEnd(TurnEndPayload {
            reason: TurnEndPayload::REASON_COMPLETED.into(),
            usage: Some(TokenUsage {
                input_tokens: 12,
                output_tokens: 7,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
                cost_usd: Some(0.01),
            }),
            error: None,
        }));
        w.close().unwrap();

        let path = session_log_container_path(container, id);
        SessionLogReader::open(&path)
            .and_then(|r| r.read_events(false))
            .expect("read back")
    }

    #[test]
    fn render_rows_fold_chunks_and_render_audit_lines() {
        let dir = tempfile::tempdir().unwrap();
        let container = dir.path().join("sessions");
        let id = "11111111-2222-3333-4444-555555555555";
        let events = seed(&container, id);

        let mut folded =
            shannon_core::session_log::SessionLogWriter::open_layout(&container, "second").unwrap();
        folded.record(SessionEventBody::AssistantChunk(
            shannon_types::session_event::AssistantChunkPayload {
                delta: "Hel".into(),
                thinking: false,
            },
        ));
        folded.record(SessionEventBody::AssistantChunk(
            shannon_types::session_event::AssistantChunkPayload {
                delta: "lo".into(),
                thinking: false,
            },
        ));
        folded.close().unwrap();
        let more = SessionLogReader::open(&session_log_container_path(&container, "second"))
            .and_then(|r| r.read_events(true))
            .unwrap();

        let mut all = render_rows(&events);
        all.extend(render_rows(&more));
        let lines: Vec<&str> = all.iter().map(|r| r.line.as_str()).collect();
        assert!(lines.iter().any(|l| l.starts_with("user ▸ run ls")));
        assert!(lines.iter().any(|l| l.starts_with("tool ▸ Bash ")));
        assert!(
            lines
                .iter()
                .any(|l| l == &String::from("permission ▸ allow tool=Bash"))
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("turn ▸ end") && l.contains("reason=completed")),
            "rendered turn/end with usage: {lines:?}"
        );
        // Two consecutive half-chunks fold into exactly one assistant row.
        assert_eq!(
            lines.iter().filter(|l| **l == "assistant ▸ Hello").count(),
            1
        );
    }

    #[test]
    fn cmd_show_filters_by_turn_tool_and_permission() {
        let dir = tempfile::tempdir().unwrap();
        let container = dir.path().join("sessions");
        seed(&container, "22222222-3333-4444-5555-666666666666");

        let all = cmd_show(&container, "latest", None, None, false).expect("show all");
        assert!(all.contains("tool ▸ Bash"));
        assert!(all.contains("permission ▸"));

        let perms = cmd_show(&container, "latest", None, None, true).expect("show permissions");
        assert!(perms.contains("permission ▸ allow"));
        assert!(!perms.contains("tool ▸"));

        let tools = cmd_show(&container, "latest", None, Some("Bash".into()), false)
            .expect("show tool-filtered");
        assert!(tools.contains("tool ▸ Bash"));
        assert!(!tools.contains("permission"));

        let none = cmd_show(&container, "latest", Some(99), None, false).unwrap();
        assert_eq!(none, "(no matching rows)");
    }

    #[test]
    fn cmd_diff_reports_identity_for_equal_logs_and_divergence_otherwise() {
        let dir = tempfile::tempdir().unwrap();
        let container = dir.path().join("sessions");
        let a = "aaaaaaa1-0000-0000-0000-000000000001";
        let b = "aaaaaaa2-0000-0000-0000-000000000002";
        seed(&container, a);
        seed(&container, b);

        let same = cmd_diff(&container, a, b).unwrap();
        assert!(same.contains("divergences=0"), "{same}");

        // Mutate `b`: append an extra row → divergence at that seq.
        let mut w =
            shannon_core::session_log::SessionLogWriter::open_layout(&container, b).unwrap();
        w.record(SessionEventBody::Error(
            shannon_types::session_event::ErrorPayload {
                category: "extra".into(),
                message: "diverged".into(),
                detail: None,
            },
        ));
        w.close().unwrap();
        let different = cmd_diff(&container, a, b).unwrap();
        assert!(different.contains("rows=6"), "{different}");
        assert!(different.contains("error"), "{different}");
    }

    #[test]
    fn cmd_export_writes_bundle_with_analytics_and_summary() {
        let home = tempfile::tempdir().unwrap();
        let container = home.path().join("sessions");
        let id = "bbbbbbb1-0000-0000-0000-000000000009";
        seed(&container, id);

        let out_root = tempfile::tempdir().unwrap();
        let dest = cmd_export(&container, id, Some(out_root.path())).expect("export ok");

        assert!(dest.join("events.jsonl").is_file());
        assert!(dest.join("analytics.jsonl").is_file());
        assert!(dest.join("summary.json").is_file());

        let analytics = std::fs::read_to_string(dest.join("analytics.jsonl")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(analytics.trim_end()).unwrap();
        assert_eq!(parsed["prompts_submitted"], 1);

        let summary = std::fs::read_to_string(dest.join("summary.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&summary).unwrap();
        assert_eq!(parsed["event_count"], 5);
        assert_eq!(parsed["token_totals"]["input"], 12);
    }

    #[test]
    fn resolve_session_supports_latest_and_prefixes() {
        let dir = tempfile::tempdir().unwrap();
        let container = dir.path().join("sessions");
        let full = "ccccccc1-0000-0000-0000-000000000007";
        seed(&container, full);

        let (resolved_id, path) = resolve_session(&container, "latest").unwrap();
        assert_eq!(resolved_id, full);
        assert!(path.ends_with("events.jsonl"));
        assert_eq!(resolve_session(&container, "ccccccc1").unwrap().0, full);
        assert!(
            resolve_session(&container, "ddddddd").is_err(),
            "unknown prefix errors"
        );
    }

    #[test]
    fn digest_is_stable_across_identical_bodies() {
        let dir = tempfile::tempdir().unwrap();
        let container = dir.path().join("sessions");
        let events_a = seed(&container, "eeeeeee1-0000-0000-0000-000000000001");
        // Same body stream written again → identical digests.
        let events_b = seed(
            &dir.path().join("sessions2"),
            "eeeeeee2-0000-0000-0000-000000000002",
        );

        for (a, b) in events_a.iter().zip(events_b.iter()) {
            assert_eq!(payload_digest(a), payload_digest(b));
        }
    }
}
