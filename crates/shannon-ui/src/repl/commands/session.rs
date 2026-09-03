//! Session management command handlers

use crate::{Result, widgets::ChatRole};

use super::super::Repl;

use shannon_core::signals::RewindKind;
use shannon_tools::{FileHistoryConfig, FileHistoryManager, FileSnapshot, RewindAction};
use std::path::{Path, PathBuf};

pub(crate) fn handle_sessions(repl: &mut Repl, _args: &str) -> Result<()> {
    // /sessions has been removed in favour of /resume (the picker).
    // Stub retained temporarily so existing muscle memory gets a clear pointer.
    repl.chat.add_message(
        ChatRole::System,
        "/sessions has been removed. Use /resume to open the picker, or /resume <uuid> for a specific session.".to_string(),
    );
    Ok(())
}

pub(crate) fn handle_resume(repl: &mut Repl, args: &str) -> Result<()> {
    let arg = args.trim();
    if arg.is_empty() {
        // /resume with no args → open the interactive session picker.
        return repl.open_session_picker();
    }

    let session_id = if let Ok(uuid) = uuid::Uuid::parse_str(arg) {
        uuid
    } else if let Ok(num) = arg.parse::<usize>() {
        if num == 0 || num > repl.last_session_list.len() {
            repl.chat.add_message(
                ChatRole::System,
                format!("Invalid session number: {num}. Open /resume to see available sessions."),
            );
            return Ok(());
        }
        repl.last_session_list[num - 1].session_id
    } else {
        repl.chat.add_message(
            ChatRole::System,
            format!(
                "Invalid session identifier: {arg}. Open /resume to pick, or use /resume <uuid>."
            ),
        );
        return Ok(());
    };

    match repl.l0_store().load(&session_id) {
        Ok(Some(data)) => {
            repl.chat.clear();
            let title = data.metadata.title.as_deref().unwrap_or("Untitled");
            let msg_count = data.messages.len();

            repl.chat.add_message(
                ChatRole::System,
                format!(
                    "Resumed session: \"{}\" ({} messages, model: {})\nCreated: {} | Updated: {}",
                    title,
                    msg_count,
                    data.metadata.model,
                    data.metadata.created_at.format("%Y-%m-%d %H:%M"),
                    data.metadata.updated_at.format("%Y-%m-%d %H:%M"),
                ),
            );

            for msg in &data.messages {
                let role = match msg.role.as_str() {
                    "user" => ChatRole::User,
                    "assistant" => ChatRole::Assistant,
                    _ => ChatRole::System,
                };
                let content = super::super::session::render_message_content(&msg.content);
                if !content.trim().is_empty() {
                    repl.chat.add_message(role, content);
                }
            }

            if !data.metadata.model.is_empty() {
                repl.state.model = Some(data.metadata.model.clone());
            }
            repl.state.tokens_used =
                data.metadata.total_input_tokens + data.metadata.total_output_tokens;

            if let Some(ref mut engine) = repl.query_engine {
                match engine.restore_session(session_id) {
                    Ok(true) => {
                        tracing::info!(session_id = %session_id, "QueryEngine conversation restored");
                    }
                    Ok(false) => {
                        tracing::warn!(session_id = %session_id, "No persisted session data for QueryEngine restore");
                    }
                    Err(e) => {
                        tracing::warn!(session_id = %session_id, error = %e, "Failed to restore QueryEngine session");
                        repl.chat.add_message(ChatRole::System, format!("Warning: could not restore AI context (messages will lack prior history): {e}"));
                    }
                }
            }
        }
        Ok(None) => {
            super::set_error(repl, &format!("session not found: {session_id}"));
        }
        Err(e) => {
            super::set_error(repl, &format!("loading session: {e}"));
        }
    }

    Ok(())
}

pub(crate) fn handle_branch(repl: &mut Repl, args: &str) -> Result<()> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        repl.chat.add_message(
            ChatRole::System,
            "Usage: /branch <session-id-or-number> [message-index]\nOpen /resume to see available sessions.".to_string(),
        );
        return Ok(());
    }

    // Resolve session ID
    let session_id = if let Ok(uuid) = uuid::Uuid::parse_str(parts[0]) {
        uuid
    } else if let Ok(num) = parts[0].parse::<usize>() {
        if num == 0 || num > repl.last_session_list.len() {
            repl.chat.add_message(
                ChatRole::System,
                format!("Invalid session number: {num}. Open /resume to see available sessions."),
            );
            return Ok(());
        }
        repl.last_session_list[num - 1].session_id
    } else {
        repl.chat.add_message(
            ChatRole::System,
            format!(
                "Invalid session identifier: {}. Open /resume to pick, or use a UUID.",
                parts[0]
            ),
        );
        return Ok(());
    };

    // Load parent to get message count for default branch point
    let parent_data = match repl.l0_store().load(&session_id) {
        Ok(Some(data)) => data,
        Ok(None) => {
            repl.chat
                .add_message(ChatRole::System, format!("Session not found: {session_id}"));
            return Ok(());
        }
        Err(e) => {
            super::set_error(repl, &format!("loading session for branch: {e}"));
            return Ok(());
        }
    };

    let total_messages = parent_data.messages.len();

    // Parse optional branch point (defaults to end of conversation)
    let branch_point = if parts.len() > 1 {
        match parts[1].parse::<usize>() {
            Ok(idx) if idx <= total_messages => idx,
            Ok(idx) => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!(
                        "Branch point {idx} is out of range. Session has {total_messages} messages."
                    ),
                );
                return Ok(());
            }
            Err(_) => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Invalid branch point: {}. Must be a number.", parts[1]),
                );
                return Ok(());
            }
        }
    } else {
        total_messages
    };

    // Create the branch
    match repl
        .l0_store()
        .create_branch(&session_id, branch_point, None)
    {
        Ok(branch_data) => {
            let title = parent_data.metadata.title.as_deref().unwrap_or("Untitled");
            let branch_id = branch_data.session_id;
            repl.chat.add_message(
                ChatRole::System,
                format!(
                    "Created branch from \"{}\" at message {}/{}\nNew session: {branch_id}\nMessages copied: {}\nUse /resume {branch_id} to work on this branch",
                    title, branch_point, total_messages, branch_data.messages.len(),
                ),
            );
        }
        Err(e) => {
            super::set_error(repl, &format!("creating branch: {e}"));
        }
    }

    Ok(())
}

pub(crate) fn handle_history(repl: &mut Repl, args: &str) -> Result<()> {
    let arg = args.trim();

    if let Some(rest) = arg.strip_prefix("--export") {
        let export_path = rest.trim();
        if export_path.is_empty() {
            repl.chat.add_message(
                ChatRole::System,
                "Usage: /history --export <file-path>".to_string(),
            );
            return Ok(());
        }

        let mut md = String::from("# Shannon Session Export\n\n");
        for i in 0..repl.chat.len() {
            if let Some(msg) = repl.chat.get_message(i) {
                let role = match msg.role {
                    ChatRole::User => "## User",
                    ChatRole::Assistant => "## Assistant",
                    ChatRole::System => "## System",
                    ChatRole::Tool => "## Tool",
                };
                md.push_str(&format!("{}\n\n{}\n\n---\n\n", role, msg.content));
            }
        }

        match std::fs::write(export_path, md) {
            Ok(_) => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Session exported to: {export_path}"),
                );
            }
            Err(e) => {
                super::set_error(repl, &format!("exporting session: {e}"));
            }
        };
        return Ok(());
    }

    let msg_count = repl.chat.len();
    let mut user_count = 0;
    let mut assistant_count = 0;
    for i in 0..repl.chat.len() {
        if let Some(msg) = repl.chat.get_message(i) {
            match msg.role {
                ChatRole::User => user_count += 1,
                ChatRole::Assistant => assistant_count += 1,
                ChatRole::System | ChatRole::Tool => {}
            }
        }
    }

    let tokens = repl.state.tokens_used;
    let model = repl.state.model.as_deref().unwrap_or("default");

    let mut stats = format!(
        "Current session stats:\n  Messages: {} total ({} user, {} assistant)\n  Tokens used: {} ({:.1}k)\n  Model: {}\n  Working dir: {}\n  Commands run: {}\n  Tools invoked: {}",
        msg_count,
        user_count,
        assistant_count,
        tokens,
        tokens as f64 / 1000.0,
        model,
        repl.state.working_directory,
        repl.commands_run,
        repl.tools_invoked,
    );

    if let Some(started) = &repl.session_started_at {
        let elapsed = chrono::Utc::now() - *started;
        let mins = elapsed.num_minutes();
        let secs = elapsed.num_seconds() % 60;
        stats.push_str(&format!("\n  Session duration: {mins}m {secs}s"));
    }

    if repl.diff_data.total_files_modified() > 0
        || repl.diff_data.total_files_created() > 0
        || repl.diff_data.total_files_deleted() > 0
    {
        stats.push_str(&format!(
            "\n  Files: +{}/-{}/{} modified, {} created, {} deleted",
            repl.diff_data.total_additions(),
            repl.diff_data.total_deletions(),
            repl.diff_data.total_files_modified(),
            repl.diff_data.total_files_created(),
            repl.diff_data.total_files_deleted(),
        ));
    }

    repl.chat.add_message(ChatRole::System, stats);
    Ok(())
}

pub(crate) fn handle_undo(repl: &mut Repl, args: &str) -> Result<()> {
    // `/undo` is an alias of `/rewind` (W6-2 B.1 command unification). The
    // git-checkpoint preview/confirm flow that used to live here is reachable
    // via `/rewind code <n>` / `/rewind both <n>`; per-file content-snapshot
    // revert is `/rewind <path>`.
    handle_rewind(repl, args)
}

/// What a `/rewind` (or `/undo` / `/checkpoint` alias) invocation intends to do.
#[derive(Debug)]
enum RewindIntent {
    /// Show the turn-checkpoint history list.
    History,
    /// Revert file changes to their state at turn checkpoint `index`
    /// (B.2: driven by `FileHistoryManager` turn-tagged content snapshots,
    /// not git).
    Code(usize),
    /// Revert file changes and rewind the conversation to turn checkpoint `index`.
    Both(usize),
    /// Rewind the conversation by `turns` turns.
    Conversation(usize),
    /// Per-file revert: restore `path` to its most recent content snapshot.
    /// `skip_confirm` is set by `--yes`.
    File { path: String, skip_confirm: bool },
}

/// Parse `/rewind` arguments into a [`RewindIntent`].
///
/// Disambiguation: a bare number is always a conversation-turn count (never a
/// file), and any other token — optionally followed by `--yes` — is treated as
/// a file path. Keyword subcommands (`history`, `code <n>`, `both <n>`) match
/// first.
fn parse_rewind_intent(args: &str) -> RewindIntent {
    let trimmed = args.trim();

    if trimmed.is_empty() {
        return RewindIntent::Conversation(1);
    }

    if matches!(trimmed, "history" | "list" | "ls") {
        return RewindIntent::History;
    }

    if let Some(rest) = trimmed.strip_prefix("code ") {
        if let Ok(n) = rest.trim().parse::<usize>() {
            return RewindIntent::Code(n);
        }
    }
    if let Some(rest) = trimmed.strip_prefix("both ") {
        if let Ok(n) = rest.trim().parse::<usize>() {
            return RewindIntent::Both(n);
        }
    }

    let (positional, skip_confirm) = strip_trailing_yes(trimmed);
    if positional.is_empty() {
        // Only `--yes` was provided — no meaningful target; default to 1 turn.
        return RewindIntent::Conversation(1);
    }
    if let Ok(n) = positional.parse::<usize>() {
        return RewindIntent::Conversation(n);
    }
    RewindIntent::File {
        path: positional,
        skip_confirm,
    }
}

/// Split a trailing `--yes` flag off `s`. Returns the remaining positional text
/// and whether `--yes` was present.
fn strip_trailing_yes(s: &str) -> (String, bool) {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_suffix("--yes") {
        (rest.trim().to_string(), true)
    } else {
        (trimmed.to_string(), false)
    }
}

/// Resolve a user-typed path against the set of file-history-tracked files.
///
/// Matching is tried in order: exact string, canonicalized absolute, then
/// suffix (so a relative `src/foo.rs` matches a tracked `/abs/.../src/foo.rs`
/// and vice-versa). Returns the tracked key form — what the manager stores
/// snapshots under.
fn resolve_tracked_path(tracked: &[PathBuf], user: &str) -> Option<PathBuf> {
    let user_path = PathBuf::from(user);

    if let Some(t) = tracked.iter().find(|t| *t == &user_path) {
        return Some(t.clone());
    }

    let canon = canonicalish(&user_path);
    for t in tracked {
        if canonicalish(t) == canon {
            return Some(t.clone());
        }
    }

    for t in tracked {
        if t.ends_with(&user_path) || user_path.ends_with(t) {
            return Some(t.clone());
        }
    }

    None
}

/// Best-effort absolute form of `p`: canonicalize if it exists, else absolutize
/// relative to the current directory without resolving symlinks.
fn canonicalish(p: &Path) -> PathBuf {
    if let Ok(c) = p.canonicalize() {
        return c;
    }
    if p.is_absolute() {
        return p.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(p))
        .unwrap_or_else(|_| p.to_path_buf())
}

/// Choose which snapshot to restore for a per-file rewind: the most recent
/// snapshot whose content differs from the current on-disk content. A restore
/// records itself as a new snapshot, so this skip lets repeated `/rewind <path>`
/// calls walk backwards through real versions.
fn select_restore_snapshot<'a>(
    snapshots: &'a [FileSnapshot],
    current: &str,
) -> Option<&'a FileSnapshot> {
    snapshots.iter().rev().find(|s| s.content != current)
}

/// Core per-file restore: roll `path` back to snapshot `id` via the manager and
/// persist the restored content to disk. The manager records the restore itself
/// as a new snapshot (so it can be undone by rewinding again).
fn restore_file_snapshot(
    mgr: &mut FileHistoryManager,
    path: &Path,
    id: &str,
) -> std::result::Result<String, String> {
    // `restore` writes through the manager's filesystem world, so a remote
    // session restores the file on the target, not on the local disk.
    mgr.restore(path, id).map_err(|e| e.to_string())
}

/// Per-file rewind used by both the `--yes` fast path and the confirm-dialog
/// handler. Prefers the registry's provider-wired manager (same snapshots the
/// file tools recorded, same execution world); falls back to building one
/// from the file-history env config.
pub(crate) fn apply_file_rewind(
    history: Option<&std::sync::Arc<std::sync::Mutex<FileHistoryManager>>>,
    path: &Path,
    id: &str,
) -> std::result::Result<String, String> {
    match history {
        Some(shared) => {
            let mut mgr = shared.lock().map_err(|p| format!("history lock: {p}"))?;
            restore_file_snapshot(&mut mgr, path, id)
        }
        None => {
            let cfg = FileHistoryConfig::from_env().unwrap_or_default();
            let mut mgr = FileHistoryManager::new(cfg);
            restore_file_snapshot(&mut mgr, path, id)
        }
    }
}

/// Drive a per-file rewind: resolve the path, pick the snapshot, and either
/// restore immediately (`skip_confirm`) or raise a confirm dialog. Failures are
/// reported as system chat messages; this always returns `Ok(())`.
fn run_file_rewind(repl: &mut Repl, raw_path: &str, skip_confirm: bool) -> Result<()> {
    // Reuse the registry's provider-wired manager when available so listing
    // and restoring see exactly what the file tools recorded.
    #[allow(unused_assignments)] // initializer keeps the bindings total
    let mut owned: Option<FileHistoryManager> = None;
    #[allow(unused_assignments)]
    let mut shared_guard: Option<std::sync::MutexGuard<'_, FileHistoryManager>> = None;
    let mgr: &mut FileHistoryManager = match repl.file_history.as_ref() {
        Some(shared) => match shared.lock() {
            Ok(g) => {
                shared_guard = Some(g);
                shared_guard.as_mut().expect("just stored")
            }
            Err(p) => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("File rewind unavailable: history lock ({p})."),
                );
                return Ok(());
            }
        },
        None => {
            let cfg = FileHistoryConfig::from_env().unwrap_or_default();
            owned = Some(FileHistoryManager::new(cfg));
            owned.as_mut().expect("just built")
        }
    };

    let tracked = match mgr.list_tracked_files() {
        Ok(t) => t,
        Err(e) => {
            repl.chat.add_message(
                ChatRole::System,
                format!("File rewind unavailable: could not read history ({e})."),
            );
            return Ok(());
        }
    };

    let Some(path) = resolve_tracked_path(&tracked, raw_path) else {
        repl.chat.add_message(
            ChatRole::System,
            format!(
                "No file history for `{raw_path}`. Snapshots are recorded before AI file edits — \
                 edit the file first or check the path.",
            ),
        );
        return Ok(());
    };

    let history = match mgr.get_history(&path) {
        Ok(h) => h,
        Err(e) => {
            repl.chat
                .add_message(ChatRole::System, format!("Could not read history: {e}."));
            return Ok(());
        }
    };
    if history.snapshots.is_empty() {
        repl.chat.add_message(
            ChatRole::System,
            format!("No snapshots recorded for `{}`.", path.display()),
        );
        return Ok(());
    }

    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let Some(target) = select_restore_snapshot(&history.snapshots, &current) else {
        repl.chat.add_message(
            ChatRole::System,
            format!(
                "`{}` is already at its earliest recorded version — nothing to rewind.",
                path.display()
            ),
        );
        return Ok(());
    };

    let id = target.id.clone();
    let restored_lines = target.content.lines().count();
    let current_lines = current.lines().count();
    let time = target.timestamp.format("%H:%M:%S");
    let op = format!("{:?}", target.operation).to_lowercase();
    let short_id = &id[..id.len().min(8)];

    if skip_confirm {
        match apply_file_rewind(repl.file_history.as_ref(), &path, &id) {
            Ok(_) => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!(
                        "Rewound `{}` to snapshot `{short_id}` ({op} @ {time}): {current_lines} → {restored_lines} lines.",
                        path.display()
                    ),
                );
            }
            Err(e) => {
                repl.chat
                    .add_message(ChatRole::System, format!("File rewind failed: {e}"));
            }
        }
        return Ok(());
    }

    // Confirm before overwriting uncommitted work (W6-2 §3.2).
    let dialog =
        crate::widgets::dialog::DialogWidget::new(format!("Rewind file: {}", path.display()))
            .with_subtitle(format!("restore to {op} @ {time} · {restored_lines} lines"))
            .with_content(format!(
                "Restore `{}` to its most recent saved snapshot?\n\n\
         current on disk: {current_lines} lines\n\
         restore to:      {restored_lines} lines ({op} @ {time})\n\n\
         This overwrites the file's current (possibly uncommitted) contents.",
                path.display(),
            ))
            .with_button(
                crate::widgets::dialog::DialogButton::new(
                    "Revert".to_string(),
                    "rewind_file_confirm".to_string(),
                )
                .dangerous(),
            )
            .with_button(crate::widgets::dialog::DialogButton::new(
                "Cancel".to_string(),
                "cancel".to_string(),
            ));

    repl.state.rewind_file_path = Some(path);
    repl.state.rewind_file_snapshot_id = Some(id);
    repl.state.active_dialog = Some(dialog);
    Ok(())
}

/// W6-2 B.2: revert working-tree files to their state at the end of the turn recorded
/// at checkpoint list position `index`, using `FileHistoryManager` content snapshots
/// (no git). `index` is a list position (as shown by `/rewind history`); the actual
/// `turn_index` is resolved from that entry since the two can diverge across file-less
/// turns. Returns a human-readable summary, or an error.
/// Outcome of a code rewind: the target turn, and which files were restored
/// vs. deleted (because they were created after the target).
#[derive(Debug)]
struct CodeRewindOutcome {
    target_turn: usize,
    restored: Vec<String>,
    deleted: Vec<String>,
    /// Files whose restore/delete I/O failed (permissions, disk full, …).
    failed: Vec<String>,
}

/// Core code-rewind logic, factored out so it is unit-testable without env or
/// a live `Repl`.
///
/// For each file touched in a turn *after* `checkpoints[index].turn_index`,
/// restore it to its end-of-target content (or delete it if it was created
/// after the target) via the file-history manager.
fn apply_code_rewind(
    checkpoints: &[shannon_core::TurnCheckpoint],
    index: usize,
    manager: &mut FileHistoryManager,
    cwd: &Path,
) -> std::result::Result<CodeRewindOutcome, String> {
    let target = checkpoints.get(index).ok_or_else(|| {
        format!(
            "Invalid checkpoint [{index}]. Available: 0..{}",
            checkpoints.len().saturating_sub(1)
        )
    })?;
    let target_turn = target.turn_index;

    // Files touched in any turn AFTER the target are the only ones that may differ
    // from the end-of-target state.
    let mut files_after: Vec<String> = Vec::new();
    for tc in checkpoints {
        if tc.turn_index > target_turn {
            for f in &tc.files_changed {
                if !files_after.contains(f) {
                    files_after.push(f.clone());
                }
            }
        }
    }

    let mut restored: Vec<String> = Vec::new();
    let mut deleted: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();

    for file in &files_after {
        let key = Path::new(file);
        let fs_path = if key.is_absolute() {
            PathBuf::from(file)
        } else {
            cwd.join(file)
        };
        match manager
            .rewind_file_to_turn(key, target_turn)
            .map_err(|e| e.to_string())?
        {
            RewindAction::Restore(content) => match std::fs::write(&fs_path, content) {
                Ok(()) => restored.push(file.clone()),
                Err(e) => {
                    tracing::warn!("rewind: failed to restore {fs_path:?}: {e}");
                    failed.push(file.clone());
                }
            },
            RewindAction::Delete => match std::fs::remove_file(&fs_path) {
                Ok(()) => deleted.push(file.clone()),
                Err(e) => {
                    tracing::warn!("rewind: failed to delete {fs_path:?}: {e}");
                    failed.push(file.clone());
                }
            },
            RewindAction::NoChange => {}
        }
    }

    Ok(CodeRewindOutcome {
        target_turn,
        restored,
        deleted,
        failed,
    })
}

fn run_code_rewind(repl: &Repl, index: usize) -> std::result::Result<String, String> {
    let checkpoints = repl.checkpoint_manager.list_checkpoints();
    let Some(config) = FileHistoryConfig::from_env() else {
        return Err(
            "File history is disabled (SHANNON_FILE_HISTORY=0); cannot rewind code.".into(),
        );
    };
    let mut manager = FileHistoryManager::new(config);
    let cwd = std::env::current_dir().unwrap_or_default();
    let outcome = apply_code_rewind(&checkpoints, index, &mut manager, &cwd)?;

    let mut summary = format!("Reverted code to turn {}.", outcome.target_turn);
    if outcome.restored.is_empty() && outcome.deleted.is_empty() && outcome.failed.is_empty() {
        summary.push_str(" No files needed reverting (no recorded changes after this turn).");
    } else {
        if !outcome.restored.is_empty() {
            summary.push_str(&format!("\nRestored: {}", outcome.restored.join(", ")));
        }
        if !outcome.deleted.is_empty() {
            summary.push_str(&format!(
                "\nDeleted (created after this turn): {}",
                outcome.deleted.join(", ")
            ));
        }
        if !outcome.failed.is_empty() {
            summary.push_str(&format!(
                "\nFailed to revert (I/O error; see logs): {}",
                outcome.failed.join(", ")
            ));
        }
    }
    Ok(summary)
}

pub(crate) fn handle_rewind(repl: &mut Repl, args: &str) -> Result<()> {
    let intent = parse_rewind_intent(args);
    // §4.15 online signals: count the invocation kind (never the argument
    // value — a file path stays here). `history` is a read-only listing and
    // does not count as rewind usage.
    match &intent {
        RewindIntent::History => {}
        RewindIntent::Code(_) => shannon_core::signals::observe_rewind(RewindKind::Code),
        RewindIntent::Both(_) => shannon_core::signals::observe_rewind(RewindKind::Both),
        RewindIntent::Conversation(_) => {
            shannon_core::signals::observe_rewind(RewindKind::Conversation)
        }
        RewindIntent::File { .. } => shannon_core::signals::observe_rewind(RewindKind::File),
    }
    match intent {
        RewindIntent::History => {
            let checkpoints = repl.checkpoint_manager.list_checkpoints();
            if checkpoints.is_empty() {
                repl.chat.add_message(
                    ChatRole::System,
                    "No turn checkpoints available.".to_string(),
                );
                return Ok(());
            }
            let mut msg = String::from("Turn history:\n\n");
            for (i, tc) in checkpoints.iter().enumerate() {
                let time = chrono::DateTime::from_timestamp(tc.checkpoint.timestamp, 0)
                    .map(|t| t.format("%H:%M:%S").to_string())
                    .unwrap_or_else(|| "??:??:??".to_string());
                let files = if tc.files_changed.is_empty() {
                    String::new()
                } else if tc.files_changed.len() <= 3 {
                    format!(" [{}]", tc.files_changed.join(", "))
                } else {
                    format!(" [{} files]", tc.files_changed.len())
                };
                let preview = tc
                    .prompt_preview
                    .as_deref()
                    .map(|p| {
                        if p.len() > 60 {
                            let mut end = 60;
                            while !p.is_char_boundary(end) {
                                end -= 1;
                            }
                            format!("{}...", &p[..end])
                        } else {
                            p.to_string()
                        }
                    })
                    .unwrap_or_default();
                msg.push_str(&format!(
                    "  [{}] turn {} {}{} — {}\n",
                    i, tc.turn_index, time, files, preview,
                ));
            }
            msg.push_str("\n/rewind <n> — rewind conversation by n turns");
            msg.push_str("\n/rewind code <n> — revert code to checkpoint [n]");
            msg.push_str(
                "\n/rewind both <n> — revert code + rewind conversation to checkpoint [n]",
            );
            msg.push_str("\n/rewind <path> — rewind a single file to its previous version");
            repl.chat.add_message(ChatRole::System, msg);
        }

        RewindIntent::Code(index) => match run_code_rewind(repl, index) {
            Ok(summary) => {
                repl.chat.add_message(ChatRole::System, summary);
            }
            Err(e) => {
                repl.chat
                    .add_message(ChatRole::System, format!("Code revert failed: {e}"));
            }
        },

        RewindIntent::Both(index) => {
            // Revert code via content snapshots first; conversation rewind is independent.
            let code_result = run_code_rewind(repl, index);

            // Remove the "/rewind both" command message + rewind the conversation.
            repl.chat.pop_last();
            let turns_to_rewind = repl
                .checkpoint_manager
                .list_checkpoints()
                .len()
                .saturating_sub(index);
            if turns_to_rewind > 0 {
                repl.chat.rewind(turns_to_rewind);
                if let Some(ref mut engine) = repl.query_engine {
                    engine.rewind_conversation(turns_to_rewind);
                }
            }

            let msg = match code_result {
                Ok(summary) => format!(
                    "Rewound to checkpoint [{index}]: reverted code + conversation.\n{summary}"
                ),
                Err(e) => {
                    format!("Rewound conversation to checkpoint [{index}]; code revert failed: {e}")
                }
            };
            repl.chat.add_message(ChatRole::System, msg);
        }

        RewindIntent::Conversation(turns) => {
            if turns == 0 {
                repl.chat.pop_last();
                repl.chat.add_message(
                    ChatRole::System,
                    "Usage: /rewind [n | history | code <n> | both <n> | <path>]".to_string(),
                );
                return Ok(());
            }

            // Remove the "/rewind" command message
            repl.chat.pop_last();

            let before_count = repl.chat.len();
            let removed = repl.chat.rewind(turns);
            let after_count = repl.chat.len();

            if let Some(ref mut engine) = repl.query_engine {
                engine.rewind_conversation(turns);
            }

            if removed > 0 {
                repl.chat.add_message(
                    ChatRole::System,
                    format!(
                        "Rewound {turns} turn(s): removed {removed} messages ({before_count} → {after_count} remaining).\nUse /rewind code <n> to also revert file changes, or /rewind <path> for a single file."
                    ),
                );
            } else {
                repl.chat.add_message(
                    ChatRole::System,
                    "No conversation turns to rewind.".to_string(),
                );
            }
        }

        RewindIntent::File { path, skip_confirm } => {
            run_file_rewind(repl, &path, skip_confirm)?;
        }
    }

    Ok(())
}

pub(crate) fn handle_plan(repl: &mut Repl, args: &str) -> Result<()> {
    let args = args.trim();

    // Handle plan mode deactivation
    if args == "off" || args == "exit" || args == "end" {
        if let Ok(mut flag) = repl.plan_mode_flag.write() {
            *flag = false;
        }
        repl.state.plan.active = false;
        repl.state.plan.approved = false;
        repl.chat.add_message(
            ChatRole::System,
            "Plan mode deactivated. Write operations are now enabled.".to_string(),
        );
        return Ok(());
    }

    // Delegate to cost::handle_plan for all other cases (creates plan, status, approve, reject, etc.)
    // and also activate the plan-mode flag so write tools are blocked.
    super::cost::handle_plan(repl, args)?;

    // If a plan was created (active and has content), also set the engine flag
    if repl.state.plan.active {
        if let Ok(mut flag) = repl.plan_mode_flag.write() {
            *flag = true;
        }
    }

    Ok(())
}

pub(crate) fn handle_compact(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_engine::compact::{CompactEngine, CompactStrategy};

    let Some(ref engine) = repl.query_engine else {
        repl.chat
            .add_message(ChatRole::System, "No query engine available.".to_string());
        return Ok(());
    };

    let history = engine.conversation_history();

    // Parse subcommand early — "status" should work even with empty history
    let subcmd = args.trim();
    if subcmd == "status" || subcmd == "info" {
        // Create compact engine using the REPL's existing tokio runtime handle.
        let client = engine.client().clone();
        let rt_handle = repl.runtime.handle().clone();
        let compact_engine = match CompactEngine::with_llm_summarizer_on_runtime(client, rt_handle)
        {
            Ok(e) => e,
            Err(_) => match CompactEngine::with_defaults() {
                Ok(e) => e,
                Err(e) => {
                    repl.chat
                        .add_message(ChatRole::System, format!("Compact engine error: {e}"));
                    return Ok(());
                }
            },
        };
        let analysis = compact_engine.analyze_context(&history);
        let info = format!(
            "Context Analysis:\n  Estimated tokens: {}\n  Context usage: {:.1}%\n  Messages: {}\n  Should compact: {}\n  Recommended strategy: {}\n  Compactable messages: {}\n  Micro-compact candidates: {}",
            analysis.estimated_tokens,
            analysis.context_usage_ratio * 100.0,
            history.len(),
            if analysis.should_compact { "yes" } else { "no" },
            analysis.recommended_strategy,
            analysis.compactable_message_count,
            analysis.micro_compact_candidates,
        );
        repl.chat.add_message(ChatRole::System, info);
        return Ok(());
    }

    // Early exit for other subcommands when there's nothing to compact
    if history.is_empty() {
        repl.chat
            .add_message(ChatRole::System, "No conversation to compact.".to_string());
        return Ok(());
    }

    // Create compact engine using the REPL's existing tokio runtime handle.
    // This avoids nested-runtime panics when the LLM summarizer calls block_on().
    let client = engine.client().clone();
    let rt_handle = repl.runtime.handle().clone();
    let compact_engine = match CompactEngine::with_llm_summarizer_on_runtime(client, rt_handle) {
        Ok(e) => e,
        Err(_) => match CompactEngine::with_defaults() {
            Ok(e) => e,
            Err(e) => {
                repl.chat
                    .add_message(ChatRole::System, format!("Compact engine error: {e}"));
                return Ok(());
            }
        },
    };

    let analysis = compact_engine.analyze_context(&history);

    // /compact preview — show what will be compacted without doing it
    if subcmd == "preview" || subcmd == "--preview" {
        let total = history.len();
        let recent_keep = compact_engine.config().keep_recent_count;
        let old_count = total.saturating_sub(recent_keep);
        let mut preview = format!(
            "Compact Preview:\n  Total messages: {total}\n  Keep recent: {recent_keep}\n  Compactible: {old_count}\n  Strategy: {}\n  Estimated tokens: {} ({:.1}% of context)",
            analysis.recommended_strategy,
            analysis.estimated_tokens,
            analysis.context_usage_ratio * 100.0,
        );
        if old_count > 0 {
            preview.push_str("\n\nMessages to compact:");
            let preview_count = old_count.min(10);
            for (i, msg) in history.iter().take(preview_count).enumerate() {
                let role = &msg.role;
                let preview_text: String = match &msg.content {
                    shannon_engine::api::MessageContent::Text(t) => t.chars().take(60).collect(),
                    shannon_engine::api::MessageContent::Blocks(blocks) => blocks
                        .iter()
                        .take(1)
                        .filter_map(|b| match b {
                            shannon_engine::api::ContentBlock::Text { text } => {
                                Some(text.chars().take(60).collect::<String>())
                            }
                            _ => None,
                        })
                        .next()
                        .unwrap_or_default(),
                };
                preview.push_str(&format!(
                    "\n  {}. [{role}] {preview_text}{}",
                    i + 1,
                    if preview_text.len() >= 60 { "..." } else { "" }
                ));
            }
            if old_count > preview_count {
                preview.push_str(&format!("\n  ... and {} more", old_count - preview_count));
            }
        }
        preview
            .push_str("\n\nUse /compact to proceed, or /compact <strategy> to choose a strategy.");
        repl.chat.add_message(ChatRole::System, preview);
        return Ok(());
    }

    // /compact focus <topic> — compact but preserve messages about topic
    let (strategy, focus_keywords) = if let Some(focus) = subcmd.strip_prefix("focus ") {
        let keywords: Vec<&str> = focus.split_whitespace().collect();
        repl.chat.add_message(ChatRole::System, format!(
            "Focus compact: preserving messages matching '{}'\nCompacting remaining messages...",
            keywords.join("', '")
        ));
        (
            CompactStrategy::SummarizeOld,
            Some(keywords.into_iter().map(String::from).collect::<Vec<_>>()),
        )
    } else {
        let strategy = match subcmd {
            "truncate" => CompactStrategy::TruncateOld,
            "micro" => CompactStrategy::MicroCompress,
            "group" => CompactStrategy::GroupCompress,
            "auto" | "" => CompactStrategy::SummarizeOld,
            // Treat unrecognized non-empty args as focus keywords (freeform instructions)
            other if !other.is_empty() => {
                let keywords: Vec<&str> = other.split_whitespace().collect();
                repl.chat.add_message(ChatRole::System, format!(
                    "Focus compact: preserving messages matching '{}'\nCompacting remaining messages...",
                    keywords.join("', '")
                ));
                return handle_compact_with_focus(repl, keywords, history, compact_engine);
            }
            _ => CompactStrategy::SummarizeOld,
        };
        (strategy, None)
    };

    // Pre-compaction feedback: tell user what's about to happen
    let compactable = analysis.compactable_message_count;
    let strategy_name = match strategy {
        CompactStrategy::TruncateOld => "truncate",
        CompactStrategy::MicroCompress => "micro",
        CompactStrategy::GroupCompress => "group",
        CompactStrategy::SummarizeOld => "summarize",
        _ => "auto",
    };
    repl.chat.add_message(
        ChatRole::System,
        format!("Compacting context ({compactable} messages, {strategy_name} strategy)..."),
    );

    let (messages, compact_result) = if let Some(ref keywords) = focus_keywords {
        // For focus mode, compact only non-matching messages
        let mut to_compact: Vec<shannon_engine::api::Message> = Vec::new();
        let mut to_keep: Vec<shannon_engine::api::Message> = Vec::new();
        for msg in history {
            let text = match &msg.content {
                shannon_engine::api::MessageContent::Text(t) => t.to_lowercase(),
                shannon_engine::api::MessageContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|b| match b {
                        shannon_engine::api::ContentBlock::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase(),
            };
            let matches_focus = keywords.iter().any(|kw| text.contains(&kw.to_lowercase()));
            if matches_focus || msg.role == "system" {
                to_keep.push(msg);
            } else {
                to_compact.push(msg);
            }
        }
        let _original_count = to_compact.len();
        if !to_compact.is_empty() {
            let mut compact_engine = compact_engine;
            let cr = compact_engine.compact(&mut to_compact);
            to_keep.append(&mut to_compact);
            (to_keep, cr.ok())
        } else {
            (to_keep, None)
        }
    } else {
        let mut messages = history;
        let mut compact_engine = compact_engine;
        let result = match strategy {
            CompactStrategy::MicroCompress => compact_engine.micro_compact(&mut messages),
            CompactStrategy::GroupCompress => compact_engine.group_compact(&mut messages),
            _ => compact_engine.compact(&mut messages),
        };
        (messages, result.ok())
    };

    // Update the query engine's conversation
    if let Some(ref mut engine) = repl.query_engine {
        engine.replace_conversation(messages);
    }

    if let Some(compact_result) = compact_result {
        let mut report = format!(
            "Context compacted:\n  Strategy: {}\n  Tokens: {} → {} ({:.1}% reduction)\n  Messages removed: {}\n  Messages compacted: {}\n  Duration: {:.2}s",
            compact_result.strategy,
            compact_result.original_tokens,
            compact_result.compacted_tokens,
            compact_result.reduction_ratio * 100.0,
            compact_result.messages_removed,
            compact_result.messages_compacted,
            compact_result.duration.as_secs_f64(),
        );
        if let Some(ref kws) = focus_keywords {
            report.push_str(&format!("\n  Focus: {}", kws.join(", ")));
        }
        repl.chat.add_message(ChatRole::System, report);
    } else if focus_keywords.is_some() {
        repl.chat.add_message(
            ChatRole::System,
            "Focus compact complete (no compaction needed for focused messages).".to_string(),
        );
    }

    Ok(())
}

/// Helper for freeform-focus compaction (called when /compact receives unrecognized non-empty args).
fn handle_compact_with_focus(
    repl: &mut Repl,
    keywords: Vec<&str>,
    history: Vec<shannon_engine::api::Message>,
    compact_engine: shannon_engine::compact::CompactEngine,
) -> Result<()> {
    let keyword_strings: Vec<String> = keywords.iter().map(|s| s.to_string()).collect();
    let mut to_compact: Vec<shannon_engine::api::Message> = Vec::new();
    let mut to_keep: Vec<shannon_engine::api::Message> = Vec::new();
    for msg in history {
        let text = match &msg.content {
            shannon_engine::api::MessageContent::Text(t) => t.to_lowercase(),
            shannon_engine::api::MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    shannon_engine::api::ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase(),
        };
        let matches_focus = keywords.iter().any(|kw| text.contains(&kw.to_lowercase()));
        if matches_focus || msg.role == "system" {
            to_keep.push(msg);
        } else {
            to_compact.push(msg);
        }
    }
    let compact_result = if !to_compact.is_empty() {
        let count = to_compact.len();
        repl.chat.add_message(
            ChatRole::System,
            format!(
                "Focus compacting {count} messages (preserving matches for '{}')...",
                keyword_strings.join("', '")
            ),
        );
        let mut compact_engine = compact_engine;
        let cr = compact_engine.compact(&mut to_compact);
        to_keep.append(&mut to_compact);
        cr.ok()
    } else {
        None
    };

    if let Some(ref mut engine) = repl.query_engine {
        engine.replace_conversation(to_keep);
    }

    if let Some(cr) = compact_result {
        repl.chat.add_message(ChatRole::System, format!(
            "Context compacted (focus: {}):\n  Tokens: {} → {} ({:.1}% reduction)\n  Messages removed: {}",
            keyword_strings.join(", "),
            cr.original_tokens, cr.compacted_tokens, cr.reduction_ratio * 100.0,
            cr.messages_removed,
        ));
    } else {
        repl.chat.add_message(
            ChatRole::System,
            "Focus compact complete (no compaction needed for focused messages).".to_string(),
        );
    }

    Ok(())
}

/// /session — manage conversation sessions (list, export).
pub(crate) fn handle_session(repl: &mut Repl, args: &str) -> Result<()> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    let subcmd = parts.first().copied().unwrap_or("list");

    match subcmd {
        "list" | "ls" | "" => {
            let sessions = repl.l0_store().list().unwrap_or_default();

            if sessions.is_empty() {
                repl.chat
                    .add_message(ChatRole::System, "No saved sessions found.".to_string());
                return Ok(());
            }

            let mut msg = String::from("Saved Sessions:\n\n");
            for (i, s) in sessions.iter().take(20).enumerate() {
                let title = s
                    .title
                    .as_deref()
                    .or(s.preview.as_deref())
                    .unwrap_or("(untitled)");
                let time = s.updated_at.format("%m/%d %H:%M");
                let tokens = s.total_input_tokens + s.total_output_tokens;
                msg.push_str(&format!(
                    "  {:>2}. {}  {}  {} turns  {} tokens\n      ID: {}\n\n",
                    i + 1,
                    title,
                    time,
                    s.turn_count,
                    tokens,
                    s.session_id,
                ));
            }

            if sessions.len() > 20 {
                msg.push_str(&format!("  ... and {} more\n", sessions.len() - 20));
            }

            msg.push_str("\nUsage: /session list | /session export");
            repl.chat.add_message(ChatRole::System, msg);
        }
        "export" => {
            // Export current session as markdown
            let engine = match repl.query_engine.as_ref() {
                Some(e) => e,
                None => {
                    repl.chat
                        .add_message(ChatRole::System, "No active session to export.".to_string());
                    return Ok(());
                }
            };

            let messages = engine.conversation_history();
            if messages.is_empty() {
                repl.chat
                    .add_message(ChatRole::System, "Current session is empty.".to_string());
                return Ok(());
            }

            let mut md = String::from("# Shannon Session Export\n\n");
            md.push_str(&format!(
                "Date: {}\nModel: {}\n\n---\n\n",
                chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
                repl.state.model.as_deref().unwrap_or("unknown"),
            ));

            for msg in &messages {
                let role = match msg.role.as_str() {
                    "user" => "## User",
                    "assistant" => "## Assistant",
                    "system" => "## System",
                    _ => "## Message",
                };
                let text = match &msg.content {
                    shannon_engine::api::MessageContent::Text(t) => t.clone(),
                    shannon_engine::api::MessageContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| match b {
                            shannon_engine::api::ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                md.push_str(&format!("{role}\n\n{text}\n\n---\n\n"));
            }

            let filename = format!(
                "shannon-session-{}.md",
                chrono::Utc::now().format("%Y%m%d-%H%M%S")
            );
            let path = std::path::Path::new(&filename);
            match std::fs::write(path, &md) {
                Ok(()) => {
                    repl.chat.add_message(
                        ChatRole::System,
                        format!(
                            "Session exported to {filename} ({} messages)",
                            messages.len()
                        ),
                    );
                }
                Err(e) => {
                    super::set_error(repl, &format!("exporting session: {e}"));
                }
            }
        }
        _ => {
            repl.chat.add_message(
                ChatRole::System,
                "Usage: /session list | /session export".to_string(),
            );
        }
    }

    Ok(())
}

pub(crate) fn handle_rename(repl: &mut Repl, args: &str) -> Result<()> {
    let name = args.trim();
    if name.is_empty() {
        // Show current title
        match &repl.state.session_title {
            Some(title) => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Current session: {title}\nUsage: /rename <new-name>"),
                );
            }
            None => {
                repl.chat.add_message(
                    ChatRole::System,
                    "No custom session name set.\nUsage: /rename <new-name>".to_string(),
                );
            }
        }
        return Ok(());
    }

    if name == "reset" || name == "clear" {
        repl.state.session_title = None;
        repl.chat.add_message(
            ChatRole::System,
            "Session name reset to default.".to_string(),
        );
        return Ok(());
    }

    repl.state.session_title = Some(name.to_string());
    repl.chat
        .add_message(ChatRole::System, format!("Session renamed to: {name}"));
    Ok(())
}

/// /recap — Generate a summary of the conversation so far.
///
/// Shows message counts by role, the last N user messages, total turns,
/// and the session title if set. REPL-only, no API call.
pub(crate) fn handle_recap(repl: &mut Repl, _args: &str) -> Result<()> {
    let total = repl.chat.len();
    if total == 0 {
        repl.chat.add_message(
            ChatRole::System,
            "No messages in this session yet.".to_string(),
        );
        return Ok(());
    }

    let mut user_count = 0usize;
    let mut assistant_count = 0usize;
    let mut system_count = 0usize;
    let mut tool_count = 0usize;
    let mut user_messages: Vec<String> = Vec::new();

    for i in 0..repl.chat.len() {
        if let Some(msg) = repl.chat.get_message(i) {
            match msg.role {
                ChatRole::User => {
                    user_count += 1;
                    let preview: String = msg.content.chars().take(80).collect();
                    let ellipsis = if msg.content.len() > 80 { "..." } else { "" };
                    user_messages.push(format!("{preview}{ellipsis}"));
                }
                ChatRole::Assistant => assistant_count += 1,
                ChatRole::System => system_count += 1,
                ChatRole::Tool => tool_count += 1,
            }
        }
    }

    let mut output = String::from("Conversation Recap:\n\n");
    output.push_str(&format!(
        "  Messages: {total} total ({user_count} user, {assistant_count} assistant, {system_count} system, {tool_count} tool)\n"
    ));
    output.push_str(&format!("  Turns: {}\n", repl.state.turn_count));

    if let Some(ref title) = repl.state.session_title {
        output.push_str(&format!("  Session: \"{title}\"\n"));
    }

    if let Some(started) = &repl.session_started_at {
        let elapsed = chrono::Utc::now() - *started;
        let mins = elapsed.num_minutes();
        let secs = elapsed.num_seconds() % 60;
        output.push_str(&format!("  Duration: {mins}m {secs}s\n"));
    }

    let model = repl.state.model.as_deref().unwrap_or("default");
    output.push_str(&format!("  Model: {model}\n"));

    if !user_messages.is_empty() {
        let last_n: Vec<&String> = user_messages.iter().rev().take(5).collect();
        output.push_str("\n  Recent user messages:\n");
        for (i, msg) in last_n.iter().rev().enumerate() {
            output.push_str(&format!("    {}. {msg}\n", i + 1));
        }
    }

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

/// /effort — Set or view the thinking effort level for the model.
///
/// With no args: show current effort level.
/// With args "low", "medium", "high": set the effort level.
pub(crate) fn handle_effort(repl: &mut Repl, args: &str) -> Result<()> {
    let level = args.trim().to_lowercase();

    if level.is_empty() {
        match &repl.state.effort_level {
            Some(effort) => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Current effort level: {effort}\nUsage: /effort <low|medium|high>"),
                );
            }
            None => {
                repl.chat.add_message(
                    ChatRole::System,
                    "No effort level set (using model default).\nUsage: /effort <low|medium|high>"
                        .to_string(),
                );
            }
        }
        return Ok(());
    }

    match level.as_str() {
        "low" | "medium" | "high" => {
            repl.state.effort_level = Some(level.clone());
            repl.chat
                .add_message(ChatRole::System, format!("Effort level set to: {level}"));
        }
        _ => {
            repl.chat.add_message(
                ChatRole::System,
                "Invalid effort level. Use: low, medium, or high.".to_string(),
            );
        }
    }

    Ok(())
}

/// /focus — Set context focus to limit what the model pays attention to.
///
/// With no args: show current focus.
/// With args: set focus area (e.g., "frontend", "backend", "security").
/// With "off" or "clear": remove focus.
pub(crate) fn handle_focus(repl: &mut Repl, args: &str) -> Result<()> {
    let area = args.trim();

    if area.is_empty() {
        match &repl.state.focus_area {
            Some(focus) => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Current focus: {focus}\nUsage: /focus <area> | /focus off"),
                );
            }
            None => {
                repl.chat.add_message(
                    ChatRole::System,
                    "No focus area set.\nUsage: /focus <area> (e.g., frontend, backend, security)"
                        .to_string(),
                );
            }
        }
        return Ok(());
    }

    let area_lower = area.to_lowercase();
    if area_lower == "off" || area_lower == "clear" {
        repl.state.focus_area = None;
        repl.chat
            .add_message(ChatRole::System, "Focus area cleared.".to_string());
    } else {
        repl.state.focus_area = Some(area.to_string());
        repl.chat
            .add_message(ChatRole::System, format!("Focus area set to: {area}"));
    }

    Ok(())
}

#[cfg(test)]
mod rewind_tests {
    use super::*;
    use shannon_tools::FileHistoryOperation;

    // --- parse_rewind_intent -------------------------------------------------

    #[test]
    fn parse_conversation_variants() {
        assert!(matches!(
            parse_rewind_intent(""),
            RewindIntent::Conversation(1)
        ));
        assert!(matches!(
            parse_rewind_intent("   "),
            RewindIntent::Conversation(1)
        ));
        assert!(matches!(
            parse_rewind_intent("3"),
            RewindIntent::Conversation(3)
        ));
        assert!(matches!(
            parse_rewind_intent("0"),
            RewindIntent::Conversation(0)
        ));
    }

    #[test]
    fn parse_keyword_subcommands() {
        assert!(matches!(
            parse_rewind_intent("history"),
            RewindIntent::History
        ));
        assert!(matches!(parse_rewind_intent("list"), RewindIntent::History));
        assert!(matches!(parse_rewind_intent("ls"), RewindIntent::History));
        assert!(matches!(
            parse_rewind_intent("code 2"),
            RewindIntent::Code(2)
        ));
        assert!(matches!(
            parse_rewind_intent("both 5"),
            RewindIntent::Both(5)
        ));
        // A non-numeric `code <x>` argument falls through to file-path handling.
        assert!(matches!(
            parse_rewind_intent("code abc"),
            RewindIntent::File { .. }
        ));
    }

    #[test]
    fn parse_file_path_with_and_without_yes() {
        match parse_rewind_intent("src/main.rs") {
            RewindIntent::File { path, skip_confirm } => {
                assert_eq!(path, "src/main.rs");
                assert!(!skip_confirm);
            }
            other => panic!("expected File, got {other:?}"),
        }
        match parse_rewind_intent("src/main.rs --yes") {
            RewindIntent::File { path, skip_confirm } => {
                assert_eq!(path, "src/main.rs");
                assert!(skip_confirm);
            }
            other => panic!("expected File, got {other:?}"),
        }
        // A path with spaces before --yes keeps the full path.
        match parse_rewind_intent("src/my file.rs --yes") {
            RewindIntent::File { path, skip_confirm } => {
                assert_eq!(path, "src/my file.rs");
                assert!(skip_confirm);
            }
            other => panic!("expected File, got {other:?}"),
        }
        // Bare --yes (no target) defaults to a 1-turn conversation rewind.
        assert!(matches!(
            parse_rewind_intent("--yes"),
            RewindIntent::Conversation(1)
        ));
    }

    // --- strip_trailing_yes --------------------------------------------------

    #[test]
    fn strip_yes_handles_trailing_flag() {
        assert_eq!(strip_trailing_yes("foo --yes"), ("foo".to_string(), true));
        assert_eq!(strip_trailing_yes("foo"), ("foo".to_string(), false));
        assert_eq!(
            strip_trailing_yes("  bar --yes  "),
            ("bar".to_string(), true)
        );
        assert_eq!(strip_trailing_yes("--yes"), ("".to_string(), true));
    }

    // --- resolve_tracked_path ------------------------------------------------

    #[test]
    fn resolve_exact_and_suffix_match() {
        let tracked: Vec<PathBuf> = vec![
            PathBuf::from("/abs/src/foo.rs"),
            PathBuf::from("/abs/other/bar.rs"),
        ];
        // Exact string match.
        assert_eq!(
            resolve_tracked_path(&tracked, "/abs/src/foo.rs"),
            Some(PathBuf::from("/abs/src/foo.rs"))
        );
        // Suffix match: a relative path resolves to the tracked absolute one.
        assert_eq!(
            resolve_tracked_path(&tracked, "src/foo.rs"),
            Some(PathBuf::from("/abs/src/foo.rs"))
        );
        // No match.
        assert_eq!(resolve_tracked_path(&tracked, "nope.rs"), None);
    }

    // --- select_restore_snapshot + restore_file_snapshot ---------------------

    fn mgr_in_tmp() -> (tempfile::TempDir, FileHistoryManager) {
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = FileHistoryConfig {
            history_dir: tmp.path().to_path_buf(),
            max_history_per_file: 50,
            max_total_history_mb: 100,
            ttl: Some(604_800),
        };
        (tmp, FileHistoryManager::new(cfg))
    }

    #[test]
    fn select_restore_picks_previous_version() {
        // Real flow: snapshots record pre-edit content. Two edits → [v1, v2],
        // disk now at v3 (post last edit). Rewind selects v2 (undo last edit).
        let (_tmp, mut mgr) = mgr_in_tmp();
        let path = Path::new("/tmp/shannon-rewind-fake.rs");
        mgr.record_snapshot(path, "v1\n", FileHistoryOperation::Edit)
            .unwrap();
        mgr.record_snapshot(path, "v2\n", FileHistoryOperation::Edit)
            .unwrap();

        let history = mgr.get_history(path).unwrap();
        let pick = select_restore_snapshot(&history.snapshots, "v3\n").unwrap();
        assert_eq!(pick.content, "v2\n");
    }

    #[test]
    fn select_restore_none_when_already_earliest() {
        let (_tmp, mut mgr) = mgr_in_tmp();
        let path = Path::new("/tmp/shannon-rewind-fake2.rs");
        mgr.record_snapshot(path, "only\n", FileHistoryOperation::Edit)
            .unwrap();

        let history = mgr.get_history(path).unwrap();
        // Disk equals the only snapshot → nothing earlier to restore.
        assert!(select_restore_snapshot(&history.snapshots, "only\n").is_none());
    }

    #[test]
    fn restore_writes_previous_version_to_disk() {
        let (_hist_tmp, mut mgr) = mgr_in_tmp();
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("target.txt");

        // File at "original"; snapshot it; then mutate to "changed".
        std::fs::write(&file, "original\n").unwrap();
        mgr.record_snapshot(&file, "original\n", FileHistoryOperation::Edit)
            .unwrap();
        std::fs::write(&file, "changed\n").unwrap();

        let history = mgr.get_history(&file).unwrap();
        let current = std::fs::read_to_string(&file).unwrap();
        let target = select_restore_snapshot(&history.snapshots, &current).unwrap();

        let restored = restore_file_snapshot(&mut mgr, &file, &target.id).unwrap();
        assert_eq!(restored, "original\n");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original\n");
    }

    // --- apply_code_rewind (B.2 end-to-end) --------------------------------
    //
    // Wires the two B.2 halves: turn-tagged content snapshots (what
    // `capture_turn_snapshots` records) → multi-file restore/delete (what
    // `run_code_rewind` orchestrates). No Repl or env needed — the core takes
    // the manager + checkpoints directly.

    #[test]
    fn apply_code_rewind_restores_and_deletes_across_turns() {
        //   turn 0: edit `a.txt` (content "v0")
        //   turn 1: edit `a.txt` again ("v1") + create `b.txt` ("b1")
        // Rewind to turn 0 → `a.txt` restored to "v0", `b.txt` deleted.
        let (tmp, mut mgr) = mgr_in_tmp();
        let cwd = tmp.path();
        let a = cwd.join("a.txt");
        let b = cwd.join("b.txt");
        let a_s = a.to_string_lossy().to_string();
        let b_s = b.to_string_lossy().to_string();

        // Simulate `capture_turn_snapshots` recording post-turn content.
        mgr.record_turn_snapshot(&a, "v0\n", 0).unwrap();
        mgr.record_turn_snapshot(&a, "v1\n", 1).unwrap();
        mgr.record_turn_snapshot(&b, "b1\n", 1).unwrap();

        // Disk is in the post-turn-1 state.
        std::fs::write(&a, "v1\n").unwrap();
        std::fs::write(&b, "b1\n").unwrap();

        let cps = vec![
            shannon_core::TurnCheckpoint {
                turn_index: 0,
                checkpoint: shannon_core::Checkpoint {
                    description: "turn 0".into(),
                    timestamp: 0,
                },
                files_changed: vec![a_s.clone()],
                prompt_preview: None,
            },
            shannon_core::TurnCheckpoint {
                turn_index: 1,
                checkpoint: shannon_core::Checkpoint {
                    description: "turn 1".into(),
                    timestamp: 1,
                },
                files_changed: vec![a_s.clone(), b_s.clone()],
                prompt_preview: None,
            },
        ];

        let outcome = apply_code_rewind(&cps, 0, &mut mgr, cwd).unwrap();
        assert_eq!(outcome.target_turn, 0);
        assert!(outcome.restored.contains(&a_s));
        assert!(outcome.deleted.contains(&b_s));

        // Disk reflects the rewind: `a.txt` back to "v0", `b.txt` gone.
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "v0\n");
        assert!(!b.exists());
    }

    #[test]
    fn apply_code_rewind_surfaces_failed_restore() {
        // When the restore I/O fails (here: the target's parent dir is gone),
        // the file is reported in `failed` rather than silently dropped, so a
        // failed rewind is visible instead of looking like a no-op.
        let (tmp, mut mgr) = mgr_in_tmp();
        let cwd = tmp.path();
        let subdir = cwd.join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        let a = subdir.join("a.txt");
        let a_s = a.to_string_lossy().to_string();

        // turn 0: a.txt = "v0"; turn 1: a.txt = "v1".
        mgr.record_turn_snapshot(&a, "v0\n", 0).unwrap();
        mgr.record_turn_snapshot(&a, "v1\n", 1).unwrap();
        std::fs::write(&a, "v1\n").unwrap();

        // Remove the parent dir so the restore write fails (ENOENT).
        std::fs::remove_dir_all(&subdir).unwrap();

        let cps = vec![
            shannon_core::TurnCheckpoint {
                turn_index: 0,
                checkpoint: shannon_core::Checkpoint {
                    description: "turn 0".into(),
                    timestamp: 0,
                },
                files_changed: vec![a_s.clone()],
                prompt_preview: None,
            },
            shannon_core::TurnCheckpoint {
                turn_index: 1,
                checkpoint: shannon_core::Checkpoint {
                    description: "turn 1".into(),
                    timestamp: 1,
                },
                files_changed: vec![a_s.clone()],
                prompt_preview: None,
            },
        ];

        let outcome = apply_code_rewind(&cps, 0, &mut mgr, cwd).unwrap();
        assert_eq!(outcome.target_turn, 0);
        assert!(outcome.restored.is_empty());
        assert!(outcome.failed.contains(&a_s));
    }

    #[test]
    fn apply_code_rewind_no_change_when_target_is_latest() {
        // Rewinding to the latest turn touches no later turns → no-op.
        let (tmp, mut mgr) = mgr_in_tmp();
        let cwd = tmp.path();
        let a = cwd.join("a.txt");
        let a_s = a.to_string_lossy().to_string();
        mgr.record_turn_snapshot(&a, "v0\n", 0).unwrap();
        std::fs::write(&a, "v0\n").unwrap();

        let cps = vec![shannon_core::TurnCheckpoint {
            turn_index: 0,
            checkpoint: shannon_core::Checkpoint {
                description: "turn 0".into(),
                timestamp: 0,
            },
            files_changed: vec![a_s.clone()],
            prompt_preview: None,
        }];

        let outcome = apply_code_rewind(&cps, 0, &mut mgr, cwd).unwrap();
        assert_eq!(outcome.target_turn, 0);
        assert!(outcome.restored.is_empty());
        assert!(outcome.deleted.is_empty());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "v0\n");
    }

    #[test]
    fn apply_code_rewind_invalid_index_errors() {
        let (_tmp, mut mgr) = mgr_in_tmp();
        let cps: Vec<shannon_core::TurnCheckpoint> = vec![];
        let err = apply_code_rewind(&cps, 0, &mut mgr, std::path::Path::new(".")).unwrap_err();
        assert!(err.contains("Invalid checkpoint"));
    }

    // --- capture → rewind, real production functions end-to-end -------------
    //
    // Drives the actual REPL capture helper (`capture_turn_snapshots_with`,
    // invoked at the turn boundary in query.rs) through the actual rewind core
    // (`apply_code_rewind`), sharing one on-disk history manager. No env, no
    // TUI, no hand-built snapshots — this is the closest automated check to
    // the real `/rewind code <n>` flow.

    #[test]
    fn capture_then_apply_code_rewind_end_to_end() {
        use crate::repl::query::capture_turn_snapshots_with;

        //   turn 0: edit a.txt (content "v0")
        //   turn 1: edit a.txt ("v1") + create b.txt ("b1")
        // Rewind to turn 0 → a.txt restored to "v0", b.txt deleted.
        let (tmp, mut mgr) = mgr_in_tmp();
        let cwd = tmp.path();
        let a = cwd.join("a.txt");
        let b = cwd.join("b.txt");
        let a_s = a.to_string_lossy().to_string();
        let b_s = b.to_string_lossy().to_string();

        // turn 0: write a.txt, then capture its post-turn state.
        std::fs::write(&a, "v0\n").unwrap();
        capture_turn_snapshots_with(&mut mgr, &[a_s.clone()], 0);

        // turn 1: mutate a.txt + create b.txt, then capture both.
        std::fs::write(&a, "v1\n").unwrap();
        std::fs::write(&b, "b1\n").unwrap();
        capture_turn_snapshots_with(&mut mgr, &[a_s.clone(), b_s.clone()], 1);

        // Checkpoints as the REPL's CheckpointManager.record_turn would produce.
        let cps = vec![
            shannon_core::TurnCheckpoint {
                turn_index: 0,
                checkpoint: shannon_core::Checkpoint {
                    description: "turn 0".into(),
                    timestamp: 0,
                },
                files_changed: vec![a_s.clone()],
                prompt_preview: None,
            },
            shannon_core::TurnCheckpoint {
                turn_index: 1,
                checkpoint: shannon_core::Checkpoint {
                    description: "turn 1".into(),
                    timestamp: 1,
                },
                files_changed: vec![a_s.clone(), b_s.clone()],
                prompt_preview: None,
            },
        ];

        let outcome = apply_code_rewind(&cps, 0, &mut mgr, cwd).unwrap();
        assert_eq!(outcome.target_turn, 0);
        assert!(outcome.restored.contains(&a_s));
        assert!(outcome.deleted.contains(&b_s));

        // Disk reflects the rewind through the real capture+rewind pipeline.
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "v0\n");
        assert!(!b.exists());
    }
}
