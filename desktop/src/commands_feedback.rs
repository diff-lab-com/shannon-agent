//! PM-12 feedback signal store — persists per-message 👍/👎 so reactions
//! survive reloads and become a real quality signal instead of a local
//! `useState` that dies with the component tree.
//!
//! Storage: `<SHANNON_HOME|~/.shannon>/feedback/<session_id>.json`, a single
//! JSON object mapping message keys (`"<timestamp>:<content-hash>"`, built by
//! the frontend) to `"up" | "down"`. Per-session files keep the write path
//! lock-free and trivially inspectable; toggling a rating off simply removes
//! the key.

use std::collections::HashMap;
use std::path::PathBuf;

fn feedback_dir() -> PathBuf {
    if let Ok(home) = std::env::var("SHANNON_HOME") {
        return PathBuf::from(home).join("feedback");
    }
    match dirs::home_dir() {
        Some(home) => home.join(".shannon").join("feedback"),
        None => std::env::temp_dir().join(".shannon").join("feedback"),
    }
}

fn feedback_path(session_id: &str) -> Result<PathBuf, String> {
    if session_id.is_empty() || session_id.contains('/') || session_id.contains('\\') || session_id.contains("..") {
        return Err("invalid session id".into());
    }
    Ok(feedback_dir().join(format!("{session_id}.json")))
}

fn read_map(session_id: &str) -> HashMap<String, String> {
    let Ok(path) = feedback_path(session_id) else {
        return HashMap::new();
    };
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

fn write_map(session_id: &str, map: &HashMap<String, String>) -> Result<(), String> {
    let path = feedback_path(session_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

fn is_valid_rating(rating: &str) -> bool {
    matches!(rating, "up" | "down")
}

/// Persist (or, with `rating: None`, clear) the feedback for one message.
#[tauri::command]
pub async fn record_message_feedback(
    session_id: String,
    key: String,
    rating: Option<String>,
) -> Result<(), String> {
    if key.is_empty() {
        return Err("empty feedback key".into());
    }
    if let Some(r) = &rating {
        if !is_valid_rating(r) {
            return Err(format!("invalid rating: {r}"));
        }
    }
    let mut map = read_map(&session_id);
    match rating.as_deref() {
        Some(r) => {
            map.insert(key, r.to_string());
        }
        None => {
            map.remove(&key);
        }
    }
    write_map(&session_id, &map)
}

/// All feedback recorded for a session (missing file → empty map).
#[tauri::command]
pub async fn list_message_feedback(
    session_id: String,
) -> Result<HashMap<String, String>, String> {
    Ok(read_map(&session_id))
}

/// PM-12 display half: per-session 👍/👎 aggregates so the collected signal
/// is actually visible. Only sessions with at least one rating are listed,
/// most recently updated first.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FeedbackSessionSummary {
    pub session_id: String,
    pub up: usize,
    pub down: usize,
    /// File mtime (epoch seconds) — a proxy for "last feedback activity".
    pub updated_at: i64,
}

pub fn list_feedback_sessions_impl() -> Vec<FeedbackSessionSummary> {
    let dir = feedback_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<FeedbackSessionSummary> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&raw) else {
            continue;
        };
        let up = map.values().filter(|r| r.as_str() == "up").count();
        let down = map.values().filter(|r| r.as_str() == "down").count();
        if up + down == 0 {
            continue;
        }
        let Some(session_id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let updated_at = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        out.push(FeedbackSessionSummary { session_id: session_id.to_string(), up, down, updated_at });
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.session_id.cmp(&b.session_id)));
    out
}

#[tauri::command]
pub async fn list_feedback_sessions() -> Result<Vec<FeedbackSessionSummary>, String> {
    Ok(list_feedback_sessions_impl())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal_session_ids() {
        assert!(feedback_path("../evil").is_err());
        assert!(feedback_path("a/b").is_err());
        assert!(feedback_path("").is_err());
        assert!(feedback_path("3f2a9c1e-1234-5678-9abc-def012345678").is_ok());
    }

    #[test]
    fn rating_vocabulary_is_up_or_down() {
        assert!(is_valid_rating("up"));
        assert!(is_valid_rating("down"));
        assert!(!is_valid_rating("meh"));
    }

    #[test]
    fn list_feedback_sessions_aggregates_per_session() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("feedback");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("sess-a.json"),
            r#"{"k1":"up","k2":"down","k3":"up"}"#,
        )
        .unwrap();
        std::fs::write(dir.join("sess-b.json"), r#"{"k1":"down"}"#).unwrap();
        std::fs::write(dir.join("sess-empty.json"), "{}").unwrap();
        std::fs::write(dir.join("ignore.txt"), "not json").unwrap();

        let prev = std::env::var("SHANNON_HOME").ok();
        unsafe { std::env::set_var("SHANNON_HOME", tmp.path()) };
        let mut summaries = list_feedback_sessions_impl();
        match prev {
            Some(prev) => unsafe { std::env::set_var("SHANNON_HOME", prev) },
            None => unsafe { std::env::remove_var("SHANNON_HOME") },
        }

        summaries.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].session_id, "sess-a");
        assert_eq!(summaries[0].up, 2);
        assert_eq!(summaries[0].down, 1);
        assert_eq!(summaries[1].session_id, "sess-b");
        assert_eq!(summaries[1].down, 1);
    }
}
