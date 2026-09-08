//! Tauri commands for the persistent memory layer (P2.1) + P2-4b wiring.
//!
//! The store lives at `~/.shannon/memories/{project_hash}.jsonl` (the same
//! path and load conventions the CLI uses). Since P2-4b, [`AppState`] holds
//! **one shared** [`MemoryStore`] handle (`Arc<RwLock<..>>`); every engine the
//! desktop constructs (interactive send, background task, slash diagnostics,
//! and the goal/batch/inbox unattended runners) attaches that same handle via
//! `attach_shared_memory`, so memory injection and auto-extraction converge
//! on a single in-process instance.
//!
//! The commands below operate on that shared instance (lock per command, no
//! await while held). Read paths refresh from disk first (best-effort) so
//! entries written by other processes — the migration import, the CLI — show
//! up without a restart. Provenance (P2-4): hand-created entries are stamped
//! `source_kind = "manual"`; migration stamps `"import"`; engine extraction
//! stamps `"auto-extract"` + the producing session id.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use shannon_core::memory::{MemoryCategory, MemoryEntry, MemoryStore};
use shannon_core::query_engine::QueryEngine;

use crate::commands::AppState;

/// Cap on entry nodes in one graph payload (P2-4: >200 → truncate to the most
/// recent 200 and let the UI prompt for filters).
pub const MEMORY_GRAPH_MAX_ENTRIES: usize = 200;

/// Edge kind: hierarchy edge (project root → category → its entries).
/// Expresses grouping only — never semantic relatedness.
pub const EDGE_CLUSTER: &str = "cluster";
/// Edge kind: weak association between entries captured in the same source
/// session. Chained (consecutive pairs by creation time), never a clique.
pub const EDGE_SESSION: &str = "session";

/// The process-wide memory store handle every engine shares (P2-4b).
pub(crate) type SharedMemoryStore = Arc<RwLock<MemoryStore>>;

/// Resolve the on-disk memories directory (`~/.shannon/memories/`) — the same
/// location the CLI uses.
fn storage_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "could not resolve $HOME".to_string())?;
    Ok(home.join(".shannon").join("memories"))
}

/// Construct the shared store: default path + initial disk load. Load errors
/// are logged, never fatal — the store still works in-memory (P2-4b).
pub(crate) fn open_shared_store() -> SharedMemoryStore {
    open_shared_store_at(storage_path().unwrap_or_else(|_| PathBuf::from(".shannon/memories")))
}

/// [`open_shared_store`] over an explicit path (tests, temp homes).
pub(crate) fn open_shared_store_at(path: PathBuf) -> SharedMemoryStore {
    let mut store = MemoryStore::new(path);
    if let Err(e) = store.load() {
        tracing::debug!(error = %e, "shared memory store initial load failed");
    }
    Arc::new(RwLock::new(store))
}

/// Best-effort reload from disk so entries written by other processes
/// (migration import, CLI) become visible in-process. Deletions made by other
/// processes are intentionally not tracked here (see module docs).
pub(crate) fn refresh_shared_store(store: &SharedMemoryStore) {
    if let Ok(mut guard) = store.write() {
        if let Err(e) = guard.load() {
            tracing::debug!(error = %e, "shared memory store reload failed");
        }
    }
}

/// Attach the shared store handle to an engine (P2-4b).
///
/// Refreshes from disk first (cheap: a few small JSONL files) so memory
/// written since the app started is visible to this engine's injection path,
/// then threads a clone of the shared handle into the engine. All six desktop
/// engine construction sites call this — pass the handle, never rebuild.
pub(crate) fn attach_shared_memory(engine: QueryEngine, store: &SharedMemoryStore) -> QueryEngine {
    refresh_shared_store(store);
    engine.with_memory_arc(store.clone())
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
/// unchanged (includes the optional P2-4 provenance fields).
pub type MemoryEntryDto = MemoryEntry;

/// All entries currently in the store, unsorted. Uses `search("", None)` which
/// matches every entry (empty query is a substring of every content string).
fn all_entries(store: &MemoryStore) -> Vec<MemoryEntry> {
    store.search("", None)
}

/// Create a [`MemoryEntry`] for the hand-written path, stamped with
/// `source_kind = "manual"` (P2-4 provenance).
fn manual_entry(
    project: &str,
    category: MemoryCategory,
    content: &str,
    tags: Option<Vec<String>>,
    confidence: Option<f64>,
) -> Result<MemoryEntry, String> {
    let mut entry = MemoryEntry::new(project, category, content);
    if let Some(t) = tags {
        entry.tags = t;
    }
    if let Some(c) = confidence {
        if !(0.0..=1.0).contains(&c) {
            return Err(format!("confidence must be in [0.0, 1.0], got {c}"));
        }
        entry.confidence = c;
    }
    entry.source_kind = Some(MemoryEntry::SOURCE_MANUAL.to_string());
    Ok(entry)
}

/// The stored source session of a memory entry, if any — the lookup behind
/// the frozen `get_memory_source` contract.
fn source_session_of(store: &MemoryStore, memory_id: &str) -> Option<String> {
    store
        .get(memory_id)
        .and_then(|e| e.source_session_id.clone())
}

/// List distinct project names that have at least one memory entry.
#[tauri::command]
pub async fn list_memory_projects(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let store = &state.memory_store;
    refresh_shared_store(store);
    let guard = store.read().map_err(|e| e.to_string())?;
    let mut projects: Vec<String> = all_entries(&guard)
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
    state: tauri::State<'_, AppState>,
    project: Option<String>,
    category: Option<String>,
    query: Option<String>,
) -> Result<Vec<MemoryEntryDto>, String> {
    let store = &state.memory_store;
    refresh_shared_store(store);
    let guard = store.read().map_err(|e| e.to_string())?;
    let mut rows = if query.as_deref().map(str::is_empty).unwrap_or(true) {
        all_entries(&guard)
    } else {
        guard.search(query.as_deref().unwrap_or(""), project.as_deref())
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
/// Provenance: stamped `source_kind = "manual"` (P2-4).
#[tauri::command]
pub async fn create_memory(
    state: tauri::State<'_, AppState>,
    project: String,
    category: String,
    content: String,
    tags: Option<Vec<String>>,
    confidence: Option<f64>,
) -> Result<MemoryEntryDto, String> {
    let entry = manual_entry(
        &project,
        parse_category(&category),
        &content,
        tags,
        confidence,
    )?;
    let store = &state.memory_store;
    let mut guard = store.write().map_err(|e| e.to_string())?;
    guard.add(entry.clone()).map_err(|e| e.to_string())?;
    guard.save().map_err(|e| e.to_string())?;
    Ok(entry)
}

/// Update an existing memory entry's mutable fields (content, tags, category).
///
/// Only fields supplied as `Some(...)` are updated; `None` leaves the existing
/// value intact. Returns the updated entry or an error if the ID is unknown.
#[tauri::command]
pub async fn update_memory(
    state: tauri::State<'_, AppState>,
    id: String,
    content: Option<String>,
    tags: Option<Vec<String>>,
    category: Option<String>,
) -> Result<MemoryEntryDto, String> {
    let store = &state.memory_store;
    let mut guard = store.write().map_err(|e| e.to_string())?;
    {
        let entry = guard
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
        guard.save().map_err(|e| e.to_string())?;
        Ok(updated)
    }
}

/// Delete a memory by ID. Returns `true` if the entry existed.
#[tauri::command]
pub async fn delete_memory(state: tauri::State<'_, AppState>, id: String) -> Result<bool, String> {
    let store = &state.memory_store;
    let mut guard = store.write().map_err(|e| e.to_string())?;
    let removed = guard.delete(&id).map_err(|e| e.to_string())?;
    if removed {
        guard.save().map_err(|e| e.to_string())?;
    }
    Ok(removed)
}

/// Search memories by free-text query (matches content + tags). Results are
/// sorted by relevance (confidence + recency), most relevant first.
#[tauri::command]
pub async fn search_memories(
    state: tauri::State<'_, AppState>,
    query: String,
    project: Option<String>,
) -> Result<Vec<MemoryEntryDto>, String> {
    let store = &state.memory_store;
    refresh_shared_store(store);
    let guard = store.read().map_err(|e| e.to_string())?;
    Ok(guard.search(&query, project.as_deref()))
}

/// Frozen contract (P2-4): resolve the source session of a memory entry.
///
/// Simple passthrough — returns the entry's stored `source_session_id` wrapped
/// in `{ sessionId }`, or `null` when the memory is unknown or carries no
/// source session (manual entries / legacy imports). The `sessionId` argument
/// identifies the calling session per the frozen shape; all desktop sessions
/// are local to the user, so it does not gate the lookup.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySourceDto {
    pub session_id: String,
}

#[tauri::command]
pub async fn get_memory_source(
    state: tauri::State<'_, AppState>,
    session_id: String,
    memory_id: String,
) -> Result<Option<MemorySourceDto>, String> {
    let _ = session_id; // frozen contract shape; see doc comment
    let store = &state.memory_store;
    refresh_shared_store(store);
    let guard = store.read().map_err(|e| e.to_string())?;
    Ok(source_session_of(&guard, &memory_id).map(|sid| MemorySourceDto { session_id: sid }))
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
pub async fn get_memory_stats(state: tauri::State<'_, AppState>) -> Result<MemoryStats, String> {
    let store = &state.memory_store;
    refresh_shared_store(store);
    let guard = store.read().map_err(|e| e.to_string())?;
    let mut by_category = std::collections::HashMap::new();
    let mut by_project = std::collections::HashMap::new();
    let mut most_recent: Option<DateTime<Utc>> = None;
    for entry in all_entries(&guard) {
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

// ─── Memory graph (P2-4) ────────────────────────────────────────────────────

/// One node in the memory graph: the project root, a category cluster, or an
/// individual entry.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryGraphNode {
    /// Stable id: `project:<path>`, `category:<project>|<category>`, or
    /// `entry:<id>`.
    pub id: String,
    /// `"project"` | `"category"` | `"entry"`.
    pub kind: String,
    /// Display label.
    pub label: String,
    /// Category string for `category`/`entry` nodes (semantic color mapping).
    pub category: Option<String>,
    /// Visual weight: entry count for project/category nodes, confidence for
    /// entries. Drives node size in the UI.
    pub weight: f64,
    /// Tags of the entry (empty for project/category nodes) — shown in the
    /// detail popover.
    pub tags: Vec<String>,
    /// Provenance passthrough for entry nodes.
    pub source_kind: Option<String>,
    /// Provenance passthrough for entry nodes.
    pub source_session_id: Option<String>,
}

/// One edge in the memory graph. Conservative by design — see the module docs
/// and the report: `cluster` is pure containment, `session` a chained weak
/// association between entries captured in the same source session.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryGraphEdge {
    pub source: String,
    pub target: String,
    /// `"cluster"` | `"session"`.
    pub kind: String,
}

/// Aggregated graph payload: project root → category clusters → entry nodes.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryGraph {
    /// The project filter the graph was built for, when provided.
    pub project: Option<String>,
    pub nodes: Vec<MemoryGraphNode>,
    pub edges: Vec<MemoryGraphEdge>,
    /// Entries in scope before truncation.
    pub entry_count: usize,
    pub max_entries: usize,
    /// True when more than [`MEMORY_GRAPH_MAX_ENTRIES`] entries were in scope
    /// and only the most recent 200 are included (UI prompts for filters).
    pub truncated: bool,
}

/// Aggregate entries into the graph structure (pure — unit-tested).
///
/// Grouping: one root node per project in scope, one category node per
/// (project, category) actually present, one node per entry. Edges:
/// root → category and category → entry as `cluster`; entries sharing the
/// same non-empty `source_session_id` are chained by `session` edges in
/// creation order (n−1 edges per group, never a clique — "better too few
/// than too many"). Over [`MEMORY_GRAPH_MAX_ENTRIES`] entries, only the most
/// recent [`MEMORY_GRAPH_MAX_ENTRIES`] are kept and `truncated` is set.
pub(crate) fn build_memory_graph(project: Option<&str>, entries: &[MemoryEntry]) -> MemoryGraph {
    let mut scoped: Vec<&MemoryEntry> = entries
        .iter()
        .filter(|e| project.is_none_or(|p| e.project == p))
        .collect();
    let entry_count = scoped.len();
    let truncated = entry_count > MEMORY_GRAPH_MAX_ENTRIES;
    if truncated {
        scoped.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        scoped.truncate(MEMORY_GRAPH_MAX_ENTRIES);
    }

    let empty = |project: Option<&str>| MemoryGraph {
        project: project.map(str::to_string),
        nodes: Vec::new(),
        edges: Vec::new(),
        entry_count,
        max_entries: MEMORY_GRAPH_MAX_ENTRIES,
        truncated,
    };
    if scoped.is_empty() {
        return empty(project);
    }

    // Deterministic order: project, then category, then newest entry first.
    scoped.sort_by(|a, b| {
        a.project
            .cmp(&b.project)
            .then_with(|| a.category.to_string().cmp(&b.category.to_string()))
            .then_with(|| b.created_at.cmp(&a.created_at))
    });

    let mut nodes: Vec<MemoryGraphNode> = Vec::new();
    let mut edges: Vec<MemoryGraphEdge> = Vec::new();
    let mut seen_roots: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut per_project_entries: std::collections::HashMap<String, Vec<&MemoryEntry>> =
        std::collections::HashMap::new();

    // Category clusters.
    let mut by_project_category: std::collections::BTreeMap<(String, String), Vec<&MemoryEntry>> =
        std::collections::BTreeMap::new();
    for e in &scoped {
        by_project_category
            .entry((e.project.clone(), e.category.to_string()))
            .or_default()
            .push(e);
    }

    for ((project, category), members) in &by_project_category {
        let root_id = format!("project:{project}");
        if seen_roots.insert(root_id.clone()) {
            let count = scoped.iter().filter(|e| e.project == *project).count();
            nodes.push(MemoryGraphNode {
                id: root_id.clone(),
                kind: "project".into(),
                label: project.clone(),
                category: None,
                weight: count as f64,
                tags: Vec::new(),
                source_kind: None,
                source_session_id: None,
            });
        }
        let category_id = format!("category:{project}|{category}");
        nodes.push(MemoryGraphNode {
            id: category_id.clone(),
            kind: "category".into(),
            label: category.clone(),
            category: Some(category.clone()),
            weight: members.len() as f64,
            tags: Vec::new(),
            source_kind: None,
            source_session_id: None,
        });
        edges.push(MemoryGraphEdge {
            source: root_id,
            target: category_id.clone(),
            kind: EDGE_CLUSTER.into(),
        });

        for e in members {
            let entry_id = format!("entry:{}", e.id);
            nodes.push(MemoryGraphNode {
                id: entry_id.clone(),
                kind: "entry".into(),
                label: e.content.clone(),
                category: Some(category.clone()),
                weight: e.confidence,
                tags: e.tags.clone(),
                source_kind: e.source_kind.clone(),
                source_session_id: e.source_session_id.clone(),
            });
            edges.push(MemoryGraphEdge {
                source: category_id.clone(),
                target: entry_id,
                kind: EDGE_CLUSTER.into(),
            });
            per_project_entries
                .entry(project.clone())
                .or_default()
                .push(e);
        }
    }

    // Weak same-source-session association edges, chained per project in
    // creation order (oldest → newest). Entries without a session id never
    // participate.
    for members in per_project_entries.values() {
        let mut by_session: std::collections::HashMap<&String, Vec<&&MemoryEntry>> =
            std::collections::HashMap::new();
        for e in members {
            if let Some(sid) = &e.source_session_id {
                if !sid.is_empty() {
                    by_session.entry(sid).or_default().push(e);
                }
            }
        }
        for (_, group) in by_session {
            let mut ordered: Vec<&&MemoryEntry> = group;
            ordered.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            for pair in ordered.windows(2) {
                edges.push(MemoryGraphEdge {
                    source: format!("entry:{}", pair[0].id),
                    target: format!("entry:{}", pair[1].id),
                    kind: EDGE_SESSION.into(),
                });
            }
        }
    }

    MemoryGraph {
        project: project.map(str::to_string),
        nodes,
        edges,
        entry_count,
        max_entries: MEMORY_GRAPH_MAX_ENTRIES,
        truncated,
    }
}

/// Graph payload for the Memory page's graph view (P2-4).
#[tauri::command]
pub async fn get_memory_graph(
    state: tauri::State<'_, AppState>,
    project: Option<String>,
) -> Result<MemoryGraph, String> {
    let store = &state.memory_store;
    refresh_shared_store(store);
    let guard = store.read().map_err(|e| e.to_string())?;
    let entries = all_entries(&guard);
    Ok(build_memory_graph(project.as_deref(), &entries))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn entry(project: &str, category: MemoryCategory, content: &str) -> MemoryEntry {
        MemoryEntry::new(project, category, content)
    }

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

    #[test]
    fn manual_entry_is_stamped_manual() {
        let e = manual_entry("proj", MemoryCategory::Decision, "use rust", None, None).unwrap();
        assert_eq!(e.source_kind.as_deref(), Some(MemoryEntry::SOURCE_MANUAL));
        assert!(
            e.source_session_id.is_none(),
            "manual entries carry no session"
        );
    }

    #[test]
    fn manual_entry_rejects_out_of_range_confidence() {
        assert!(manual_entry("p", MemoryCategory::Error, "x", None, Some(1.5)).is_err());
        assert!(manual_entry("p", MemoryCategory::Error, "x", None, Some(-0.1)).is_err());
    }

    #[test]
    fn attach_shared_memory_attaches_one_shared_handle() {
        use shannon_core::query_engine::QueryEngine;
        use shannon_engine::api::client::LlmClient;
        use shannon_engine::api::types::LlmClientConfig;
        use shannon_engine::permissions::PermissionManager;
        use shannon_engine::state::StateManager;

        let dir = tempfile::TempDir::new().unwrap();
        let shared = open_shared_store_at(dir.path().to_path_buf());

        // Same construction shape as the six desktop engine sites.
        let build = || {
            attach_shared_memory(
                QueryEngine::with_defaults_arc(
                    LlmClient::new(LlmClientConfig::default()),
                    std::sync::Arc::new(shannon_core::tools::ToolRegistry::new()),
                    PermissionManager::new(),
                    StateManager::new(),
                ),
                &shared,
            )
        };
        let engine_a = build();
        let engine_b = build();

        let handle_a = engine_a.memory().cloned().expect("a attached");
        assert!(
            std::sync::Arc::ptr_eq(&handle_a, engine_b.memory().expect("b attached")),
            "both engines must share one store instance"
        );

        // Seeding through the shared handle (simulating the Memory page's
        // writes) is visible on the engine's injection read path.
        let project = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "default".to_string());
        {
            let mut store = shared.write().unwrap();
            store
                .add(MemoryEntry::new(
                    &project,
                    MemoryCategory::Context,
                    "desktop injects this",
                ))
                .unwrap();
        }
        let injected = engine_b
            .memory()
            .unwrap()
            .read()
            .unwrap()
            .format_for_injection(&project)
            .expect("injection text");
        assert!(injected.contains("desktop injects this"));
    }

    #[test]
    fn refresh_shared_store_picks_up_other_process_appends() {
        let dir = tempfile::TempDir::new().unwrap();
        let shared = open_shared_store_at(dir.path().to_path_buf());
        let project = "migration-project";

        // Before the "other process" writes, the store knows nothing.
        assert!(shared.read().unwrap().project_memories(project).is_empty());

        // Another process (migration import / CLI) appends via its own store.
        let mut external = MemoryStore::new(dir.path().to_path_buf());
        external
            .add(MemoryEntry::new(
                project,
                MemoryCategory::Context,
                "imported fact",
            ))
            .unwrap();

        refresh_shared_store(&shared);
        let memories = shared.read().unwrap().project_memories(project);
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].content, "imported fact");
    }

    #[test]
    fn source_session_of_returns_stored_session() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let mut e = entry("proj", MemoryCategory::Pattern, "sourced");
        e.source_session_id = Some("sess-1".to_string());
        let id = e.id.clone();
        store.add(e).unwrap();
        let bare = entry("proj", MemoryCategory::Pattern, "unsourced");
        let bare_id = bare.id.clone();
        store.add(bare).unwrap();

        assert_eq!(source_session_of(&store, &id).as_deref(), Some("sess-1"));
        assert_eq!(source_session_of(&store, &bare_id), None);
        assert_eq!(source_session_of(&store, "missing"), None);
    }

    // --- build_memory_graph ---

    fn sourced(
        project: &str,
        category: MemoryCategory,
        content: &str,
        session: Option<&str>,
    ) -> MemoryEntry {
        let mut e = entry(project, category, content);
        e.source_session_id = session.map(str::to_string);
        e
    }

    #[test]
    fn graph_empty_scope_has_no_nodes() {
        let g = build_memory_graph(Some("proj"), &[]);
        assert!(g.nodes.is_empty());
        assert!(g.edges.is_empty());
        assert_eq!(g.entry_count, 0);
        assert!(!g.truncated);
        assert_eq!(g.project.as_deref(), Some("proj"));
    }

    #[test]
    fn graph_groups_project_category_entry_with_cluster_edges() {
        let entries = vec![
            sourced("p", MemoryCategory::Preference, "tabs", None),
            sourced("p", MemoryCategory::Preference, "dark mode", None),
            sourced("p", MemoryCategory::Decision, "rust", None),
            sourced("other", MemoryCategory::Context, "unrelated", None),
        ];
        let g = build_memory_graph(Some("p"), &entries);
        let kinds: Vec<&str> = g.nodes.iter().map(|n| n.kind.as_str()).collect();
        assert_eq!(kinds.iter().filter(|k| **k == "project").count(), 1);
        assert_eq!(kinds.iter().filter(|k| **k == "category").count(), 2);
        assert_eq!(kinds.iter().filter(|k| **k == "entry").count(), 3);
        assert_eq!(g.entry_count, 3, "only the scoped project's entries");
        // Cluster edges: 1 root→category per cluster + 1 category→entry per entry.
        assert_eq!(g.edges.iter().filter(|e| e.kind == EDGE_CLUSTER).count(), 5);
        assert!(g.edges.iter().all(|e| e.kind == EDGE_CLUSTER));
    }

    #[test]
    fn graph_without_project_filter_makes_one_root_per_project() {
        let entries = vec![
            sourced("p", MemoryCategory::Preference, "tabs", None),
            sourced("q", MemoryCategory::Decision, "rust", None),
        ];
        let g = build_memory_graph(None, &entries);
        let roots: Vec<&MemoryGraphNode> = g.nodes.iter().filter(|n| n.kind == "project").collect();
        assert_eq!(roots.len(), 2);
        // Categories of the same name in different projects stay distinct.
        let cats: Vec<&MemoryGraphNode> = g.nodes.iter().filter(|n| n.kind == "category").collect();
        assert_eq!(cats.len(), 2);
    }

    #[test]
    fn graph_session_edges_chain_entries_sharing_a_session() {
        let entries = vec![
            sourced("p", MemoryCategory::Pattern, "a", Some("s1")),
            sourced("p", MemoryCategory::Decision, "b", Some("s1")),
            sourced("p", MemoryCategory::Error, "c", Some("s1")),
            sourced("p", MemoryCategory::Context, "d", Some("s2")),
            sourced("p", MemoryCategory::Context, "e", None),
        ];
        let g = build_memory_graph(Some("p"), &entries);
        let session_edges: Vec<&MemoryGraphEdge> =
            g.edges.iter().filter(|e| e.kind == EDGE_SESSION).collect();
        // s1 chains a→b→c (2 edges); s2 is alone; unsessioned entry excluded.
        assert_eq!(session_edges.len(), 2, "chained, never a clique");
        for e in session_edges {
            assert_ne!(e.source, e.target);
        }
    }

    #[test]
    fn graph_truncates_to_cap_and_flags() {
        let entries: Vec<MemoryEntry> = (0..(MEMORY_GRAPH_MAX_ENTRIES + 50))
            .map(|i| {
                let mut e = entry("p", MemoryCategory::Context, &format!("e{i}"));
                // Ascending created_at so truncation keeps the newest ones.
                e.created_at = Utc::now() - chrono::Duration::seconds(1000 - i as i64);
                e
            })
            .collect();
        let g = build_memory_graph(Some("p"), &entries);
        assert!(g.truncated);
        assert_eq!(g.entry_count, MEMORY_GRAPH_MAX_ENTRIES + 50);
        let entry_nodes = g.nodes.iter().filter(|n| n.kind == "entry").count();
        assert_eq!(entry_nodes, MEMORY_GRAPH_MAX_ENTRIES);
        // The newest entries survive: the highest-index (newest) is present.
        assert!(
            g.nodes
                .iter()
                .any(|n| n.id == format!("entry:{}", entries.last().unwrap().id))
        );
    }

    #[test]
    fn graph_node_ids_are_stable_and_prefixed() {
        let mut tagged = sourced("p", MemoryCategory::Pattern, "x", Some("s"));
        tagged.tags = vec!["alpha".to_string()];
        let entries = vec![tagged];
        let g = build_memory_graph(Some("p"), &entries);
        let ids: Vec<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"project:p"));
        assert!(ids.contains(&"category:p|pattern"));
        assert!(ids[2].starts_with("entry:"));
        // Entry node passes provenance + tags through for the detail popover.
        let entry_node = g.nodes.iter().find(|n| n.kind == "entry").unwrap();
        assert_eq!(entry_node.source_session_id.as_deref(), Some("s"));
        assert_eq!(entry_node.tags, vec!["alpha".to_string()]);
        // Cluster nodes carry no tags.
        let root = g.nodes.iter().find(|n| n.kind == "project").unwrap();
        assert!(root.tags.is_empty());
    }

    #[test]
    fn graph_is_deterministic() {
        let entries = vec![
            sourced("p", MemoryCategory::Preference, "a", Some("s1")),
            sourced("p", MemoryCategory::Decision, "b", Some("s1")),
            sourced("q", MemoryCategory::Context, "c", None),
        ];
        let g1 = build_memory_graph(None, &entries);
        let g2 = build_memory_graph(None, &entries);
        assert_eq!(g1.nodes, g2.nodes);
        assert_eq!(g1.edges.len(), g2.edges.len());
    }
}
