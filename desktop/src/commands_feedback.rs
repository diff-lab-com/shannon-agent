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
}
