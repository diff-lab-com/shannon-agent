//! Session lifecycle Tauri commands (extracted from `commands.rs`).
//!
//! Second step of the commands.rs decomposition (R2-A3 / P1.1). The session
//! cluster is the largest cohesive domain — new/list/search/load/export/
//! switch/delete/rename/duplicate/branch + working_dir. StateManager-backed.
//!
//! P0-4: per-session mutable state (`messages`, `querying`,
//! `cancellation_token`, `current_session_id`) lives in `state.registry`
//! keyed by `SessionKey`. The display list (`state.sessions: Vec<SessionMeta>`)
//! stays on `AppState` because the UI consumes it as a flat list keyed by
//! UUID strings.

use crate::commands::{AppState, ChatMessage, SessionMeta, chrono_timestamp};
use crate::scheduled_commands::TaskWorktreeDto;
use crate::session_registry::SessionKey;
use crate::{config, events, events::event_names};
use serde::Serialize;
use shannon_core::session_log::SessionCuration;
use std::path::Path;
use tauri::Emitter;

/// Tauri event pushed when `switch_session` auto-unarchives an archived
/// session (卡A resume-unarchive: opening a session must never be blocked
/// by its archived flag — the Codex Desktop bug lesson). Payload:
/// [`SessionAutoUnarchived`]. The UI toasts it so the user understands why
/// the conversation left the archived section.
pub const SESSION_AUTO_UNARCHIVED_EVENT: &str = "session-auto-unarchived";

/// Wire payload for [`SESSION_AUTO_UNARCHIVED_EVENT`].
#[derive(Debug, Clone, Serialize)]
pub struct SessionAutoUnarchived {
    /// The session that was unarchived by being opened.
    pub session_id: String,
    /// Its title when known (empty string otherwise).
    pub title: String,
}

/// Create a new session and return its UUID.
///
/// P0-4: materialises the new session in `state.registry`, clears the
/// (about-to-be-stale) message buffer, and promotes the new key to active.
#[tauri::command]
pub async fn new_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4();
    let id_str = id.to_string();
    let title = format!("Session {}", id_str.split('-').next().unwrap_or(&id_str));
    let now = chrono_timestamp();

    // Create the session's L0 log (§4.6): opening + closing a fresh writer
    // records the `session/start` row, which is all a brand-new session has.
    let model = state.client_config.read().await.model.clone();
    shannon_core::session_log::SessionTee::open_in_container(
        state.l0_store().container(),
        &id_str,
        &model,
        None,
    )
    .close();

    // Create session metadata
    let session_meta = SessionMeta {
        id: id_str.clone(),
        title: title.clone(),
        created_at: now,
        message_count: 0,
        working_dir: None,
        parent_id: None,
        branch_point: None,
    };

    // Add to sessions list
    {
        let mut sessions = state.sessions.lock().await;
        sessions.push(session_meta);
    }

    // P0-4: register in the per-session registry and promote to active.
    state.registry.insert(id);
    state.registry.set_active(SessionKey(id));

    // Clear messages for new session
    if let Some(session) = state.registry.get(SessionKey(id)) {
        let mut messages = session.messages.lock().await;
        messages.clear();
    }

    // Emit sessions updated event
    let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());

    Ok(id_str)
}

/// Tier-1 auto-title: derive a session title from the first user message.
///
/// Deterministic truncation — no LLM call. First line only (the session rail
/// must never show newlines), trimmed, capped at 50 chars with an ellipsis.
/// Returns empty for whitespace-only input; callers treat that as "keep the
/// placeholder".
pub(crate) fn derive_title_from_message(message: &str) -> String {
    const MAX_CHARS: usize = 50;
    let first_line = message.lines().next().unwrap_or("").trim();
    if first_line.chars().count() <= MAX_CHARS {
        first_line.to_string()
    } else {
        let truncated: String = first_line.chars().take(MAX_CHARS).collect();
        format!("{truncated}…")
    }
}

/// Promote the first user message of a still-placeholder-titled session to
/// its title (Tier-1 auto-title, 2026-08-26; ed-approved).
///
/// Fires only while the title is the generated `Session {uuid-prefix}`
/// placeholder — a user rename writes a real title and is never
/// overwritten. Mirrors `rename_session`'s persistence path (sessions vec +
/// StateManager save with `Some(title)`); later auto-saves pass
/// `title: None`, which `StateManager::save_session` backfills from disk,
/// so the derived title survives every subsequent save.
pub(crate) async fn auto_title_from_first_message(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    session_id: uuid::Uuid,
    message: &str,
) {
    let title = derive_title_from_message(message);
    if title.is_empty() {
        return;
    }
    let id_str = session_id.to_string();

    let mut sessions = state.sessions.lock().await;
    let Some(session) = sessions.iter_mut().find(|s| s.id == id_str) else {
        return;
    };
    if !session.title.starts_with("Session ") {
        return;
    }
    session.title = title.clone();

    // Persist the curated title in the session sidecar (§4.6) so it is
    // durable even if the app closes before the query completes. The
    // conversation itself is already continuous in events.jsonl.
    drop(sessions);
    let _ = state.l0_store().save_sidecar(
        &session_id,
        &shannon_core::session_log::SessionSidecar {
            title: Some(title),
            ..Default::default()
        },
    );

    let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());
}

/// List all sessions.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn list_sessions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<events::SessionInfo>, String> {
    // P0 sidebar telemetry: clone the display metas out of the lock, then
    // join each with the registry's live `querying` flag (an await — must
    // not happen while holding the std Mutex).
    let metas: Vec<SessionMeta> = state.sessions.lock().await.clone();
    let mut result = Vec::with_capacity(metas.len());
    for s in &metas {
        result.push(session_wire_info(&state, s).await);
    }
    Ok(result)
}

/// P0 sidebar telemetry: build the wire `SessionInfo`, joining the live
/// `running` flag from the session registry, the session's last activity
/// time (events.jsonl mtime, epoch ms), and its archived curation flag
/// (卡A; `None` for legacy non-UUID rows / older wire consumers — see
/// `events::SessionInfo`). All three fields are additive.
async fn session_wire_info(state: &AppState, s: &SessionMeta) -> events::SessionInfo {
    let running = match uuid::Uuid::parse_str(&s.id) {
        Ok(id) => Some(state.registry.is_querying(id).await),
        Err(_) => None,
    };
    let archived = uuid::Uuid::parse_str(&s.id)
        .ok()
        .map(|id| state.l0_store().curation(&id).archived);
    events::SessionInfo {
        id: s.id.clone(),
        title: s.title.clone(),
        created_at: s.created_at,
        message_count: s.message_count,
        working_dir: s.working_dir.clone(),
        parent_id: s.parent_id.clone(),
        branch_point: s.branch_point,
        running,
        updated_at: session_log_mtime(state, &s.id),
        archived,
    }
}

/// Last-activity epoch ms for a session, taken from its L0 log's mtime.
/// `None` when the log doesn't exist yet (brand-new in-memory session) or
/// the id is not a UUID (legacy rows).
fn session_log_mtime(state: &AppState, id: &str) -> Option<i64> {
    let uuid = uuid::Uuid::parse_str(id).ok()?;
    let path = state
        .l0_store()
        .container()
        .join(uuid.to_string())
        .join("events.jsonl");
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    mtime
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

// ---------------------------------------------------------------------------
// Session archive MVP (卡A) — curation flag + active-rail sync + archived lens
// ---------------------------------------------------------------------------

/// One archived session as the 归档 lens renders it (`SidebarSessions`
/// collapsed section): id + title + last activity. Deliberately lean — the
/// lens never needs messages or token counts.
#[derive(Debug, Clone, Serialize)]
pub struct ArchivedSessionRow {
    /// Owning session id.
    pub id: String,
    /// Curated title from the sidecar; `None` → the UI renders its
    /// "untitled" placeholder.
    pub title: Option<String>,
    /// Last activity, epoch ms (latest event timestamp); `None` when
    /// unknown.
    pub updated_at: Option<i64>,
}

/// Rebuild an active-rail [`SessionMeta`] from one `SessionStore::list()`
/// summary ([`StoredSessionInfo`]): single enumeration + sidecar title. The
/// unarchive rebuild uses this — never the full `store.load` projection —
/// so repopulating the rail cannot fail on the session's own log parsing
/// (final review F2) and one listing serves every lookup.
fn session_meta_from_info(info: &shannon_core::session_log::StoredSessionInfo) -> SessionMeta {
    let id = info.session_id.to_string();
    let title = info
        .title
        .clone()
        .unwrap_or_else(|| format!("Session {}", id.split('-').next().unwrap_or(&id)));
    SessionMeta {
        id,
        title,
        created_at: info.created_at.timestamp_millis(),
        // The listing projects turns, not raw messages; the rail does not
        // render this count — turn_count is the honest closest value.
        message_count: info.turn_count,
        working_dir: info.project_path.clone(),
        parent_id: info.parent_session_id.map(|p| p.to_string()),
        branch_point: info.branch_point_message_index,
    }
}

/// Rebuild one session's active-rail row from [`SessionStore::list`] info —
/// the shared unarchive repair behind [`apply_archived_flag`] and
/// [`resume_unarchive_in`]. Returns whether the row was (re)built. Skips
/// silently when the row is already on the rail; best-effort otherwise —
/// when the listing cannot serve the session the failure is logged (the
/// flag is already correct by then, and the next successful retry or
/// restart repairs the row), never fatal.
fn rebuild_rail_row_from_listing(
    store: &shannon_core::session_log::SessionStore,
    sessions: &mut Vec<SessionMeta>,
    session_id: &uuid::Uuid,
) -> bool {
    let id_str = session_id.to_string();
    if sessions.iter().any(|s| s.id == id_str) {
        return false; // already on the rail — nothing to repair
    }
    let info = match store.list() {
        Ok(infos) => infos.into_iter().find(|i| i.session_id == *session_id),
        Err(e) => {
            tracing::warn!(
                error = %e,
                session_id = %id_str,
                "unarchive: store listing failed; rail row not rebuilt (retry or restart repairs)"
            );
            None
        }
    };
    let Some(info) = info else {
        return false;
    };
    sessions.push(session_meta_from_info(&info));
    true
}

/// Shared archive/unarchive mutation (卡A): flip the curation sidecar flag
/// and keep the in-memory display list (`state.sessions`) in sync — archive
/// removes the row (the active rail and every input adapter stop seeing the
/// session), unarchive rebuilds it from `SessionStore::list` info. Returns
/// whether the visible state changed: a flag flip, or a display-list repair
/// on the idempotent path (final review F2 — a previous run may have
/// persisted the flag but died before the row moved, leaving the session in
/// neither the active rail nor the archived lens until restart; the retry
/// must repair instead of short-circuiting).
/// Hermetic by design: the store and list are injected, so tests run on a
/// tempdir container without touching `AppState` or `$HOME`.
pub(crate) fn apply_archived_flag(
    store: &shannon_core::session_log::SessionStore,
    sessions: &mut Vec<SessionMeta>,
    session_id: &uuid::Uuid,
    archived: bool,
) -> Result<bool, String> {
    // Guard: only real sessions (an L0 log on disk) are archivable. Path
    // existence, deliberately not a full read: the flag and the rail repair
    // never depend on parsing the log, so a corrupt log can still be
    // archived / repaired instead of wedging the session.
    let log = shannon_core::session_log::session_log_container_path(
        store.container(),
        &session_id.to_string(),
    );
    if !log.exists() {
        return Err(format!("Session not found: {session_id}"));
    }
    let was_archived = store.curation(session_id).archived;
    let flipped = was_archived != archived;
    if flipped {
        store
            .save_curation(session_id, &SessionCuration { archived })
            .map_err(|e| format!("failed to write session curation: {e}"))?;
    }

    // Display-list sync + repair — on BOTH paths: an archive request always
    // drops a (possibly stale) rail row; an unarchive request rebuilds a
    // missing one from the listing.
    let id_str = session_id.to_string();
    let repaired = if archived {
        let had_row = sessions.iter().any(|s| s.id == id_str);
        sessions.retain(|s| s.id != id_str);
        had_row
    } else {
        rebuild_rail_row_from_listing(store, sessions, session_id)
    };
    Ok(flipped || repaired)
}

/// Archived-lens rows over one container, most recently active first
/// ([`list_archived_sessions`]' body; injected store keeps it hermetic).
pub(crate) fn archived_rows(
    store: &shannon_core::session_log::SessionStore,
) -> Result<Vec<ArchivedSessionRow>, String> {
    let mut rows = Vec::new();
    for info in store.list().map_err(|e| e.to_string())? {
        if !store.curation(&info.session_id).archived {
            continue;
        }
        rows.push(ArchivedSessionRow {
            id: info.session_id.to_string(),
            title: info.title,
            updated_at: Some(info.updated_at.timestamp_millis()),
        });
    }
    Ok(rows)
}

/// Archive a session (卡A): write the curation sidecar flag, drop it from
/// the active rail, and emit `sessions-updated`. Returns `true` when this
/// call flipped the flag (`false` = already archived). On success, when
/// `dream_enabled` is on, a best-effort dream pass rides along (T4).
#[tauri::command]
pub async fn archive_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<bool, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;
    let changed = {
        let mut sessions = state.sessions.lock().await;
        apply_archived_flag(&state.l0_store(), &mut sessions, &session_uuid, true)?
    };
    if changed {
        let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());
        spawn_post_archive_dream(&state, app_handle, session_uuid).await;
    }
    Ok(changed)
}

/// T4 archive callback: a successful archive (when `dream_enabled`) spawns
/// one best-effort dream pass over a 3-day window — archiving is a natural
/// "this thread is done, distill it" moment. Strictly off the hot path:
/// failures are warn-logged, never surfaced, and the pass's own 6h throttle
/// naturally rate-limits bursts of archives.
///
/// Ordering (final review F1): the session is flagged archived BEFORE this
/// callback runs — a crash can then never leave it un-archived — which is
/// exactly why the plain dream window can no longer see it. Its content is
/// distilled through the pass's explicit include: `session_id` is handed to
/// the pass as an `extra_session_ids` entry, which `SessionQuery` fetches by
/// id regardless of the archived flag.
async fn spawn_post_archive_dream(
    state: &AppState,
    app_handle: tauri::AppHandle,
    session_id: uuid::Uuid,
) {
    let dream_enabled = state.desktop_config.read().await.dream_enabled;
    tauri::async_runtime::spawn(async move {
        post_archive_dream_with(dream_enabled, session_id, |id| {
            let app_handle = app_handle.clone();
            async move {
                crate::commands_dream::execute_dream_pass(
                    app_handle,
                    DEFAULT_ARCHIVE_DAYS_BACK,
                    vec![id.to_string()],
                )
                .await
                .map(|_| ())
            }
        })
        .await;
    });
}

/// The T4 handoff over an injected pass runner (the hermetic seam behind
/// [`spawn_post_archive_dream`]): a disabled switch means no pass at all;
/// otherwise the just-archived session id is handed to exactly one pass,
/// and a pass failure is warn-logged (best-effort — the archive itself
/// already succeeded).
async fn post_archive_dream_with<F, Fut>(dream_enabled: bool, session_id: uuid::Uuid, run_pass: F)
where
    F: FnOnce(uuid::Uuid) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    if !dream_enabled {
        return;
    }
    // The pass re-checks its own privacy gates + throttle internally.
    if let Err(e) = run_pass(session_id).await {
        tracing::warn!(
            error = %e,
            "post-archive dream pass failed (best-effort; archive already succeeded)"
        );
    }
}

/// `days_back` for the T4 post-archive dream pass (brief: 3 — same window
/// as the manual entry point).
const DEFAULT_ARCHIVE_DAYS_BACK: u32 = 3;

/// Unarchive a session (卡A): clear the curation flag, rebuild the rail row
/// from the store projection, and emit `sessions-updated`. Returns `true`
/// when this call flipped the flag (`false` = was not archived).
#[tauri::command]
pub async fn unarchive_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<bool, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;
    let changed = {
        let mut sessions = state.sessions.lock().await;
        apply_archived_flag(&state.l0_store(), &mut sessions, &session_uuid, false)?
    };
    if changed {
        let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());
    }
    Ok(changed)
}

/// The 归档 lens: every archived session in the container, most recently
/// active first. Runs off the async runtime (the store listing may
/// re-project logs).
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn list_archived_sessions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ArchivedSessionRow>, String> {
    let store = state.l0_store();
    tokio::task::spawn_blocking(move || archived_rows(&store))
        .await
        .map_err(|e| format!("archived list task failed: {e}"))?
}

/// Resume-unarchive (卡A): if `session_id` is archived, clear the flag,
/// repopulate the display list from the store projection, and emit both
/// `sessions-updated` and `session-auto-unarchived` (the UI toasts the
/// latter). Best-effort by contract: a curation-write failure logs and
/// gives up — resuming must never break because the flag could not be
/// cleared.
async fn auto_unarchive_if_archived(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    session_id: &uuid::Uuid,
) {
    let store = state.l0_store();
    let mut sessions = state.sessions.lock().await;
    let Some(title) = resume_unarchive_in(&store, &mut sessions, session_id) else {
        return;
    };
    drop(sessions);
    let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());
    let _ = app_handle.emit(
        SESSION_AUTO_UNARCHIVED_EVENT,
        SessionAutoUnarchived {
            session_id: session_id.to_string(),
            title,
        },
    );
}

/// The resume-unarchive mutation over injected store + display list (the
/// hermetic seam behind [`auto_unarchive_if_archived`]): returns the
/// session's title when the flag was cleared (empty string when untitled),
/// `None` when the session was not archived or the write failed. The rail
/// rebuild rides the same list-based repair as [`apply_archived_flag`]
/// (final review F2): best-effort — a listing that cannot serve the session
/// is logged, never fatal to the resume.
pub(crate) fn resume_unarchive_in(
    store: &shannon_core::session_log::SessionStore,
    sessions: &mut Vec<SessionMeta>,
    session_id: &uuid::Uuid,
) -> Option<String> {
    if !store.curation(session_id).archived {
        return None;
    }
    if let Err(e) = store.save_curation(session_id, &SessionCuration { archived: false }) {
        tracing::warn!(
            error = %e,
            session_id = %session_id,
            "resume-unarchive: failed to clear archived flag; continuing"
        );
        return None;
    }
    // Rail row: repaired from the listing when missing (logged when the
    // listing cannot serve it). The toast title comes from the row on the
    // rail after the repair — rebuilt or already present.
    rebuild_rail_row_from_listing(store, sessions, session_id);
    let id_str = session_id.to_string();
    Some(
        sessions
            .iter()
            .find(|s| s.id == id_str)
            .map(|s| s.title.clone())
            .unwrap_or_default(),
    )
}

// ---------------------------------------------------------------------------
// Session GC wiring (卡A) — archived-aware retention over the live config
// ---------------------------------------------------------------------------

/// Effective session-GC retention window, resolved from the desktop config
/// plus the `SHANNON_SESSION_GC_ENABLED` env override (卡A documented
/// contract):
///
/// - **Deletion requires config.** `session_gc_enabled == false` (the
///   default) → disabled. No environment value can ever enable deletion.
/// - **The env var can only force-disable.** `SHANNON_SESSION_GC_ENABLED`
///   set to `0` or `false` (case-insensitive) disables the GC even when the
///   config opts in; unset (the common case) or any other value has no
///   effect.
/// - **`Ok(None)` = enabled but windowless.** `session_retention_days`
///   defaults to `None` = never delete; a pass runs and deletes nothing.
pub(crate) fn effective_gc_retention_days(
    cfg: &config::DesktopConfig,
) -> Result<Option<u32>, &'static str> {
    effective_gc_retention_days_with(
        &cfg.session_gc_enabled,
        cfg.session_retention_days,
        std::env::var("SHANNON_SESSION_GC_ENABLED").ok().as_deref(),
    )
}

/// [`effective_gc_retention_days`] with every input injected (hermetic —
/// tests never touch process env).
fn effective_gc_retention_days_with(
    gc_enabled: &bool,
    retention_days: Option<u32>,
    env_override: Option<&str>,
) -> Result<Option<u32>, &'static str> {
    if !gc_enabled {
        return Err("session GC disabled (session_gc_enabled=false)");
    }
    if let Some(v) = env_override {
        if v == "0" || v.eq_ignore_ascii_case("false") {
            return Err("session GC force-disabled via SHANNON_SESSION_GC_ENABLED");
        }
    }
    Ok(retention_days)
}

/// One archived-session GC pass over injected state — the hermetic seam
/// behind [`spawn_session_gc`]. Returns a human-readable outcome (for the
/// log); `Ok` never implies deletion happened: disabled and windowless
/// passes report instead. On deletions, skill candidates whose
/// `example_session_ids` reference a deleted session are named in the log
/// and message (裁决③: report only — the candidate structure is untouched),
/// and any stale display-list rows are dropped.
pub(crate) async fn run_session_gc_with(
    desktop_config: &tokio::sync::RwLock<config::DesktopConfig>,
    sessions: &tokio::sync::Mutex<Vec<SessionMeta>>,
    container: &Path,
    candidates_dir: &Path,
) -> Result<String, String> {
    let days = {
        let cfg = desktop_config.read().await;
        effective_gc_retention_days(&cfg)
    };
    let days = match days {
        Ok(Some(days)) => days,
        Ok(None) => {
            return Ok(
                "session GC enabled but no session_retention_days configured; nothing deleted"
                    .to_string(),
            );
        }
        Err(reason) => return Ok(format!("session GC skipped: {reason}")),
    };
    let report = shannon_core::housekeeping::prune_archived_sessions(
        container,
        Some(days),
        std::time::SystemTime::now(),
    )
    .map_err(|e| format!("session GC failed: {e}"))?;
    if report.deleted_session_ids.is_empty() {
        return Ok(format!(
            "session GC: no archived sessions past the {days}-day retention window"
        ));
    }

    // Reference hygiene (裁决③): candidates keep their example ids; the
    // affected candidate ids land in the log + message only.
    let affected = crate::commands_skill_candidates::candidate_ids_referencing_sessions_in(
        candidates_dir,
        &report.deleted_session_ids,
    );
    // Defensive rail sync: archived rows should already be off the display
    // list; a deletion makes any stale row real — drop it.
    {
        let mut list = sessions.lock().await;
        list.retain(|s| !report.deleted_session_ids.contains(&s.id));
    }
    tracing::info!(
        deleted = report.deleted_session_ids.len(),
        retention_days = days,
        affected_candidates = affected.len(),
        "session GC pruned archived sessions past retention"
    );
    let mut msg = format!(
        "session GC: pruned {} archived session(s) past the {days}-day retention window",
        report.deleted_session_ids.len()
    );
    if !affected.is_empty() {
        msg.push_str("; skill candidates referencing deleted sessions: ");
        msg.push_str(&affected.join(", "));
    }
    Ok(msg)
}

/// Daily archived-session GC loop (卡A). Inert by design unless the user
/// opts in: `session_gc_enabled` (default false) gates every pass and
/// `session_retention_days` (default None = never delete) holds the policy
/// at zero deletions. First pass 10 minutes after startup (let the app
/// settle — GC is never urgent), then every 24 hours; every outcome is
/// log-only.
pub fn spawn_session_gc(state: &AppState) {
    let desktop_config = state.desktop_config.clone();
    let sessions = state.sessions.clone();
    let container = state.l0_store().container().to_path_buf();
    // Empty on failure: a missing desktop dir just means zero candidates to
    // cross-reference (the helper treats it as an empty candidates file).
    let candidates_dir = crate::commands_skill_candidates::desktop_dir().unwrap_or_default();
    tauri::async_runtime::spawn(async move {
        const STARTUP_DELAY: std::time::Duration = std::time::Duration::from_secs(10 * 60);
        const INTERVAL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
        tracing::info!(
            "session GC loop started (inert unless session_gc_enabled and \
             session_retention_days are configured)"
        );
        loop {
            tokio::time::sleep(STARTUP_DELAY).await;
            match run_session_gc_with(&desktop_config, &sessions, &container, &candidates_dir).await
            {
                Ok(msg) => tracing::debug!(outcome = %msg, "session GC pass"),
                Err(e) => tracing::warn!(error = %e, "session GC pass failed"),
            }
            tokio::time::sleep(INTERVAL).await;
        }
    });
}

// ---------------------------------------------------------------------------
// P0 plan dock (ZCode delta ②) — surface the engine's persisted plan doc
// ---------------------------------------------------------------------------

/// One persisted plan file, parsed into the wire shape the right dock's
/// 计划 tab renders. Mirrors the on-disk format `PlanManager::
/// save_plan_to_file` writes (`crates/shannon-tools/src/plan_mode.rs`):
/// `# Plan: {title}` / `Created: {rfc3339}` / `Status: {approved|pending}` /
/// blank line / markdown body.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionPlanInfo {
    pub id: String,
    pub title: String,
    pub status: String,
    pub created_at: String,
    pub content: String,
}

/// Read the session working directory's most recent plan from
/// `<working_dir>/.shannon/plans/*.md` (newest by file mtime). Returns
/// `Ok(None)` when no plan has been persisted — the dock tab renders its
/// empty state. Read-only: plan lifecycle stays owned by the engine tools
/// (`enter_plan_mode` / `exit_plan_mode` / `get_plan_status`).
#[tauri::command]
pub async fn get_session_plan(working_dir: String) -> Result<Option<SessionPlanInfo>, String> {
    if working_dir.trim().is_empty() {
        return Ok(None);
    }
    // Sync file IO on a worker thread — plans are tiny but the scan is
    // still blocking IO, and this command fires on every plan-tab refresh.
    let plan = tokio::task::spawn_blocking(move || {
        let plans_dir = std::path::Path::new(&working_dir)
            .join(".shannon")
            .join("plans");
        let entries = match std::fs::read_dir(&plans_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("Failed to read plans directory: {e}")),
        };
        let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            if newest.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
                newest = Some((mtime, path));
            }
        }
        let Some((_, path)) = newest else {
            return Ok(None);
        };
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) => return Err(format!("Failed to read plan file: {e}")),
        };
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Parse the 3-line header the engine writes (see save_plan_to_file);
        // anything after the first blank line is the markdown body.
        let mut lines = raw.lines();
        let title = lines
            .next()
            .and_then(|l| l.strip_prefix("# Plan: "))
            .unwrap_or("Untitled plan")
            .trim()
            .to_string();
        let created_at = lines
            .next()
            .and_then(|l| l.strip_prefix("Created: "))
            .unwrap_or("")
            .trim()
            .to_string();
        let status = lines
            .next()
            .and_then(|l| l.strip_prefix("Status: "))
            .unwrap_or("pending")
            .trim()
            .to_string();
        let _blank = lines.next();
        let content = lines.collect::<Vec<_>>().join("\n");

        Ok(Some(SessionPlanInfo {
            id,
            title,
            status,
            created_at,
            content,
        }))
    })
    .await
    .map_err(|e| format!("plan read task failed: {e}"))??;
    Ok(plan)
}
/// Search sessions by title substring or message content.
///
/// Title matches rank first; content matches fill the rest. Only the first
/// `CONTENT_SCAN_LIMIT` sessions without a title match have their messages
/// loaded, so cost stays bounded per keystroke.
#[tauri::command]
pub async fn search_sessions(
    state: tauri::State<'_, AppState>,
    query: String,
) -> Result<Vec<events::SessionInfo>, String> {
    const CONTENT_SCAN_LIMIT: usize = 200;

    let query_lower = query.to_lowercase();
    if query_lower.is_empty() {
        return Ok(Vec::new());
    }

    // P0 sidebar telemetry: collect matching metas under the lock (pure
    // sync work), then build wire infos after dropping it — the `running`
    // join awaits the registry and must not hold a std Mutex guard.
    let matched: Vec<SessionMeta> = {
        let sessions = state.sessions.lock().await;
        let mut title_matches: Vec<SessionMeta> = Vec::new();
        let mut content_matches: Vec<SessionMeta> = Vec::new();

        for s in sessions.iter() {
            if s.title.to_lowercase().contains(&query_lower) {
                title_matches.push(s.clone());
                continue;
            }

            if content_matches.len() + title_matches.len() >= CONTENT_SCAN_LIMIT {
                continue;
            }

            if let Ok(uuid) = uuid::Uuid::parse_str(&s.id) {
                // Full-text search on L0 events — the transcript-search successor.
                let hit = state
                    .l0_store()
                    .search_session(&uuid, &query_lower)
                    .map(|hits| !hits.is_empty())
                    .unwrap_or(false);
                if hit {
                    content_matches.push(s.clone());
                }
            }
        }

        title_matches.extend(content_matches);
        title_matches
    };

    let mut result = Vec::with_capacity(matched.len());
    for s in &matched {
        result.push(session_wire_info(&state, s).await);
    }
    Ok(result)
}

/// Load a session by ID.
#[tauri::command]
pub async fn load_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<Vec<ChatMessage>, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;

    // Load by projecting the session's L0 log.
    let session_data = state
        .l0_store()
        .load(&session_uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session not found: {id}"))?;

    // Convert shannon_core Messages to ChatMessages
    let messages: Vec<ChatMessage> = session_data
        .messages
        .into_iter()
        .map(|msg| ChatMessage {
            role: msg.role,
            content: match msg.content {
                shannon_engine::api::MessageContent::Text(t) => t,
                shannon_engine::api::MessageContent::Blocks(blocks) => {
                    // For blocks, extract text content
                    blocks
                        .iter()
                        .filter_map(|b| match b {
                            shannon_engine::api::ContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            },
            timestamp: chrono_timestamp(),
            file_attachments: None,
        })
        .collect();

    // Update current messages
    let session = state.registry.get_or_create(SessionKey(session_uuid));
    {
        let mut current_messages = session.messages.lock().await;
        *current_messages = messages.clone();
    }

    // P0-4: promote to active session.
    state.registry.set_active(SessionKey(session_uuid));

    // Emit session loaded event
    let event_messages: Vec<events::ChatMessage> = messages
        .iter()
        .map(|m| events::ChatMessage {
            role: m.role.clone(),
            content: m.content.clone(),
            timestamp: m.timestamp,
        })
        .collect();
    let _ = app_handle.emit(
        event_names::SESSION_LOADED,
        events::SessionLoaded {
            messages: event_messages,
        },
    );

    Ok(messages)
}

/// Export a session to Markdown or JSON format.
#[tauri::command]
pub async fn export_session(
    state: tauri::State<'_, AppState>,
    id: String,
    format: String,
) -> Result<String, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;

    let session_data = state
        .l0_store()
        .load(&session_uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session not found: {id}"))?;

    let title = session_data
        .metadata
        .title
        .as_deref()
        .unwrap_or("Untitled Session");

    match format.as_str() {
        "markdown" | "md" => {
            let mut md = format!("# {title}\n\n");
            md.push_str(&format!(
                "Exported: {}\n\n---\n\n",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            ));
            for msg in &session_data.messages {
                let role_label = match msg.role.as_str() {
                    "user" => "**You**",
                    "assistant" => "**Assistant**",
                    "system" => "**System**",
                    other => &format!("**{other}**"),
                };
                let content = match &msg.content {
                    shannon_engine::api::MessageContent::Text(t) => t.clone(),
                    shannon_engine::api::MessageContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| match b {
                            shannon_engine::api::ContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                md.push_str(&format!("### {role_label}\n\n{content}\n\n---\n\n"));
            }
            Ok(md)
        }
        "json" => {
            let messages: Vec<serde_json::Value> = session_data
                .messages
                .iter()
                .map(|msg| {
                    let content = match &msg.content {
                        shannon_engine::api::MessageContent::Text(t) => t.clone(),
                        shannon_engine::api::MessageContent::Blocks(blocks) => blocks
                            .iter()
                            .filter_map(|b| match b {
                                shannon_engine::api::ContentBlock::Text { text } => {
                                    Some(text.clone())
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                    };
                    serde_json::json!({
                        "role": msg.role,
                        "content": content,
                    })
                })
                .collect();
            let export = serde_json::json!({
                "id": id,
                "title": title,
                "exported_at": chrono::Local::now().to_rfc3339(),
                "message_count": messages.len(),
                "messages": messages,
            });
            serde_json::to_string_pretty(&export).map_err(|e| e.to_string())
        }
        _ => Err(format!(
            "Unsupported format: {format}. Use 'markdown' or 'json'."
        )),
    }
}

/// Switch to a different session, saving the current one first.
#[tauri::command]
pub async fn switch_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<Vec<ChatMessage>, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;

    // 卡A resume-unarchive (the Codex-shipped-a-bug lesson): opening an
    // archived session must never be blocked by its archived flag — clear
    // it first so the session silently returns to the active rail, and
    // toast the UI about it.
    auto_unarchive_if_archived(&state, &app_handle, &session_uuid).await;

    // (§4.6) No explicit save needed before switching: every turn is already
    // durable in events.jsonl via the engine tee.

    // Load new session by projecting its L0 log.
    let messages = match state
        .l0_store()
        .load(&session_uuid)
        .map_err(|e| e.to_string())?
    {
        Some(data) => data
            .messages
            .into_iter()
            .map(|msg| ChatMessage {
                role: msg.role,
                content: match msg.content {
                    shannon_engine::api::MessageContent::Text(t) => t,
                    shannon_engine::api::MessageContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| match b {
                            shannon_engine::api::ContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                },
                timestamp: chrono_timestamp(),
                file_attachments: None,
            })
            .collect(),
        None => Vec::new(),
    };

    // Update state (P0-4: register the new session and promote to active).
    state.registry.insert(session_uuid);
    let new_session = state.registry.get_or_create(SessionKey(session_uuid));
    {
        let mut msgs = new_session.messages.lock().await;
        *msgs = messages.clone();
    }
    state.registry.set_active(SessionKey(session_uuid));

    // Restore working_dir from session metadata if present.
    {
        let sessions = state.sessions.lock().await;
        if let Some(meta) = sessions.iter().find(|s| s.id == id) {
            if let Some(ref wd) = meta.working_dir {
                let _ = std::env::set_current_dir(wd);
                let mut desktop_cfg = state.desktop_config.write().await;
                desktop_cfg.working_dir = Some(wd.clone());
                let _ = app_handle.emit(
                    event_names::CONFIG_UPDATED,
                    events::ConfigUpdatedPayload {
                        key: "working_dir".into(),
                        value: wd.clone(),
                    },
                );
            }
        }
    }

    // Emit session loaded event
    let event_messages: Vec<events::ChatMessage> = messages
        .iter()
        .map(|m| events::ChatMessage {
            role: m.role.clone(),
            content: m.content.clone(),
            timestamp: m.timestamp,
        })
        .collect();
    let _ = app_handle.emit(
        event_names::SESSION_LOADED,
        events::SessionLoaded {
            messages: event_messages,
        },
    );

    // T5: the user just opened this session — an outstanding `session_failed`
    // entry has now been seen, so it is resolved (mark read). Best-effort.
    crate::inbox_session_events::resolve_session_failure(
        state.inbox_store().as_ref(),
        &app_handle,
        &id,
    );

    Ok(messages)
}

/// Set working directory for a session. Updates in-memory metadata, the
/// process cwd, and the persisted desktop config. Pass an empty string to
/// reset to the Shannon home directory.
#[tauri::command]
pub async fn set_session_working_dir(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
    path: String,
) -> Result<(), String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;
    let wd = if path.trim().is_empty() {
        None
    } else {
        let canonical =
            std::fs::canonicalize(&path).map_err(|e| format!("Invalid path {path}: {e}"))?;
        Some(canonical.to_string_lossy().into_owned())
    };

    // Update session metadata
    {
        let mut sessions = state.sessions.lock().await;
        if let Some(meta) = sessions.iter_mut().find(|s| s.id == id) {
            meta.working_dir = wd.clone();
        }
    }

    // If this is the current session, switch process cwd + desktop config
    let current = state.registry.active_key();
    let is_current = current == Some(SessionKey(session_uuid));
    if is_current {
        if let Some(ref p) = wd {
            let _ = std::env::set_current_dir(p);
        }
        let mut desktop_cfg = state.desktop_config.write().await;
        desktop_cfg.working_dir = wd.clone();
        drop(desktop_cfg);
        let desktop_cfg = state.desktop_config.read().await;
        let _ = config::save_config(&desktop_cfg);
        let _ = app_handle.emit(
            event_names::CONFIG_UPDATED,
            events::ConfigUpdatedPayload {
                key: "working_dir".into(),
                value: wd.clone().unwrap_or_default(),
            },
        );
    }

    let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());
    Ok(())
}

/// Create an isolated git worktree for a session and bind it as the session's
/// working directory. Delegates to [`shannon_core::scheduled_worktree::create_for_task`]
/// — the same helper used by scheduled tasks — so session and task worktrees
/// live under the same base dir (`.shannon/scheduled-worktrees/` by default).
///
/// Safe to call repeatedly: if the worktree path already exists, the helper
/// returns the existing descriptor instead of erroring.
#[tauri::command]
pub async fn create_session_worktree(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
    title: String,
) -> Result<TaskWorktreeDto, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;

    // Verify session exists before creating worktree (avoid orphan worktrees)
    {
        let sessions = state.sessions.lock().await;
        let exists = sessions.iter().any(|s| s.id == id);
        if !exists {
            return Err(format!("Session not found: {id}"));
        }
    }

    let id_str = session_uuid.to_string();
    let base = shannon_core::scheduled_worktree::default_base_dir();
    let wt = shannon_core::scheduled_worktree::create_for_task(&id_str, &title, &base)
        .map_err(|e| e.to_string())?;
    let wt_path = wt.path.to_string_lossy().into_owned();

    // Update session metadata to point at the worktree
    {
        let mut sessions = state.sessions.lock().await;
        if let Some(meta) = sessions.iter_mut().find(|s| s.id == id_str) {
            meta.working_dir = Some(wt_path.clone());
        }
    }

    // If this is the current session, switch process cwd + desktop config
    let current = state.registry.active_key();
    if current == Some(SessionKey(session_uuid)) {
        let _ = std::env::set_current_dir(&wt_path);
        let mut desktop_cfg = state.desktop_config.write().await;
        desktop_cfg.working_dir = Some(wt_path.clone());
        drop(desktop_cfg);
        let desktop_cfg = state.desktop_config.read().await;
        let _ = config::save_config(&desktop_cfg);
        let _ = app_handle.emit(
            event_names::CONFIG_UPDATED,
            events::ConfigUpdatedPayload {
                key: "working_dir".into(),
                value: wt_path.clone(),
            },
        );
    }

    let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());
    Ok(wt.into())
}

/// Delete a session by ID. If the session had a bound worktree (working_dir
/// pointing inside the default worktree base), the worktree is removed too —
/// best-effort, logs failures but does not block session deletion.
#[tauri::command]
pub async fn delete_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<bool, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;

    // Capture working_dir before deleting so we can clean up worktree
    let working_dir = {
        let sessions = state.sessions.lock().await;
        sessions
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.working_dir.clone())
    };

    // Delete removes the session's whole L0 directory (log + sidecar).
    let deleted = state
        .l0_store()
        .delete(&session_uuid)
        .map_err(|e| e.to_string())?;

    if deleted {
        // Remove from sessions list
        {
            let mut sessions = state.sessions.lock().await;
            sessions.retain(|s| s.id != id);
        }

        // Best-effort worktree cleanup: if working_dir lives under the
        // default worktree base dir, remove the worktree. Failures are
        // logged but do not block session deletion — orphan worktrees can
        // be cleaned up later via prune_task_worktrees.
        if let Some(wd) = working_dir {
            let base = shannon_core::scheduled_worktree::default_base_dir();
            let wd_path = std::path::Path::new(&wd);
            if wd_path.starts_with(&base) {
                if let Err(e) = shannon_core::scheduled_worktree::remove(wd_path) {
                    tracing::warn!(
                        worktree = %wd,
                        error = %e,
                        "failed to remove worktree during session deletion;                          use prune_task_worktrees to clean up later"
                    );
                }
            }
        }

        // Emit sessions updated event
        let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());

        Ok(true)
    } else {
        Ok(false)
    }
}

/// Rename a session by ID.
#[tauri::command]
pub async fn rename_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
    title: String,
) -> Result<bool, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;

    // Update session metadata in sessions list
    let mut sessions = state.sessions.lock().await;
    if let Some(session) = sessions.iter_mut().find(|s| s.id == id) {
        session.title = title.clone();

        // Persist the curated title in the sidecar (§4.6); P0-4 note still
        // applies to the in-memory list above — the store never reads it.
        let _ = state.l0_store().save_sidecar(
            &session_uuid,
            &shannon_core::session_log::SessionSidecar {
                title: Some(title),
                ..Default::default()
            },
        );

        // Emit sessions updated event
        let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());

        Ok(true)
    } else {
        Ok(false)
    }
}

/// Duplicate a session by ID.
#[tauri::command]
pub async fn duplicate_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<events::SessionInfo, String> {
    let session_uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("Invalid UUID: {e}"))?;

    // Find original session
    let sessions = state.sessions.lock().await;
    let original_session = sessions
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("Session not found: {id}"))?;

    // Load original session data (projected from its L0 log)
    let session_data = state
        .l0_store()
        .load(&session_uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session data not found: {id}"))?;

    // Create the duplicate by replaying every event into a fresh log.
    let new_id = uuid::Uuid::new_v4();
    let new_id_str = new_id.to_string();
    let new_title = format!("Copy of {}", original_session.title);
    let now = chrono_timestamp();

    {
        use shannon_types::session_event::SessionEventBody;
        let events = state
            .l0_store()
            .read_events(&session_uuid)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let mut w = shannon_core::session_log::SessionLogWriter::open_layout(
            state.l0_store().container(),
            &new_id_str,
        )
        .map_err(|e| e.to_string())?;
        if events.is_empty() {
            // Keep empty duplicates listable: an explicit start row marks them.
            w.record(SessionEventBody::SessionStart(
                shannon_types::session_event::SessionStartPayload {
                    model: state.client_config.read().await.model.clone(),
                    provider: None,
                    cwd: None,
                    app_version: None,
                    // Desktop-duplicated sessions keep the writer defaults:
                    // the os/arch/cdp signals describe the live host, which
                    // is exactly this machine.
                    os: Some(std::env::consts::OS.to_string()),
                    arch: Some(std::env::consts::ARCH.to_string()),
                    browser_cdp: Some(
                        std::env::var("SHANNON_BROWSER_CDP")
                            .map(|v| !v.trim().is_empty())
                            .unwrap_or(false),
                    ),
                },
            ));
        }
        for event in events {
            w.record(event.body);
        }
        w.close().map_err(|e| e.to_string())?;
    }

    state
        .l0_store()
        .save_sidecar(
            &new_id,
            &shannon_core::session_log::SessionSidecar {
                title: Some(new_title.clone()),
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;

    // Add to sessions list
    let new_session_meta = SessionMeta {
        id: new_id_str.clone(),
        title: new_title.clone(),
        created_at: now,
        message_count: session_data.messages.len(),
        working_dir: None,
        parent_id: None,
        branch_point: None,
    };
    drop(sessions);
    {
        let mut sessions = state.sessions.lock().await;
        sessions.push(new_session_meta);
    }

    // Emit sessions updated event
    let _ = app_handle.emit(event_names::SESSIONS_UPDATED, ());

    Ok(events::SessionInfo {
        id: new_id_str,
        title: new_title,
        created_at: now,
        message_count: session_data.messages.len(),
        working_dir: None,
        parent_id: None,
        branch_point: None,
        running: Some(false),
        updated_at: None,
        archived: Some(false),
    })
}

/// Internal helper for branch_session (shared with tests).
pub(crate) async fn branch_session_internal(
    state: &AppState,
    app_handle: Option<&tauri::AppHandle>,
    parent_id: String,
    branch_point: usize,
) -> Result<events::SessionInfo, String> {
    let parent_uuid =
        uuid::Uuid::parse_str(&parent_id).map_err(|e| format!("Invalid UUID: {e}"))?;

    // Find parent session
    let sessions = state.sessions.lock().await;
    let parent_session = sessions
        .iter()
        .find(|s| s.id == parent_id)
        .ok_or_else(|| format!("Session not found: {parent_id}"))?;

    // Clone parent session data before dropping sessions
    let parent_title = parent_session.title.clone();
    let parent_working_dir = parent_session.working_dir.clone();

    // Load parent session data (projected from L0)
    let session_data = state
        .l0_store()
        .load(&parent_uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session data not found: {parent_id}"))?;

    // Create new session carrying messages up to AND INCLUDING the branch
    // point — the desktop convention (`take(branch_point + 1)`).
    let new_title = format!("Branch of {parent_title}");
    let now = chrono_timestamp();

    if branch_point >= session_data.messages.len() {
        return Err(format!(
            "Branch point {} out of bounds: session has {} messages (valid range: 0-{})",
            branch_point,
            session_data.messages.len(),
            session_data.messages.len().saturating_sub(1)
        ));
    }
    let keep_messages = branch_point + 1;

    let stored_branch = state
        .l0_store()
        .create_branch(&parent_uuid, keep_messages, Some(new_title.clone()))
        .map_err(|e| e.to_string())?;

    let branch_message_count = keep_messages;
    let _ = &session_data;

    // Drop sessions lock before re-acquiring for push
    drop(sessions);

    // Add to sessions list with parent/branch info
    let new_session_meta = SessionMeta {
        id: stored_branch.session_id.to_string(),
        title: new_title.clone(),
        created_at: now,
        message_count: branch_message_count,
        working_dir: parent_working_dir.clone(),
        parent_id: Some(parent_id.clone()),
        branch_point: Some(branch_point),
    };
    {
        let mut sessions = state.sessions.lock().await;
        sessions.push(new_session_meta);
    }

    // Emit sessions updated event
    if let Some(handle) = app_handle {
        let _ = handle.emit(event_names::SESSIONS_UPDATED, ());
    }

    Ok(events::SessionInfo {
        id: stored_branch.session_id.to_string(),
        title: new_title,
        created_at: now,
        message_count: branch_message_count,
        working_dir: parent_working_dir,
        parent_id: Some(parent_id),
        branch_point: Some(branch_point),
        running: Some(false),
        updated_at: None,
        archived: Some(false),
    })
}

/// Branch a session at a specific message index.
///
/// Creates a new session with messages up to (and including) the branch point,
/// copying the first N messages from the parent session. Sets parent_id and
/// branch_point to track the relationship.
#[tauri::command]
pub async fn branch_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    parent_id: String,
    branch_point: usize,
) -> Result<events::SessionInfo, String> {
    branch_session_internal(&state, Some(&app_handle), parent_id, branch_point).await
}

/// Turn Timeline (§4.14): the per-session projection consumed by the
/// Timeline panel — turns × tool waterfall rows × token/cost cumulative
/// curve. The fold itself lives in `project_turn_timeline` next to the other
/// L0 projections; this command is transport only.
#[tauri::command]
pub async fn trace_timeline(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<shannon_core::session_log::TurnTimeline, String> {
    let session_uuid =
        uuid::Uuid::parse_str(&session_id).map_err(|e| format!("Invalid UUID: {e}"))?;

    let events = match state.l0_store().read_events(&session_uuid) {
        Ok(Some(events)) => events,
        Ok(None) => return Err(format!("Session not found: {session_id}")),
        Err(e) => return Err(e.to_string()),
    };
    if events.is_empty() {
        return Err(format!("Session not found: {session_id}"));
    }

    Ok(shannon_core::session_log::project_turn_timeline(&events))
}

#[cfg(test)]
mod archive_tests {
    // 卡A command-layer tests. Hermetic by construction: fixtures are real
    // sessions written through the session_log writer into a tempdir
    // container, and the mutations under test take injected store/display
    // list — nothing touches `AppState` or `$HOME`.
    #![allow(clippy::unwrap_used)]
    use super::*;
    use shannon_core::session_log::{SessionLogWriter, SessionQuery, SessionStore};
    use shannon_types::session_event::{
        SessionEventBody, SessionStartPayload, TurnStartPayload, UserMessagePayload,
    };
    use std::sync::Arc;

    fn store(tmp: &tempfile::TempDir) -> SessionStore {
        SessionStore::new(tmp.path().join("sessions"))
    }

    /// Seed one real session through the L0 writer (session/start + one
    /// user turn) — the same write path production uses.
    fn seed_session(store: &SessionStore, id: &uuid::Uuid, title: Option<&str>) {
        let mut w =
            SessionLogWriter::open_layout(store.container(), &id.to_string()).expect("open log");
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
            content: "hello there".into(),
            attachment_count: 0,
        }));
        w.close().expect("close log");
        if let Some(t) = title {
            store
                .save_sidecar(
                    id,
                    &shannon_core::session_log::SessionSidecar {
                        title: Some(t.into()),
                        ..Default::default()
                    },
                )
                .expect("save sidecar");
        }
    }

    fn display_list(ids: &[&uuid::Uuid]) -> Vec<SessionMeta> {
        ids.iter()
            .map(|id| SessionMeta {
                id: id.to_string(),
                title: "Session".into(),
                created_at: 1,
                message_count: 1,
                working_dir: None,
                parent_id: None,
                branch_point: None,
            })
            .collect()
    }

    #[test]
    fn archive_hides_from_active_list_and_surfaces_in_archived_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        seed_session(&store, &a, Some("Archived one"));
        seed_session(&store, &b, None);
        let mut sessions = display_list(&[&a, &b]);

        // archive → the display list hides it, the archived lens shows it,
        // and the input adapter (SessionQuery) stops seeing it.
        assert!(apply_archived_flag(&store, &mut sessions, &a, true).unwrap());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, b.to_string());
        assert!(store.curation(&a).archived);

        let rows = archived_rows(&store).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, a.to_string());
        assert_eq!(rows[0].title.as_deref(), Some("Archived one"));
        assert!(rows[0].updated_at.is_some());

        let query = SessionQuery::new(store.container().to_path_buf());
        let visible: Vec<_> = query.list_recent(7, false).unwrap();
        assert_eq!(visible.len(), 1, "archived sessions leave the input layer");
        assert_eq!(visible[0].session_id, b);
        assert_eq!(query.list_recent(7, true).unwrap().len(), 2);
    }

    #[test]
    fn unarchive_rebuilds_the_rail_row_from_the_store_projection() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let a = uuid::Uuid::new_v4();
        seed_session(&store, &a, Some("Restorable"));
        let mut sessions = display_list(&[&a]);
        assert!(apply_archived_flag(&store, &mut sessions, &a, true).unwrap());
        assert!(sessions.is_empty());

        // unarchive → the rail row is rebuilt from StoredSessionInfo (title
        // from the sidecar, epoch-ms created_at, project working dir) so the
        // list repopulates without a restart.
        assert!(apply_archived_flag(&store, &mut sessions, &a, false).unwrap());
        assert!(!store.curation(&a).archived);
        assert_eq!(sessions.len(), 1);
        let meta = &sessions[0];
        assert_eq!(meta.id, a.to_string());
        assert_eq!(meta.title, "Restorable");
        assert_eq!(meta.working_dir.as_deref(), Some("/proj"));
        assert!(meta.created_at > 1_000_000_000_000, "epoch milliseconds");
        assert_eq!(meta.message_count, 1, "one seeded turn");
    }

    #[test]
    fn idempotent_unarchive_repairs_a_rail_row_missing_from_a_failed_rebuild() {
        // Final review F2: the old code persisted the unarchive flag and
        // THEN rebuilt the row via the full projection — a failure between
        // the two wedged the session out of both lists (flag false, row
        // absent), and the retry hit the idempotent short-circuit. The
        // retry must now repair the rail.
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let a = uuid::Uuid::new_v4();
        seed_session(&store, &a, Some("Wedged"));

        // The wedge: flag already cleared, row nowhere.
        store
            .save_curation(&a, &SessionCuration { archived: false })
            .unwrap();
        let mut sessions = Vec::new();
        assert!(
            apply_archived_flag(&store, &mut sessions, &a, false).unwrap(),
            "the repair is a visible state change, not a no-op"
        );
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, a.to_string());
        assert_eq!(sessions[0].title, "Wedged");

        // Once repaired, the next request is a true no-op again.
        let mut again = sessions.clone();
        assert!(!apply_archived_flag(&store, &mut again, &a, false).unwrap());
        assert_eq!(again.len(), 1);
    }

    #[test]
    fn idempotent_archive_drops_a_stale_rail_row() {
        // The mirror repair: the flag is already true (archived) but the
        // row is back on the rail (restored by a crashed run) — the
        // archive request must still remove it and report the change.
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let a = uuid::Uuid::new_v4();
        seed_session(&store, &a, Some("Stale"));
        store
            .save_curation(&a, &SessionCuration { archived: true })
            .unwrap();
        let mut sessions = display_list(&[&a]);
        assert!(apply_archived_flag(&store, &mut sessions, &a, true).unwrap());
        assert!(sessions.is_empty());

        // Truly idempotent afterwards.
        let mut again = display_list(&[]);
        assert!(!apply_archived_flag(&store, &mut again, &a, true).unwrap());
        assert!(again.is_empty());
    }

    #[test]
    fn unarchive_rebuild_survives_a_corrupt_log_via_the_listing() {
        // Final review F2: the rail rebuild must depend on the
        // `SessionStore::list` summaries, not the full `store.load`
        // projection. Corrupt the log IN PLACE (same byte length + restored
        // mtime, so the E-9 index still validates): the projection fails on
        // the unparsable line while the listing serves from its cache.
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let a = uuid::Uuid::new_v4();
        seed_session(&store, &a, Some("Durable"));
        let mut rail = display_list(&[&a]);
        assert!(apply_archived_flag(&store, &mut rail, &a, true).unwrap());

        let log = store.container().join(a.to_string()).join("events.jsonl");
        let mtime = std::fs::metadata(&log).unwrap().modified().unwrap();
        let raw = std::fs::read_to_string(&log).unwrap();
        let corrupted = raw.replacen('{', "x", 1);
        assert_eq!(
            corrupted.len(),
            raw.len(),
            "in-place corruption keeps the byte length"
        );
        std::fs::write(&log, corrupted).unwrap();
        std::fs::File::open(&log)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        assert!(
            store.load(&a).is_err(),
            "fixture: the full projection must fail on the corrupt log"
        );

        // Wedged state (flag already cleared, row absent) + retry: the rail
        // is rebuilt from the listing anyway — no `store.load` dependency.
        store
            .save_curation(&a, &SessionCuration { archived: false })
            .unwrap();
        let mut sessions = Vec::new();
        assert!(apply_archived_flag(&store, &mut sessions, &a, false).unwrap());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "Durable");
    }

    #[tokio::test]
    async fn post_archive_dream_hands_the_archived_id_to_exactly_one_pass() {
        // Final review F1: the archive callback must pass the just-archived
        // session id into the dream pass (its explicit include) — verified
        // through the injected runner seam, in the style of the other
        // hermetic seams in this module.
        let archived = uuid::Uuid::new_v4();
        let seen: Arc<std::sync::Mutex<Vec<uuid::Uuid>>> = Arc::default();
        let spy = seen.clone();
        post_archive_dream_with(true, archived, move |id| {
            let spy = spy.clone();
            async move {
                spy.lock().unwrap().push(id);
                Ok(())
            }
        })
        .await;
        assert_eq!(
            *seen.lock().unwrap(),
            vec![archived],
            "exactly one pass receives the archived id"
        );

        // Switch off → no pass at all.
        let disabled: Arc<std::sync::Mutex<Vec<uuid::Uuid>>> = Arc::default();
        let spy = disabled.clone();
        post_archive_dream_with(false, archived, move |id| {
            let spy = spy.clone();
            async move {
                spy.lock().unwrap().push(id);
                Ok(())
            }
        })
        .await;
        assert!(disabled.lock().unwrap().is_empty(), "disabled → no pass");
    }

    #[test]
    fn apply_flag_is_idempotent_and_rejects_unknown_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let a = uuid::Uuid::new_v4();
        seed_session(&store, &a, None);
        let mut sessions = display_list(&[&a]);

        assert!(apply_archived_flag(&store, &mut sessions, &a, true).unwrap());
        assert!(
            !apply_archived_flag(&store, &mut sessions, &a, true).unwrap(),
            "already archived → no-op"
        );
        assert!(
            sessions.is_empty(),
            "the only active row was removed; the no-op removed nothing twice"
        );

        assert!(
            apply_archived_flag(&store, &mut sessions, &a, false).unwrap(),
            "unarchive flips the flag back"
        );
        assert!(
            !apply_archived_flag(&store, &mut sessions, &a, false).unwrap(),
            "already unarchived → no-op"
        );
        let missing = uuid::Uuid::new_v4();
        let err = apply_archived_flag(&store, &mut sessions, &missing, true).unwrap_err();
        assert!(err.contains("Session not found"), "{err}");
    }

    #[test]
    fn resume_unarchive_clears_the_flag_and_reports_the_title() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let plain = uuid::Uuid::new_v4();
        let archived = uuid::Uuid::new_v4();
        seed_session(&store, &plain, None);
        seed_session(&store, &archived, Some("Welcome back"));
        // Archive one through the real mutation; it leaves the display list.
        let mut staging = Vec::new();
        assert!(apply_archived_flag(&store, &mut staging, &archived, true).unwrap());
        assert!(staging.is_empty());
        let mut sessions = Vec::new();

        // Resuming a not-archived session is a no-op.
        assert!(resume_unarchive_in(&store, &mut sessions, &plain).is_none());
        // Resuming an archived session unarchives + repopulates + names it.
        let title = resume_unarchive_in(&store, &mut sessions, &archived).unwrap();
        assert_eq!(title, "Welcome back");
        assert!(!store.curation(&archived).archived);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, archived.to_string());
        // A second resume (flag already cleared) is nothing.
        assert!(resume_unarchive_in(&store, &mut sessions, &archived).is_none());
    }

    fn gc_config(enabled: bool, retention: Option<u32>) -> config::DesktopConfig {
        let mut cfg = config::DesktopConfig::default();
        cfg.session_gc_enabled = enabled;
        cfg.session_retention_days = retention;
        cfg
    }

    #[test]
    fn gc_gate_requires_config_and_env_can_only_force_disable() {
        // Deletion requires config.
        assert!(effective_gc_retention_days_with(&false, Some(30), None).is_err());
        // …and no env value can enable it.
        assert!(effective_gc_retention_days_with(&false, Some(30), Some("1")).is_err());
        // Enabled + window.
        assert_eq!(
            effective_gc_retention_days_with(&true, Some(90), None).unwrap(),
            Some(90)
        );
        // Enabled + no window = runs but deletes nothing.
        assert_eq!(
            effective_gc_retention_days_with(&true, None, None).unwrap(),
            None
        );
        // The env var can only force-disable.
        assert!(effective_gc_retention_days_with(&true, Some(90), Some("0")).is_err());
        assert!(effective_gc_retention_days_with(&true, Some(90), Some("FALSE")).is_err());
        // Any other value is inert, never an enabler or disabler.
        assert_eq!(
            effective_gc_retention_days_with(&true, Some(90), Some("yes")).unwrap(),
            Some(90)
        );
    }

    fn seed_old_archived_and_active(
        container: &std::path::Path,
    ) -> (uuid::Uuid, uuid::Uuid, uuid::Uuid) {
        let old_archived = uuid::Uuid::new_v4();
        let recent_archived = uuid::Uuid::new_v4();
        let old_active = uuid::Uuid::new_v4();
        let st = SessionStore::new(container.to_path_buf());
        for id in [&old_archived, &recent_archived, &old_active] {
            seed_session(&st, id, None);
        }
        // Backdate the two old sessions' logs (file + dir) past any window.
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(40 * 24 * 3600);
        for id in [&old_archived, &old_active] {
            let log = container.join(id.to_string()).join("events.jsonl");
            std::fs::File::open(&log)
                .unwrap()
                .set_modified(old)
                .unwrap();
            std::fs::File::open(log.parent().unwrap())
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
        for id in [&old_archived, &recent_archived] {
            st.save_curation(id, &SessionCuration { archived: true })
                .unwrap();
        }
        (old_archived, recent_archived, old_active)
    }

    #[tokio::test]
    async fn gc_pass_deletes_only_archived_past_retention_and_reports() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        std::fs::create_dir_all(&container).unwrap();
        let (old_archived, recent_archived, old_active) = seed_old_archived_and_active(&container);

        let desktop_config = Arc::new(tokio::sync::RwLock::new(gc_config(true, Some(30))));
        let stale_row = SessionMeta {
            id: old_archived.to_string(),
            title: "stale".into(),
            created_at: 1,
            message_count: 0,
            working_dir: None,
            parent_id: None,
            branch_point: None,
        };
        let sessions = Arc::new(tokio::sync::Mutex::new(vec![stale_row]));
        let candidates = tempfile::tempdir().unwrap();

        let msg = run_session_gc_with(&desktop_config, &sessions, &container, candidates.path())
            .await
            .unwrap();
        assert!(msg.contains("pruned 1"), "{msg}");
        assert!(
            !container.join(old_archived.to_string()).exists(),
            "archived + past retention → deleted"
        );
        assert!(
            container.join(recent_archived.to_string()).exists(),
            "archived but inside the window → kept"
        );
        assert!(
            container.join(old_active.to_string()).exists(),
            "unarchived is never auto-deleted, however old"
        );
        assert!(
            sessions.lock().await.is_empty(),
            "stale display rows are dropped by the pass"
        );
    }

    #[tokio::test]
    async fn gc_pass_is_inert_disabled_and_without_window() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        std::fs::create_dir_all(&container).unwrap();
        let (old_archived, _, _) = seed_old_archived_and_active(&container);
        let sessions = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let candidates = tempfile::tempdir().unwrap();

        // Disabled (the default): skipped, nothing deleted.
        let disabled = Arc::new(tokio::sync::RwLock::new(gc_config(false, Some(30))));
        let msg = run_session_gc_with(&disabled, &sessions, &container, candidates.path())
            .await
            .unwrap();
        assert!(msg.contains("skipped"), "{msg}");
        assert!(container.join(old_archived.to_string()).exists());

        // Enabled but windowless (session_retention_days = None): zero
        // deletions even though the session is archived and ancient.
        let windowless = Arc::new(tokio::sync::RwLock::new(gc_config(true, None)));
        let msg = run_session_gc_with(&windowless, &sessions, &container, candidates.path())
            .await
            .unwrap();
        assert!(msg.contains("no session_retention_days"), "{msg}");
        assert!(
            container.join(old_archived.to_string()).exists(),
            "retention None → zero deletions even with GC enabled"
        );
    }

    #[tokio::test]
    async fn gc_pass_names_candidates_referencing_deleted_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("sessions");
        std::fs::create_dir_all(&container).unwrap();
        let (old_archived, _, _) = seed_old_archived_and_active(&container);

        // One candidate references the doomed session, another does not.
        let candidates = tempfile::tempdir().unwrap();
        let hitting = crate::commands_skill_candidates::SkillCandidate {
            id: "sig-hit".into(),
            example_session_ids: vec![old_archived.to_string()],
            ..sample_skill_candidate("sig-hit")
        };
        let other = sample_skill_candidate("sig-other");
        crate::commands_skill_candidates::append_candidate_in(candidates.path(), hitting).unwrap();
        crate::commands_skill_candidates::append_candidate_in(candidates.path(), other).unwrap();

        let desktop_config = Arc::new(tokio::sync::RwLock::new(gc_config(true, Some(30))));
        let sessions = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let msg = run_session_gc_with(&desktop_config, &sessions, &container, candidates.path())
            .await
            .unwrap();
        assert!(msg.contains("sig-hit"), "{msg}");
        assert!(!msg.contains("sig-other"), "{msg}");
    }

    fn sample_skill_candidate(id: &str) -> crate::commands_skill_candidates::SkillCandidate {
        crate::commands_skill_candidates::SkillCandidate {
            id: id.into(),
            detected_at: "2026-09-25T00:00:00Z".into(),
            occurrence_count: 2,
            example_session_ids: vec![],
            proposed_name: "bash".into(),
            proposed_trigger: "recurring bash(cmd) calls".into(),
            procedure: vec!["invoke bash".into()],
            source_tool_calls: vec![],
            refined: false,
        }
    }
}

#[cfg(test)]
mod auto_title_tests {
    use super::derive_title_from_message;

    #[test]
    fn short_message_passes_through() {
        assert_eq!(
            derive_title_from_message("Fix the login bug"),
            "Fix the login bug"
        );
    }

    #[test]
    fn long_message_truncates_to_50_chars_plus_ellipsis() {
        let long = "a".repeat(80);
        let title = derive_title_from_message(&long);
        assert_eq!(title.chars().count(), 51);
        assert!(title.ends_with('…'));
        assert_eq!(&title[..50], "a".repeat(50));
    }

    #[test]
    fn truncation_counts_chars_not_bytes() {
        // 60 CJK chars = 180 UTF-8 bytes; byte-slicing would panic or mojibake.
        let zh = "配".repeat(60);
        let title = derive_title_from_message(&zh);
        assert_eq!(title.chars().count(), 51);
        assert!(title.ends_with('…'));
    }

    #[test]
    fn exactly_50_chars_is_not_truncated() {
        let exact = "b".repeat(50);
        assert_eq!(derive_title_from_message(&exact), exact);
    }

    #[test]
    fn uses_first_line_only() {
        assert_eq!(
            derive_title_from_message("first line\nsecond line\nthird"),
            "first line"
        );
    }

    #[test]
    fn whitespace_only_yields_empty() {
        assert_eq!(derive_title_from_message("   \n\t  "), "");
    }
}
