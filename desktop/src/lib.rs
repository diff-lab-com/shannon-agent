// Suppress lints that conflict with rustfmt or are style preferences from newer clippy.
#![allow(
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::derivable_impls
)]

use std::path::{Path, PathBuf};

pub mod agent_teams;
pub mod config;
/// Local file logging, log retention, and the panic hook (audit batch 3, B).
pub mod desktop_logging;
pub mod events;
pub mod extensions;
pub mod file_permissions;
pub mod mcp;
pub mod provider_read_snapshot;
pub mod routine_templates;
/// Unified 0600 atomic writer for plaintext-secret-bearing files (R6).
pub mod secret_files;
/// OS-keyring credential storage behind the `SecretStore` trait seam
/// (R7-④ batch 2 / A8): MCP OAuth tokens + data-source credentials, mock
/// store for tests, never-silent plaintext fallback.
pub mod secret_store;
/// G1 P0-2.1 — skill → chat-tool bridge (installed skills become
/// model-callable `skill_<id>` tools).
pub mod skill_tools;

/// Typed failure of a working-dir scope check (P0-3).
///
/// The string-typed [`resolve_path_in_working_dir`] predates it and stays as
/// the thin wrapper most callers keep using. Callers that must TELL THE USER
/// why a path was rejected (the attachment pipeline) match on the variant
/// instead of parsing the `Display` string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkingDirScopeError {
    /// The path (or its symlink target) does not exist or cannot be
    /// canonicalized.
    Unresolvable(String),
    /// The configured working directory itself cannot be canonicalized.
    InvalidWorkingDir(String),
    /// The path resolves outside the working directory — the deliberate
    /// anti-exfiltration boundary, never widened by call sites.
    OutsideWorkingDir(String),
}

impl std::fmt::Display for WorkingDirScopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Keep the exact legacy strings — tests and user-visible errors
            // are pinned to them.
            WorkingDirScopeError::Unresolvable(e) => write!(f, "path not found: {e}"),
            WorkingDirScopeError::InvalidWorkingDir(e) => {
                write!(f, "invalid working directory: {e}")
            }
            WorkingDirScopeError::OutsideWorkingDir(p) => {
                write!(f, "path '{p}' is outside the working directory")
            }
        }
    }
}

/// The pasted-image cache directory (`$SHANNON_HOME/cache/pasted/`, default
/// `~/.shannon/cache/pasted/`) — the single narrow exception to the
/// working-dir attachment boundary, shared by `save_pasted_image` (writer)
/// and [`classify_path_in_working_dir`] (reader gate) so the two can never
/// disagree about where pasted images live.
pub(crate) fn pasted_image_cache_dir() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("SHANNON_HOME") {
        return Some(PathBuf::from(home).join("cache").join("pasted"));
    }
    dirs::home_dir().map(|home| home.join(".shannon").join("cache").join("pasted"))
}

/// Whether the canonicalized `path` sits inside the pasted-image cache
/// directory. Both sides are canonicalized (symlinks resolved), and
/// `Path::starts_with` compares whole components — so a sibling like
/// `cache/pasted-evil` or a planted symlink pointing elsewhere can never
/// match.
fn is_in_pasted_image_cache(canonical: &Path) -> bool {
    pasted_image_cache_dir()
        .and_then(|dir| dir.canonicalize().ok())
        .is_some_and(|dir| canonical.starts_with(dir))
}

/// Typed variant of [`resolve_path_in_working_dir`] — same resolution and
/// the same security boundary, but the caller can classify the rejection.
///
/// G3b P1-6 fix — one NARROW exception to the boundary: files under
/// `$SHANNON_HOME/cache/pasted/` classify as in-scope even though they are
/// outside the working directory. This is safe by construction, not a
/// widened hole:
///   1. That directory's contents are written ONLY by the backend's
///      `save_pasted_image` from the user's own clipboard — bytes the
///      webview already holds — so allowing the attachment pipeline to read
///      them back grants no access the frontend did not already have
///      (no arbitrary-path read escalation).
///   2. The check runs on the fully CANONICALIZED path (symlinks resolved):
///      a symlink planted in the cache pointing at `~/.ssh/id_rsa` resolves
///      outside and is still refused.
///   3. The allow-domain is exactly this one directory — sibling caches
///      (`cache/extracted/…`) and every other `$SHANNON_HOME` path remain
///      outside and refused.
///
/// Both the `check_attachment_paths` preflight and the `send_message` gate
/// go through this one classifier, so they stay in agreement automatically.
pub(crate) fn classify_path_in_working_dir(
    path: &str,
    working_dir: &Path,
) -> Result<PathBuf, WorkingDirScopeError> {
    let resolved = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        working_dir.join(path)
    };
    // `canonicalize` resolves `..`, symlinks, and case-insensitive roots.
    // We require the target to exist so callers get a meaningful "not found"
    // error before any write attempt; commands that need to create new files
    // should canonicalize the parent directory instead.
    let canonical = resolved
        .canonicalize()
        .map_err(|e| WorkingDirScopeError::Unresolvable(format!("{e}")))?;
    // Pasted-image cache exception FIRST: it is in scope regardless of the
    // configured working directory (see the doc comment for why this is
    // safe and narrow).
    if is_in_pasted_image_cache(&canonical) {
        return Ok(canonical);
    }
    let canonical_cwd = working_dir
        .canonicalize()
        .map_err(|e| WorkingDirScopeError::InvalidWorkingDir(format!("{e}")))?;
    if !canonical.starts_with(&canonical_cwd) {
        return Err(WorkingDirScopeError::OutsideWorkingDir(
            canonical.display().to_string(),
        ));
    }
    Ok(canonical)
}

/// Resolve `path` relative to `working_dir` (or use it as-is if absolute),
/// then canonicalize both and ensure the resolved path is inside the working
/// directory. Rejects path traversal (`..`), absolute paths outside the
/// working dir, and symlinks that escape the working dir.
///
/// Returns the canonicalized path on success. The helper is fallible by
/// design: callers translate the `Err(String)` into their own error type.
///
/// Used to harden IPC commands that accept user-supplied file paths — a
/// compromised frontend must not be able to read/write arbitrary files
/// (e.g. `~/.ssh/id_rsa`, `~/.shannon/desktop/config.json`).
pub(crate) fn resolve_path_in_working_dir(
    path: &str,
    working_dir: &Path,
) -> Result<PathBuf, String> {
    classify_path_in_working_dir(path, working_dir).map_err(|e| e.to_string())
}

/// Validate that `path` would write inside `working_dir`, allowing
/// intermediate directories that don't yet exist (they will be created by
/// the caller). Walks up to the first ancestor that exists, canonicalizes
/// that ancestor, and verifies it lives inside `working_dir`. Returns the
/// joined absolute target path with the original file name preserved.
pub(crate) fn resolve_write_target_in_working_dir(
    path: &str,
    working_dir: &Path,
) -> Result<PathBuf, String> {
    let resolved = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        working_dir.join(path)
    };
    let canonical_cwd = working_dir
        .canonicalize()
        .map_err(|e| format!("invalid working directory: {e}"))?;
    // Walk up to the first ancestor that exists, canonicalize it, and verify
    // it is inside the working directory. This allows intermediate dirs to
    // not yet exist (caller will `create_dir_all`) while still guaranteeing
    // no escape via `..` or absolute paths.
    let mut probe = resolved.clone();
    let canonical_anchor = loop {
        match probe.canonicalize() {
            Ok(p) => break p,
            Err(_) => {
                if !probe.pop() {
                    return Err(format!(
                        "path '{}' has no existing ancestor inside the working directory",
                        resolved.display()
                    ));
                }
            }
        }
    };
    if !canonical_anchor.starts_with(&canonical_cwd) {
        return Err(format!(
            "path '{}' is outside the working directory",
            resolved.display()
        ));
    }
    Ok(resolved)
}

#[cfg(feature = "tauri")]
pub mod agent_message_watcher;
pub mod commands;

/// Office Wave A2' — docx/pptx/xlsx/ods/csv text extraction, guard rails,
/// extracted-text cache and the send_message injection-block builder. No
/// Tauri dependency; every failure is a `Result`, never a panic.
pub mod document_parse;

#[cfg(feature = "tauri")]
pub mod commands_agents;

#[cfg(feature = "tauri")]
pub mod commands_config;
#[cfg(feature = "tauri")]
pub mod commands_connections;
/// Export-diagnostics: local logs + crash reports + a fresh doctor report
/// bundled into one zip (batch-3 follow-up). Pure bundle logic lives here
/// and is unit-tested without Tauri.
#[cfg(feature = "tauri")]
pub mod commands_diagnostics;
#[cfg(feature = "tauri")]
pub mod commands_mobile_pairing;
pub mod commands_remote;
/// Desktop approval entry for IM pairing requests (T9) — talks to the running
/// gateway's mobile listener over its pairing-access HTTP RPC.
#[cfg(feature = "tauri")]
pub mod gateway_pairing;
#[cfg(feature = "tauri")]
pub mod gateway_supervisor;

#[cfg(feature = "tauri")]
pub mod gateway_service_probe;

#[cfg(feature = "tauri")]
pub mod loopback_api;

#[cfg(feature = "tauri")]
pub mod engine_discovery;

#[cfg(feature = "tauri")]
pub mod engine_discovery_commands;

// ADR-0011 Phase B B3/B7 — surface identity + in-app CLI installation.
#[cfg(feature = "tauri")]
pub mod commands_surface;

// 2026-09-26 round2 §5-1 A — `artifact://` custom protocol: interactive
// HTML artifacts served from a registry with per-response strict CSP.
#[cfg(feature = "tauri")]
pub mod commands_artifact;

#[cfg(feature = "tauri")]
pub mod commands_memory;

#[cfg(feature = "tauri")]
pub mod commands_chat;

/// R3-3 — Plan/Act dual-tier model preference: pure tier-preference +
/// resolution logic (no Tauri dependency; unit-tested standalone). See the
/// module docs for the precedence contract.
pub mod phase_tier;

#[cfg(feature = "tauri")]
pub mod commands_mcp;

#[cfg(feature = "tauri")]
/// R3-2 (desktop slice) — provider model-profile list/switch/create.
pub mod commands_profiles;

#[cfg(feature = "tauri")]
/// R4-3 (desktop slice) — per-provider multi-key management (list/add/
/// remove/activate against the engine credential manager).
pub mod commands_keys;

#[cfg(feature = "tauri")]
/// S2-1 (模型仓固化) — curated per-provider model vault (fetch 结果固化进
/// providers.toml v2 的 `models` 列表). See the module docs.
pub mod commands_models;

#[cfg(feature = "tauri")]
/// S3-3 (utility tier 槽位化) — the first consumption chain for the
/// providers.toml v2 `auxiliary` map: compaction + session-summary slots,
/// strictly orthogonal to the interactive precedence chain (裁定⑦). See the
/// module docs for the honest per-slot consumption verdict.
pub mod utility_tier;

#[cfg(feature = "tauri")]
pub mod commands_notifications;

#[cfg(feature = "tauri")]
pub mod commands_files;

#[cfg(feature = "tauri")]
pub mod commands_onboarding;

pub mod commands_feedback;
#[cfg(feature = "tauri")]
pub mod commands_permissions;
pub mod commands_rewind;
pub mod commands_slash;

#[cfg(feature = "tauri")]
pub mod commands_plugins;

// X5 — plugin package materialization into the per-type extension homes
// (skills/agents/commands/mcp), with the `materialized.json` sidecar that
// drives reverse-materialization on uninstall/disable/update.
pub mod plugin_materialize;

// P-E3 — project registry commands (`~/.shannon/projects.db`).
#[cfg(feature = "tauri")]
pub mod commands_projects;

#[cfg(feature = "tauri")]
pub mod commands_sessions;

#[cfg(feature = "tauri")]
pub mod commands_tasks;

#[cfg(feature = "tauri")]
pub mod session_registry;

// R5-1 — durable sidecar for session-level model overrides (restart survive).
#[cfg(feature = "tauri")]
pub mod session_override_store;

// P2-5 — durable sidecar for the session-level "temporary chat" flag.
#[cfg(feature = "tauri")]
pub mod session_memory_bypass;

// P1-1 — session multi-window commands/registry/restore.
#[cfg(feature = "tauri")]
pub mod session_window_commands;

// Office Wave 3 C3 — companion Quick Capture window (open/toggle commands).
#[cfg(feature = "tauri")]
pub mod companion_window_commands;

#[cfg(feature = "tauri")]
pub mod commands_usage;

/// P2-1/P2-6 — usage governance (monthly budget + 80/100% threshold alerts)
/// and pre-task cost estimation from routine-run history.
#[cfg(feature = "tauri")]
pub mod usage_governance;

#[cfg(feature = "tauri")]
pub mod commands_voice;

#[cfg(feature = "tauri")]
pub mod commands_voice_models;

#[cfg(feature = "tauri")]
pub mod scheduled_commands;

/// P0-3 — SQLite inbox commands + shared routine-run executor.
#[cfg(feature = "tauri")]
pub mod inbox_commands;

/// T5 — inbox write/resolve seam for the unified "needs attention" stream
/// (session approvals / session failures / skill candidates).
#[cfg(feature = "tauri")]
pub mod inbox_session_events;

/// P0-2 — desktop goal runner + Tasks-page run-card commands.
#[cfg(feature = "tauri")]
pub mod goal_commands;

/// G3b fix round 1 (C1) — tests for the pasted-image cache allow-domain in
/// [`classify_path_in_working_dir`].
#[cfg(test)]
mod tests {
    use super::*;

    /// These three tests all redirect `SHANNON_HOME`; cargo runs them in
    /// parallel threads of one process, so they serialize on this lock (a
    /// module-local version of the repo-wide SHANNON_HOME test convention —
    /// cross-module serialization with the document_parse/commands tests is
    /// the recorded backlog item, untouched here).
    static SHANNON_HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Redirect `SHANNON_HOME` at a tempdir for the duration of `f` and
    /// restore it afterwards (same save/restore pattern as the
    /// commands.rs / document_parse tests). The pasted-image allow-domain is
    /// derived from this variable, so the tests control it end to end.
    fn with_temp_shannon_home<T>(f: impl FnOnce(&Path) -> T) -> T {
        let _guard = SHANNON_HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().expect("shannon home tempdir");
        let prev = std::env::var("SHANNON_HOME").ok();
        unsafe { std::env::set_var("SHANNON_HOME", home.path()) };
        let out = f(home.path());
        match prev {
            Some(p) => unsafe { std::env::set_var("SHANNON_HOME", p) },
            None => unsafe { std::env::remove_var("SHANNON_HOME") },
        }
        out
    }

    /// G3b fix round 1 (C1) — a file inside `$SHANNON_HOME/cache/pasted/`
    /// classifies as in-scope even with a completely different working
    /// directory configured: this is what makes a pasted image usable
    /// end-to-end (preflight chip + send gate go through this classifier).
    #[test]
    fn pasted_image_cache_is_in_scope_outside_the_working_dir() {
        with_temp_shannon_home(|home| {
            let pasted = home.join("cache").join("pasted");
            std::fs::create_dir_all(&pasted).expect("mkdir pasted");
            let img = pasted.join("1730000000-deadbeef.png");
            std::fs::write(&img, b"png").expect("write image");

            let workdir = tempfile::tempdir().expect("workdir");
            let got = classify_path_in_working_dir(&img.to_string_lossy(), workdir.path())
                .expect("pasted cache path must classify in-scope");
            assert_eq!(got, img.canonicalize().expect("canonical"));
        });
    }

    /// The allow-domain is EXACTLY `cache/pasted`: the sibling extracted-text
    /// cache, sibling directories (even `pasted-evil`), the cache root itself
    /// and any other `$SHANNON_HOME` path stay outside the boundary.
    #[test]
    fn allow_domain_is_exactly_cache_pasted() {
        with_temp_shannon_home(|home| {
            let extracted = home.join("cache").join("extracted");
            std::fs::create_dir_all(&extracted).expect("mkdir extracted");
            let txt = extracted.join("abc.txt");
            std::fs::write(&txt, b"text").expect("write");
            let settings = home.join("settings.json");
            std::fs::write(&settings, b"{}").expect("write");
            let sibling = home.join("cache").join("pasted-evil");
            std::fs::create_dir_all(&sibling).expect("mkdir sibling");
            let evil = sibling.join("x.png");
            std::fs::write(&evil, b"png").expect("write");
            let loose = home.join("cache").join("loose.png");
            std::fs::write(&loose, b"png").expect("write");

            let workdir = tempfile::tempdir().expect("workdir");
            for path in [&txt, &settings, &evil, &loose] {
                let err = classify_path_in_working_dir(&path.to_string_lossy(), workdir.path())
                    .expect_err("outside the narrow pasted domain must stay refused");
                assert!(
                    matches!(err, WorkingDirScopeError::OutsideWorkingDir(_)),
                    "{path:?} -> {err:?}"
                );
            }
        });
    }

    /// The check runs on the CANONICALIZED path: a symlink planted inside
    /// the pasted cache that points at a file outside the domain resolves to
    /// its target and stays refused (no planted-symlink read escalation).
    #[cfg(unix)]
    #[test]
    fn symlink_inside_pasted_cache_escaping_outside_is_refused() {
        with_temp_shannon_home(|home| {
            let pasted = home.join("cache").join("pasted");
            std::fs::create_dir_all(&pasted).expect("mkdir pasted");
            let secret_dir = tempfile::tempdir().expect("secret dir");
            let secret = secret_dir.path().join("id_rsa");
            std::fs::write(&secret, b"PRIVATE KEY").expect("write secret");
            let link = pasted.join("innocent.png");
            std::os::unix::fs::symlink(&secret, &link).expect("plant symlink");

            let workdir = tempfile::tempdir().expect("workdir");
            let err = classify_path_in_working_dir(&link.to_string_lossy(), workdir.path())
                .expect_err("symlink escape must stay refused");
            assert!(
                matches!(err, WorkingDirScopeError::OutsideWorkingDir(_)),
                "{err:?}"
            );
        });
    }
}

/// P1-2 — desktop best-of-N batch runs: parallel worktree orchestration
/// commands (start/list/diff/adopt/discard) + the `batch:updated` source.
#[cfg(feature = "tauri")]
pub mod batch_commands;

/// P1-5 C-1 — dev-server preview: detect/start/stop/status/capture
/// commands + the `PreviewManager` lifecycle owner (also backs the
/// desktop-only `preview_screenshot` engine tool).
#[cfg(feature = "tauri")]
pub mod preview_commands;

/// P1-5 D — integrated terminal: long-lived PTY sessions (≤4, owned by
/// AppState, killed on exit), the frozen `terminal_*` command contract and
/// the throttled `terminal:output` event.
#[cfg(feature = "tauri")]
pub mod terminal_commands;

/// P0-4 — cost observability: session budget, context breakdown and
/// per-session usage aggregation commands.
#[cfg(feature = "tauri")]
pub mod cost_commands;

/// P1-6 — migration wizard: scan / preview / apply imports from a Claude
/// Code or ZCode install (settings rules, MCP servers, skills, commands,
/// project memory). Read-scan + user-approved import only; known source
/// paths, never arbitrary ones.
#[cfg(feature = "tauri")]
pub mod migration_commands;

/// P2-2 — persona/profile pack: export & import Shannon's personalization
/// surfaces (skills, commands, memories, routines, profiles, persona) as a
/// single secret-stripped `.tar.gz` with path-safe, idempotent imports.
#[cfg(feature = "tauri")]
pub mod persona_pack_commands;

#[cfg(feature = "tauri")]
pub mod commands_routine_templates;

#[cfg(feature = "tauri")]
pub mod lsp_commands;

#[cfg(feature = "tauri")]
pub mod automation_commands;

#[cfg(feature = "tauri")]
pub mod sandbox_assembly;

#[cfg(feature = "tauri")]
pub mod extensions_commands;

#[cfg(feature = "tauri")]
pub mod commands_skill_loop;

#[cfg(feature = "tauri")]
pub mod commands_skill_candidates;

#[cfg(feature = "tauri")]
pub mod skill_pattern_detection;

#[cfg(feature = "tauri")]
pub mod commands_dream;

#[cfg(feature = "tauri")]
pub mod notifications;
