//! Tauri commands for the project registry (P-E3, 阶段三·项目实体化).
//!
//! "Project" becomes a first-class engine entity: one curated row per
//! working-directory path, stored by
//! [`shannon_core::project_registry::ProjectRegistry`] in
//! `~/.shannon/projects.db` (its own database — never shared with the inbox
//! store). The registry is **adopted, not migrated**: [`list_projects`]
//! back-fills it from the places project paths already live before every
//! read —
//!
//! 1. the session store's `project_path` values (deduplicated),
//! 2. routine sidecar working dirs (empty until Task 2 delivers
//!    `ScheduledTaskStore::working_dirs()` — see the `routine_working_dirs`
//!    seam below),
//! 3. on first seed (empty table) the memory layer's distinct project
//!    labels (reusing [`crate::commands_memory`]'s enumeration).
//!
//! Adoption is strictly additive (`ensure_adopted`): existing rows —
//! including archived ones and curated names — are never overwritten.
//! [`set_session_working_dir`](crate::commands_sessions::set_session_working_dir)
//! additionally adopts its new directory incrementally via the
//! `adopt_working_dir` hook.

use std::collections::HashSet;

use shannon_core::project_registry::{ProjectAdoptCandidate, ProjectRecord, ProjectRegistry};

use crate::commands::AppState;

impl AppState {
    /// Borrow the shared project registry (P-E3), opening it on first use.
    ///
    /// Falls back to an in-memory database when the on-disk open fails
    /// (permissions, corrupt file) so project features degrade gracefully
    /// instead of failing on every command — the same contract as the
    /// inbox store accessor.
    pub(crate) fn project_registry(
        &self,
    ) -> std::sync::Arc<shannon_core::project_registry::ProjectRegistry> {
        self.project_registry
            .get_or_init(
                || match ProjectRegistry::open_default() {
                    Ok(store) => std::sync::Arc::new(store),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "project registry: on-disk open failed, using in-memory fallback"
                        );
                        std::sync::Arc::new(
                            ProjectRegistry::open_in_memory()
                                .expect("in-memory SQLite must always open"),
                        )
                    }
                },
            )
            .clone()
    }
}

// ─── commands ───────────────────────────────────────────────────────────────

/// List registered projects (path-ascending). Archived rows are included
/// only with `include_archived = true`.
///
/// Before reading, the registry is back-filled from the adoption sources
/// (adopt-not-migrate) so a fresh install already shows the projects its
/// sessions and memories imply.
#[tauri::command]
pub async fn list_projects(
    state: tauri::State<'_, AppState>,
    include_archived: Option<bool>,
) -> Result<Vec<ProjectRecord>, String> {
    let registry = state.project_registry();
    let first_seed = registry
        .list(true)
        .map_err(|e| e.to_string())?
        .is_empty();
    let candidates = adoption_candidates(state.inner(), first_seed);
    if !candidates.is_empty() {
        if let Err(e) = registry.ensure_adopted(&candidates) {
            // Best-effort: the read still serves whatever is registered.
            tracing::warn!(error = %e, "project adoption failed; serving current registry");
        }
    }
    registry
        .list(include_archived.unwrap_or(false))
        .map_err(|e| e.to_string())
}

/// Register a project path. Idempotent adoption: a path that is already
/// registered (under any name, archived or not) is returned unchanged —
/// registration never overwrites an existing row. The name hint is the
/// path's tail segment.
#[tauri::command]
pub async fn register_project(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<ProjectRecord, String> {
    let path = normalize_path(&path).to_string();
    if path.is_empty() {
        return Err("project path is empty".into());
    }
    let registry = state.project_registry();
    let candidate = ProjectAdoptCandidate {
        path: path.clone(),
        name_hint: basename_hint(&path),
    };
    registry
        .ensure_adopted(std::slice::from_ref(&candidate))
        .map_err(|e| e.to_string())?;
    registry
        .get(&path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not registered after adoption: {path}"))
}

/// Set a project's custom display name (`None` clears it, falling back to
/// the path's tail segment in the UI). Unknown paths are auto-created.
#[tauri::command]
pub async fn rename_project(
    state: tauri::State<'_, AppState>,
    path: String,
    name: Option<String>,
) -> Result<ProjectRecord, String> {
    state
        .project_registry()
        .rename(normalize_path(&path), name)
        .map_err(|e| e.to_string())
}

/// Set a project's custom icon and color (`None` clears a field). Unknown
/// paths are auto-created.
#[tauri::command]
pub async fn set_project_appearance(
    state: tauri::State<'_, AppState>,
    path: String,
    icon: Option<String>,
    color: Option<String>,
) -> Result<ProjectRecord, String> {
    state
        .project_registry()
        .set_appearance(normalize_path(&path), icon, color)
        .map_err(|e| e.to_string())
}

/// Archive a project (stamps `archived_at_ms`). Unknown paths are
/// auto-created.
#[tauri::command]
pub async fn archive_project(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<ProjectRecord, String> {
    state
        .project_registry()
        .set_archived(normalize_path(&path), true)
        .map_err(|e| e.to_string())
}

/// Unarchive a project (clears `archived_at_ms`). Unknown paths are
/// auto-created.
#[tauri::command]
pub async fn unarchive_project(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<ProjectRecord, String> {
    state
        .project_registry()
        .set_archived(normalize_path(&path), false)
        .map_err(|e| e.to_string())
}

// ─── adoption plumbing ──────────────────────────────────────────────────────

/// Incremental adoption hook for `set_session_working_dir`: a session just
/// gained a working directory, so the registry learns it. Best-effort and
/// idempotent — never overwrites an existing row.
pub(crate) fn adopt_working_dir(state: &AppState, dir: &str) {
    let dir = normalize_path(dir);
    if dir.is_empty() {
        return;
    }
    let registry = state.project_registry();
    let candidate = ProjectAdoptCandidate {
        path: dir.to_string(),
        name_hint: basename_hint(dir),
    };
    if let Err(e) = registry.ensure_adopted(std::slice::from_ref(&candidate)) {
        tracing::warn!(error = %e, "project adoption: incremental adopt failed");
    }
}

/// Collect adoption candidates (adopt-not-migrate):
///
/// (a) every distinct non-empty `project_path` in the session store listing;
/// (b) routine sidecar working dirs ([`routine_working_dirs`], empty until
///     Task 2);
/// (c) when `first_seed` (the table is empty), the memory layer's distinct
///     project labels — reusing [`crate::commands_memory`]'s enumeration so
///     memory pages and the registry agree on what a project is.
///
/// The name hint is always the path's tail segment.
fn adoption_candidates(state: &AppState, first_seed: bool) -> Vec<ProjectAdoptCandidate> {
    let mut candidates = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // (a) session store project_path values.
    match state.l0_store().list() {
        Ok(infos) => {
            for path in infos.into_iter().filter_map(|info| info.project_path) {
                push_candidate(&mut candidates, &mut seen, &path);
            }
        }
        Err(e) => {
            tracing::debug!(error = %e, "project adoption: session listing failed");
        }
    }

    // (b) routine working dirs (Task 2 seam — currently an empty set).
    for dir in routine_working_dirs(state) {
        push_candidate(&mut candidates, &mut seen, &dir);
    }

    // (c) memory project labels, first seed only.
    if first_seed {
        match crate::commands_memory::memory_project_labels(&state.memory_store) {
            Ok(labels) => {
                for label in labels {
                    push_candidate(&mut candidates, &mut seen, &label);
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "project adoption: memory label listing failed");
            }
        }
    }

    candidates
}

/// Trim + dedupe + hint in one place; blank paths are never registered.
fn push_candidate(
    candidates: &mut Vec<ProjectAdoptCandidate>,
    seen: &mut HashSet<String>,
    path: &str,
) {
    let path = normalize_path(path);
    if path.is_empty() || !seen.insert(path.to_string()) {
        return;
    }
    candidates.push(ProjectAdoptCandidate {
        path: path.to_string(),
        name_hint: basename_hint(path),
    });
}

/// Registry-key normalization: trim whitespace and trailing separators so
/// `/x` and `/x/` land on one project row (the UI groups sessions by the
/// de-slashed working-dir key, so the registry must agree). Every command
/// entry point runs its `path` argument through this before touching the
/// store — the core registry stays mechanical (raw keys, auto-create) and
/// would otherwise grow a phantom duplicate row for a separator variant.
///
/// `pub(crate)` because every working-dir writer shares it: routine sidecar
/// persistence (P-E1) stores the same normalized key the registry adopts.
pub(crate) fn normalize_path(path: &str) -> &str {
    path.trim().trim_end_matches(['/', '\\'])
}

/// Routine working dirs feeding project adoption (collector source b).
///
/// `ScheduledRoutine` carries no `working_dir` until Task 2 (P-E1/P-E2)
/// adds the sidecar + `ScheduledTaskStore::working_dirs()`. The collector
/// is wired through this seam so Task 2 only replaces this body.
// Task 2: replace with `state.scheduled_task_store().working_dirs()`.
fn routine_working_dirs(_state: &AppState) -> Vec<String> {
    Vec::new()
}

/// Name hint for an adopted project: the path's tail segment (basename on
/// either separator). `None` when the path reduces to nothing (`/`,
/// whitespace).
fn basename_hint(path: &str) -> Option<String> {
    let base = normalize_path(path)
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("");
    if base.is_empty() {
        None
    } else {
        Some(base.to_string())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::commands::AppState;
    use shannon_engine::state::StateManager;
    use tauri::Manager;

    fn project_state(dir: &std::path::Path) -> AppState {
        let mut state = AppState::new();
        // Hermetic stores: sessions + memory + registry all under `dir`.
        // `AppState::new()` only *reads* ambient config — nothing writes
        // outside `dir` (same fixture contract as the budget tests).
        state.state_manager = std::sync::Arc::new(
            StateManager::with_sessions_dir(dir.join("sessions")).expect("temp sessions dir"),
        );
        state.memory_store = crate::commands_memory::open_shared_store_at(dir.join("memories"));
        assert!(
            state
                .project_registry
                .set(std::sync::Arc::new(
                    ProjectRegistry::with_path(&dir.join("projects.db")).unwrap(),
                ))
                .is_ok(),
            "fresh AppState has an unset registry OnceLock"
        );
        state
    }

    fn seed_session_with_cwd(state: &AppState, id: &uuid::Uuid, cwd: Option<&str>) {
        use shannon_types::session_event::{SessionEventBody, SessionStartPayload};
        let mut w = shannon_core::session_log::SessionLogWriter::open_layout(
            state.l0_store().container(),
            &id.to_string(),
        )
        .expect("open log");
        w.record(SessionEventBody::SessionStart(SessionStartPayload {
            model: "test-model".into(),
            provider: None,
            cwd: cwd.map(str::to_string),
            app_version: None,
            ..Default::default()
        }));
        w.close().expect("close log");
    }

    fn paths(records: &[ProjectRecord]) -> Vec<String> {
        records.iter().map(|r| r.path.clone()).collect()
    }

    #[test]
    fn basename_hint_returns_tail_segment() {
        assert_eq!(basename_hint("/home/x/proj").as_deref(), Some("proj"));
        assert_eq!(basename_hint("/home/x/proj///").as_deref(), Some("proj"));
        assert_eq!(basename_hint("C:\\dev\\proj").as_deref(), Some("proj"));
        assert_eq!(basename_hint("my-project").as_deref(), Some("my-project"));
        assert_eq!(basename_hint("/"), None);
        assert_eq!(basename_hint("  "), None);
    }

    #[test]
    fn normalize_path_trims_whitespace_and_trailing_separators() {
        assert_eq!(normalize_path("/work/x/"), "/work/x");
        assert_eq!(normalize_path("  /work/x  "), "/work/x");
        assert_eq!(normalize_path("/"), "");
    }

    #[tokio::test]
    async fn list_projects_adopts_sessions_and_first_seed_memory_labels() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_app();
        app.manage(project_state(tmp.path()));
        let state = app.state::<AppState>();

        // Two sessions in one project (differing only by a trailing slash —
        // one registry row), one elsewhere, one without a cwd.
        seed_session_with_cwd(state.inner(), &uuid::Uuid::new_v4(), Some("/work/proj-alpha/"));
        seed_session_with_cwd(state.inner(), &uuid::Uuid::new_v4(), Some("/work/proj-alpha"));
        seed_session_with_cwd(state.inner(), &uuid::Uuid::new_v4(), Some("/work/proj-beta"));
        seed_session_with_cwd(state.inner(), &uuid::Uuid::new_v4(), None);

        // One memory entry in a project no session has touched yet.
        {
            let mut store = state.memory_store.write().unwrap();
            store
                .add(shannon_core::memory::MemoryEntry::new(
                    "/work/from-memory",
                    shannon_core::memory::MemoryCategory::Context,
                    "seed",
                ))
                .unwrap();
        }

        let rows = list_projects(app.state::<AppState>(), None).await.unwrap();
        assert_eq!(
            paths(&rows),
            ["/work/from-memory", "/work/proj-alpha", "/work/proj-beta"],
            "path-ascending, deduplicated, first-seed memory labels included"
        );
        // Name hint = path tail segment.
        let alpha = rows.iter().find(|r| r.path == "/work/proj-alpha").unwrap();
        assert_eq!(alpha.name.as_deref(), Some("proj-alpha"));

        // Second read is stable (no duplicate adoption, no memory re-seed).
        let rows_again = list_projects(app.state::<AppState>(), None).await.unwrap();
        assert_eq!(paths(&rows_again), paths(&rows));
    }

    #[tokio::test]
    async fn archived_projects_hidden_by_default_and_included_explicitly() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_app();
        app.manage(project_state(tmp.path()));
        let state = app.state::<AppState>();

        seed_session_with_cwd(state.inner(), &uuid::Uuid::new_v4(), Some("/work/keep"));
        seed_session_with_cwd(state.inner(), &uuid::Uuid::new_v4(), Some("/work/gone"));
        list_projects(app.state::<AppState>(), None).await.unwrap();

        let archived = archive_project(app.state::<AppState>(), "/work/gone".into())
            .await
            .unwrap();
        assert!(archived.archived_at_ms.is_some());

        let active = list_projects(app.state::<AppState>(), None).await.unwrap();
        assert_eq!(paths(&active), ["/work/keep"]);
        let all = list_projects(app.state::<AppState>(), Some(true))
            .await
            .unwrap();
        assert_eq!(paths(&all), ["/work/gone", "/work/keep"]);
    }

    #[tokio::test]
    async fn register_project_is_idempotent_and_never_clobbers_curation() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_app();
        app.manage(project_state(tmp.path()));

        let first = register_project(app.state::<AppState>(), "/work/proj/".into())
            .await
            .unwrap();
        assert_eq!(first.path, "/work/proj", "trailing separator normalized");
        assert_eq!(first.name.as_deref(), Some("proj"));

        rename_project(
            app.state::<AppState>(),
            "/work/proj".into(),
            Some("Custom".into()),
        )
        .await
        .unwrap();
        let again = register_project(app.state::<AppState>(), "/work/proj".into())
            .await
            .unwrap();
        assert_eq!(
            again.name.as_deref(),
            Some("Custom"),
            "re-registering must not overwrite the curated name"
        );
    }

    #[tokio::test]
    async fn appearance_commands_set_and_clear() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_app();
        app.manage(project_state(tmp.path()));

        let styled = set_project_appearance(
            app.state::<AppState>(),
            "/work/proj".into(),
            Some("folder".into()),
            Some("teal".into()),
        )
        .await
        .unwrap();
        assert_eq!(styled.icon.as_deref(), Some("folder"));
        assert_eq!(styled.color.as_deref(), Some("teal"));
        let cleared = set_project_appearance(
            app.state::<AppState>(),
            "/work/proj".into(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(cleared.icon, None);
        assert_eq!(cleared.color, None);

        let unarchived = unarchive_project(app.state::<AppState>(), "/work/unknown".into())
            .await
            .unwrap();
        assert_eq!(unarchived.archived_at_ms, None, "auto-created");
    }

    // `set_session_working_dir` is `AppHandle<Wry>`-concrete, so its
    // adoption step is exercised through the runtime-generic
    // `adopt_working_dir` helper the command calls (same extraction
    // pattern as the budget-enforcement tests).
    #[tokio::test]
    async fn curation_commands_target_the_existing_row_despite_trailing_separator() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_app();
        app.manage(project_state(tmp.path()));
        let state = app.state::<AppState>();

        register_project(app.state::<AppState>(), "/work/proj".into())
            .await
            .unwrap();
        assert_eq!(
            state.project_registry().list(true).unwrap().len(),
            1,
            "one row before the separator-variant mutations"
        );

        // Every curation command with a trailing-separator variant of the
        // registered path must mutate the EXISTING row — never auto-create
        // a phantom second row (core auto-creates unknown raw keys).
        let renamed = rename_project(
            app.state::<AppState>(),
            "/work/proj/".into(),
            Some("Custom".into()),
        )
        .await
        .unwrap();
        assert_eq!(renamed.path, "/work/proj", "existing row mutated");
        assert_eq!(renamed.name.as_deref(), Some("Custom"));

        let styled = set_project_appearance(
            app.state::<AppState>(),
            "/work/proj/".into(),
            Some("folder".into()),
            Some("teal".into()),
        )
        .await
        .unwrap();
        assert_eq!(styled.path, "/work/proj");
        assert_eq!(styled.color.as_deref(), Some("teal"));

        let archived = archive_project(app.state::<AppState>(), "/work/proj///".into())
            .await
            .unwrap();
        assert_eq!(archived.path, "/work/proj");
        assert!(archived.archived_at_ms.is_some());
        assert_eq!(
            archived.name.as_deref(),
            Some("Custom"),
            "the curated row is the one archived"
        );

        let unarchived = unarchive_project(app.state::<AppState>(), "/work/proj/".into())
            .await
            .unwrap();
        assert_eq!(unarchived.path, "/work/proj");
        assert_eq!(unarchived.archived_at_ms, None);

        let rows = state.project_registry().list(true).unwrap();
        assert_eq!(rows.len(), 1, "no phantom duplicate row may appear");
        assert_eq!(paths(&rows), ["/work/proj"]);
        assert_eq!(rows[0].name.as_deref(), Some("Custom"));
    }

    #[tokio::test]
    async fn adopt_working_dir_learns_the_new_dir_idempotently() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_app();
        let state_dir = tmp.path().join("state");
        app.manage(project_state(&state_dir));
        let state = app.state::<AppState>();

        let workdir = tempfile::tempdir_in(tmp.path()).unwrap();
        let canonical = std::fs::canonicalize(workdir.path()).unwrap();
        let dir = canonical.to_string_lossy().into_owned();

        adopt_working_dir(state.inner(), &dir);
        let adopted = state.project_registry().get(&dir).unwrap();
        assert!(adopted.is_some(), "working dir must be adopted");
        assert_eq!(
            adopted.unwrap().name.as_deref(),
            canonical.file_name().and_then(|s| s.to_str()),
            "hint is the path tail"
        );

        // A curated rename survives the incremental re-adopt.
        state
            .project_registry()
            .rename(&dir, Some("Custom".into()))
            .unwrap();
        adopt_working_dir(state.inner(), &dir);
        assert_eq!(
            state.project_registry().get(&dir).unwrap().unwrap().name.as_deref(),
            Some("Custom"),
            "adoption must never overwrite an existing row"
        );
    }
}
