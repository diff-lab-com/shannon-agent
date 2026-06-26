//! Skill-pattern detection — scans recent sessions for recurring tool-call
//! sequences and writes new SkillCandidate entries to the candidates JSONL.
//!
//! Detection runs on-demand via the `trigger_skill_pattern_detection`
//! command and can be wired into a daily routine by the scheduled-tasks
//! layer. The algorithm:
//!
//! 1. Walk `~/.shannon/sessions/*.json` filtered by mtime >= `days_back`.
//! 2. For each session, extract tool-use blocks from assistant messages.
//! 3. Compute a normalized signature per session (tool_name + sorted arg
//!    keys, joined by →).
//! 4. Group identical signatures across sessions.
//! 5. Signatures seen in `min_sessions`+ distinct sessions and
//!    `min_occurrences`+ total occurrences become new candidates.
//!
//! New candidates are appended to the existing JSONL via
//! [`crate::commands_skill_candidates::append_candidate`]; existing
//! candidate ids are left untouched so approval flows aren't disrupted.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::commands_skill_candidates::{SkillCandidate, SourceToolCall, append_candidate};

/// Threshold sessions for a pattern to qualify as a candidate.
const DEFAULT_MIN_SESSIONS: usize = 2;
/// Threshold total occurrences across sessions.
const DEFAULT_MIN_OCCURRENCES: u32 = 3;

#[derive(Debug, Deserialize)]
struct SessionFile {
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    messages: Vec<serde_json::Value>,
}

/// Extract a stable signature from a tool_use block: name + sorted arg keys.
fn signature_of(tool_name: &str, input: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut keys: Vec<&str> = input.keys().map(|s| s.as_str()).collect();
    keys.sort();
    format!("{tool_name}({})", keys.join(","))
}

/// Walk assistant message content arrays for tool_use blocks and return
/// their signatures in encounter order.
#[cfg(test)]
fn extract_tool_signatures(msgs: &[serde_json::Value]) -> Vec<String> {
    let mut out = Vec::new();
    for msg in msgs {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role != "assistant" {
            continue;
        }
        let content = match msg.get("content") {
            Some(serde_json::Value::Array(a)) => a,
            _ => continue,
        };
        for block in content {
            let btype = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if btype != "tool_use" && btype != "tool_call" {
                continue;
            }
            let name = block
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let empty = serde_json::Map::new();
            let input_obj: &serde_json::Map<String, serde_json::Value> = match block.get("input").or_else(|| block.get("args")) {
                Some(serde_json::Value::Object(m)) => m,
                _ => &empty,
            };
            out.push(signature_of(name, input_obj));
        }
    }
    out
}

/// Find session files modified within `days_back` days under the sessions dir.
fn list_recent_sessions(sessions_dir: &Path, days_back: u32) -> Result<Vec<PathBuf>, String> {
    if !sessions_dir.is_dir() {
        return Ok(Vec::new());
    }
    let cutoff_secs: u64 = u64::from(days_back) * 86400;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("system clock: {e}"))?
        .as_secs();
    let cutoff = now.saturating_sub(cutoff_secs);

    let mut paths = Vec::new();
    for entry in std::fs::read_dir(sessions_dir).map_err(|e| format!("readdir: {e}"))? {
        let entry = entry.map_err(|e| format!("readdir entry: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if mtime >= cutoff {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

/// Load and parse a session file. Returns None on parse failure (logged + skipped).
fn load_session(path: &Path) -> Option<SessionFile> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice::<SessionFile>(&bytes).ok()
}

/// Aggregate: how often a signature appeared, and which sessions it appeared in.
#[derive(Default)]
struct SignatureAgg {
    sessions: std::collections::HashSet<String>,
    total: u32,
    sample_tool: String,
    sample_args: serde_json::Map<String, serde_json::Value>,
    sample_session_ids: Vec<String>,
}

/// Run pattern detection. Returns the number of new candidates appended.
/// `sessions_dir` is normally `~/.shannon/sessions/` but injected for testability.
pub fn run_detection(
    sessions_dir: &Path,
    days_back: u32,
    min_sessions: usize,
    min_occurrences: u32,
) -> Result<usize, String> {
    let paths = list_recent_sessions(sessions_dir, days_back)?;
    let mut aggregates: HashMap<String, SignatureAgg> = HashMap::new();

    for path in paths {
        let session = match load_session(&path) {
            Some(s) => s,
            None => continue,
        };
        let session_id = if session.session_id.is_empty() {
            path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string()
        } else {
            session.session_id
        };

        for msg in &session.messages {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if role != "assistant" {
                continue;
            }
            let content = match msg.get("content") {
                Some(serde_json::Value::Array(a)) => a,
                _ => continue,
            };
            for block in content {
                let btype = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if btype != "tool_use" && btype != "tool_call" {
                    continue;
                }
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let input_obj = match block.get("input").or_else(|| block.get("args")) {
                    Some(serde_json::Value::Object(m)) => m.clone(),
                    _ => serde_json::Map::new(),
                };
                let sig = signature_of(&name, &input_obj);
                let agg = aggregates.entry(sig).or_insert_with(|| SignatureAgg {
                    sample_tool: name.clone(),
                    sample_args: input_obj.clone(),
                    ..SignatureAgg::default()
                });
                agg.total += 1;
                if agg.sessions.insert(session_id.clone()) {
                    agg.sample_session_ids.push(session_id.clone());
                }
            }
        }
    }

    let mut appended = 0usize;
    let now = chrono::Utc::now().to_rfc3339();
    for (_sig, agg) in aggregates.iter() {
        if agg.sessions.len() < min_sessions || agg.total < min_occurrences {
            continue;
        }
        let id = format!("sig-{:x}", xxhash_simple(&format!("{}|{:?}", agg.sample_tool, agg.sample_session_ids)));
        let candidate = SkillCandidate {
            id,
            detected_at: now.clone(),
            occurrence_count: agg.total,
            example_session_ids: agg.sample_session_ids.iter().take(5).cloned().collect(),
            proposed_name: agg.sample_tool.clone(),
            proposed_trigger: format!("Detected {} call recurring across {} session(s)", agg.sample_tool, agg.sessions.len()),
            procedure: vec![format!("Invoke {} with the same argument shape", agg.sample_tool)],
            source_tool_calls: vec![SourceToolCall {
                tool: agg.sample_tool.clone(),
                args_summary: agg.sample_args.iter().map(|(k, _)| (k.clone(), serde_json::Value::Null)).collect(),
            }],
            refined: false,
        };
        append_candidate(candidate)?;
        appended += 1;
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

/// Default sessions dir: ~/.shannon/sessions/
pub fn default_sessions_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    Ok(home.join(".shannon").join("sessions"))
}

#[tauri::command]
pub async fn trigger_skill_pattern_detection(days_back: Option<u32>) -> Result<usize, String> {
    let dir = default_sessions_dir()?;
    let days = days_back.unwrap_or(7);
    run_detection(&dir, days, DEFAULT_MIN_SESSIONS, DEFAULT_MIN_OCCURRENCES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_session(dir: &Path, name: &str, session_id: &str, tool_calls: &[(&str, &[&str])]) {
        let mut messages: Vec<serde_json::Value> = Vec::new();
        messages.push(serde_json::json!({"role": "user", "content": "go"}));
        let mut content: Vec<serde_json::Value> = Vec::new();
        for (tool, keys) in tool_calls {
            let mut input = serde_json::Map::new();
            for k in *keys {
                input.insert((*k).to_string(), serde_json::Value::String("x".into()));
            }
            content.push(serde_json::json!({
                "type": "tool_use",
                "name": tool,
                "input": serde_json::Value::Object(input),
            }));
        }
        messages.push(serde_json::json!({"role": "assistant", "content": content}));
        let body = serde_json::json!({
            "session_id": session_id,
            "messages": messages,
        });
        let path = dir.join(name);
        fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();

        // touch mtime to now so it's "recent"
        let file = std::fs::File::open(&path).unwrap();
        let now = std::time::SystemTime::now();
        let _ = file.set_times(std::fs::FileTimes::new().set_modified(now).set_accessed(now));
    }

    #[test]
    fn signature_includes_sorted_arg_keys() {
        let mut m = serde_json::Map::new();
        m.insert("b".into(), serde_json::Value::Null);
        m.insert("a".into(), serde_json::Value::Null);
        assert_eq!(signature_of("bash", &m), "bash(a,b)");
    }

    #[test]
    fn extracts_tool_signatures_from_assistant_blocks() {
        let v: Vec<serde_json::Value> = vec![
            serde_json::json!({"role": "user", "content": "go"}),
            serde_json::json!({
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "ok"},
                    {"type": "tool_use", "name": "bash", "input": {"cmd": "ls"}},
                    {"type": "tool_use", "name": "read_file", "input": {"path": "/x", "mode": "r"}},
                ],
            }),
        ];
        let sigs = extract_tool_signatures(&v);
        assert_eq!(sigs, vec!["bash(cmd)", "read_file(mode,path)"]);
    }

    #[test]
    fn run_detection_appends_candidates_for_recurring_patterns() {
        let dir = tempdir().unwrap();
        // Three sessions, each with the same tool signature.
        write_session(dir.path(), "s1.json", "s1", &[("bash", &["cmd"])]);
        write_session(dir.path(), "s2.json", "s2", &[("bash", &["cmd"])]);
        write_session(dir.path(), "s3.json", "s3", &[("bash", &["cmd"])]);

        // Use a throwaway HOME so the candidates file lands somewhere isolated.
        let fake_home = tempdir().unwrap();
        let prev_home = std::env::var_os("HOME");
        // SAFETY: tests run single-threaded by default within this module; we restore HOME in the finally-style block below.
        unsafe { std::env::set_var("HOME", fake_home.path()); }

        let result = run_detection(dir.path(), 7, 2, 3);

        // SAFETY: same single-threaded context.
        unsafe {
            if let Some(h) = prev_home { std::env::set_var("HOME", h); }
            else { std::env::remove_var("HOME"); }
        }

        let appended = result.expect("detection ran");
        assert_eq!(appended, 1, "expected exactly one new candidate");
    }

    #[test]
    fn run_detection_skips_patterns_below_threshold() {
        let dir = tempdir().unwrap();
        write_session(dir.path(), "s1.json", "s1", &[("bash", &["cmd"])]);

        let fake_home = tempdir().unwrap();
        let prev_home = std::env::var_os("HOME");
        // SAFETY: same single-threaded test context as above.
        unsafe { std::env::set_var("HOME", fake_home.path()); }

        let result = run_detection(dir.path(), 7, 2, 3);

        // SAFETY: same single-threaded context.
        unsafe {
            if let Some(h) = prev_home { std::env::set_var("HOME", h); }
            else { std::env::remove_var("HOME"); }
        }

        let appended = result.expect("detection ran");
        assert_eq!(appended, 0, "single-session pattern should not promote");
    }

    #[test]
    fn run_detection_returns_zero_when_sessions_dir_missing() {
        let bogus = PathBuf::from("/tmp/shannon-nope-does-not-exist-12345");
        let result = run_detection(&bogus, 7, 2, 3);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
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
