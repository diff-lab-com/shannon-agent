//! Tauri commands for the persistent memory layer (P2.1).
//!
//! Thin wrappers around `shannon_core::memory::MemoryStore`. The store is
//! backed by per-project JSON files at `~/.shannon/memories/{hash}.json`.
//!
//! Each command does load → mutate → save (stateless). The store is cheap to
//! construct and load; the typical memory file is a few KB. Holding a locked
//! store in `AppState` would serialize all memory access behind the mutex;
//! the stateless design lets concurrent commands hit separate files without
//! contention.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use shannon_core::memory::{MemoryCategory, MemoryEntry, MemoryStore};

/// Resolve the on-disk memories directory (`~/.shannon/memories/`).
fn storage_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "could not resolve $HOME".to_string())?;
    Ok(home.join(".shannon").join("memories"))
}

/// Construct a fresh store loaded from disk.
fn load_store() -> Result<MemoryStore, String> {
    let path = storage_path()?;
    let mut store = MemoryStore::new(path);
    store.load().map_err(|e| e.to_string())?;
    Ok(store)
}

/// Parse a category string ("preference" / "pattern" / "decision" / "error"
/// / "context") into the engine enum. Case-insensitive. Unknown values fall
/// back to [`MemoryCategory::Context`] rather than erroring so the UI doesn't
/// hard-fail on legacy data.
fn parse_category(s: &str) -> MemoryCategory {
    match s.to_ascii_lowercase().as_str() {
        "preference" => MemoryCategory::Preference,
        "pattern" => MemoryCategory::Pattern,
        "decision" => MemoryCategory::Decision,
        "error" => MemoryCategory::Error,
        _ => MemoryCategory::Context,
    }
}

/// Serialize a [`MemoryEntry`] for Tauri. The engine struct derives Serialize
/// already; we re-export it verbatim so the frontend can pass entries through
/// unchanged.
pub type MemoryEntryDto = MemoryEntry;

/// All entries currently in the store, unsorted. Uses `search("", None)` which
/// matches every entry (empty query is a substring of every content string).
fn all_entries(store: &MemoryStore) -> Vec<MemoryEntry> {
    store.search("", None)
}

/// List distinct project names that have at least one memory entry.
#[tauri::command]
pub async fn list_memory_projects() -> Result<Vec<String>, String> {
    let store = load_store()?;
    let mut projects: Vec<String> = all_entries(&store)
        .into_iter()
        .map(|e| e.project)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    projects.sort();
    Ok(projects)
}

/// List memories, optionally filtered by project, category, or free-text query.
///
/// Sort order: most recently created first.
#[tauri::command]
pub async fn list_memories(
    project: Option<String>,
    category: Option<String>,
    query: Option<String>,
) -> Result<Vec<MemoryEntryDto>, String> {
    let store = load_store()?;
    let mut rows = if query.as_deref().map(str::is_empty).unwrap_or(true) {
        all_entries(&store)
    } else {
        store.search(query.as_deref().unwrap_or(""), project.as_deref())
    };
    rows.retain(|e| {
        if let Some(p) = &project {
            if &e.project != p {
                return false;
            }
        }
        if let Some(c) = &category {
            if e.category != parse_category(c) {
                return false;
            }
        }
        true
    });
    rows.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(rows)
}

/// Create a new memory entry. Returns the created entry with its generated ID.
#[tauri::command]
pub async fn create_memory(
    project: String,
    category: String,
    content: String,
    tags: Option<Vec<String>>,
    confidence: Option<f64>,
) -> Result<MemoryEntryDto, String> {
    let mut store = load_store()?;
    let cat = parse_category(&category);
    let mut entry = MemoryEntry::new(&project, cat, &content);
    if let Some(t) = tags {
        entry.tags = t;
    }
    if let Some(c) = confidence {
        if !(0.0..=1.0).contains(&c) {
            return Err(format!("confidence must be in [0.0, 1.0], got {c}"));
        }
        entry.confidence = c;
    }
    store.add(entry.clone()).map_err(|e| e.to_string())?;
    store.save().map_err(|e| e.to_string())?;
    Ok(entry)
}

/// Update an existing memory entry's mutable fields (content, tags, category).
///
/// Only fields supplied as `Some(...)` are updated; `None` leaves the existing
/// value intact. Returns the updated entry or an error if the ID is unknown.
#[tauri::command]
pub async fn update_memory(
    id: String,
    content: Option<String>,
    tags: Option<Vec<String>>,
    category: Option<String>,
) -> Result<MemoryEntryDto, String> {
    let mut store = load_store()?;
    let entry = store
        .get_mut(&id)
        .ok_or_else(|| format!("memory {id} not found"))?;
    if let Some(c) = content {
        entry.content = c;
    }
    if let Some(t) = tags {
        entry.tags = t;
    }
    if let Some(c) = category {
        entry.category = parse_category(&c);
    }
    let updated = entry.clone();
    store.save().map_err(|e| e.to_string())?;
    Ok(updated)
}

/// Delete a memory by ID. Returns `true` if the entry existed.
#[tauri::command]
pub async fn delete_memory(id: String) -> Result<bool, String> {
    let mut store = load_store()?;
    let removed = store.delete(&id).map_err(|e| e.to_string())?;
    if removed {
        store.save().map_err(|e| e.to_string())?;
    }
    Ok(removed)
}

/// Search memories by free-text query (matches content + tags). Results are
/// sorted by relevance (confidence + recency), most relevant first.
#[tauri::command]
pub async fn search_memories(
    query: String,
    project: Option<String>,
) -> Result<Vec<MemoryEntryDto>, String> {
    let store = load_store()?;
    Ok(store.search(&query, project.as_deref()))
}

/// Aggregate counts per category and per project. Used by the UI to render
/// a memory dashboard without pulling every entry.
#[derive(Debug, serde::Serialize)]
pub struct MemoryStats {
    pub total: usize,
    pub by_category: std::collections::HashMap<String, usize>,
    pub by_project: std::collections::HashMap<String, usize>,
    pub most_recent_at: Option<DateTime<Utc>>,
}

#[tauri::command]
pub async fn get_memory_stats() -> Result<MemoryStats, String> {
    let store = load_store()?;
    let mut by_category = std::collections::HashMap::new();
    let mut by_project = std::collections::HashMap::new();
    let mut most_recent: Option<DateTime<Utc>> = None;
    for entry in all_entries(&store) {
        *by_category.entry(entry.category.to_string()).or_default() += 1;
        *by_project.entry(entry.project.clone()).or_default() += 1;
        most_recent = Some(most_recent.map_or(entry.created_at, |prev| {
            if entry.created_at > prev {
                entry.created_at
            } else {
                prev
            }
        }));
    }
    Ok(MemoryStats {
        total: by_category.values().sum(),
        by_category,
        by_project,
        most_recent_at: most_recent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_category_known_strings() {
        assert_eq!(parse_category("preference"), MemoryCategory::Preference);
        assert_eq!(parse_category("PATTERN"), MemoryCategory::Pattern);
        assert_eq!(parse_category("Decision"), MemoryCategory::Decision);
        assert_eq!(parse_category("error"), MemoryCategory::Error);
        assert_eq!(parse_category("context"), MemoryCategory::Context);
    }

    #[test]
    fn parse_category_unknown_falls_back_to_context() {
        assert_eq!(parse_category("unknown"), MemoryCategory::Context);
        assert_eq!(parse_category(""), MemoryCategory::Context);
    }

    #[test]
    fn storage_path_is_under_shannon_home() {
        let p = storage_path().unwrap();
        let s = p.to_string_lossy();
        assert!(s.ends_with(".shannon/memories"), "got {s}");
    }
}
