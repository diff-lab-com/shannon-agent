//! REPL command dispatch and handler implementations

mod browser;
mod config;
mod cost;
mod debug;
mod extensions;
mod file_ops;
mod git;
mod goal;
mod hooks;
mod loop_engine;
mod media;
mod memory;
mod remote;
mod session;
mod web;

pub(crate) use goal::ReplGoalAccess;
pub(crate) use goal::check_goal_continuation;
pub(crate) use goal::maybe_fire_check_in;

// Re-export the single switch-path helper so the REPL init/resume paths
// (repl/mod.rs) can refresh the first-screen StatusCard through the same
// derivation used by every /connect, /model, /provider switch
// (ADR-0008 Decision 1+2).
pub(crate) use config::{apply_model_selection, provider_unconfigured, sync_active_to_chat};

// Re-export public API
#[allow(unused_imports)]
pub(crate) use cost::extract_plan_steps;
#[allow(unused_imports)]
pub(crate) use git::format_change_bar;
pub(crate) use loop_engine::{check_loop_iteration, check_ralph_iteration, notify_query_complete};
pub use media::handle_image_paste_from_input;
pub(crate) use media::{copy_nth_response, copy_to_clipboard};
pub(crate) use session::apply_file_rewind;

use crate::{Result, widgets::ChatRole};
use rust_i18n::t;
use shannon_types::recover_lock;

use super::Repl;

/// Display an error message in the chat as a system message.
/// All user-facing error messages from slash commands should use this helper
/// for a consistent "Error: <msg>" format.
pub(crate) fn set_error(repl: &mut Repl, msg: &str) {
    repl.chat
        .add_message(ChatRole::System, format!("Error: {msg}"));
}

/// Expand `[Pasted Text #N X lines]` markers with the actual stored content.
/// Removes expanded entries from the map.
fn expand_pasted_texts(
    input: &str,
    pasted_texts: &mut std::collections::HashMap<usize, String>,
) -> String {
    let marker_prefix = "[Pasted Text #";
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;
    let mut expanded_keys = Vec::new();

    while let Some(start) = remaining.find(marker_prefix) {
        result.push_str(&remaining[..start]);
        let after = &remaining[start + marker_prefix.len()..];

        // Extract the number
        let num_end = after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len());
        if let Ok(num) = after[..num_end].parse::<usize>() {
            // Find closing bracket
            if let Some(bracket_end) = after.find(']') {
                if let Some(content) = pasted_texts.get(&num) {
                    result.push_str(content);
                    expanded_keys.push(num);
                } else {
                    // Paste not found, keep the marker as-is
                    result.push_str(marker_prefix);
                    result.push_str(&after[..bracket_end + 1]);
                }
                remaining = &after[bracket_end + 1..];
            } else {
                result.push_str(remaining);
                remaining = "";
            }
        } else {
            result.push_str(remaining);
            remaining = "";
        }
    }
    result.push_str(remaining);

    for key in expanded_keys {
        pasted_texts.remove(&key);
    }
    result
}

/// Redact inline secrets from a recorded command line so they are never
/// persisted into the chat widget, command history, or session JSON.
///
/// Currently redacts:
/// - the API key from `/connect <provider> <key>`, and
/// - the value from `/credentials store <service> <value>` (any alias, any
///   store spelling — the REPL executes it directly via the CredentialManager,
///   so a plaintext secret must never reach the *recorded* text either),
///
/// in both cases replacing the secret with `***`. The real argument still
/// reaches the command handler — this redaction only affects what is
/// *recorded* (chat message + up-arrow history). Returns the input unchanged
/// for any other command, free-text input, or an invocation without an inline
/// secret.
///
/// Tokenization uses `split_whitespace`, matching how `parse_connect_args`
/// splits the real command, so runs of whitespace (`/connect  minimax  k`) are
/// handled the same way as the single-space form.
fn redact_secret_command(input: &str) -> String {
    // Preserve any leading whitespace the user typed before the '/'.
    let trimmed = input.trim_start();
    let lead = &input[..input.len() - trimmed.len()];
    let rest = match trimmed.strip_prefix('/') {
        Some(r) => r,
        None => return input.to_string(),
    };
    let mut tokens = rest.split_whitespace();
    let cmd = tokens.next().unwrap_or("");
    if cmd.eq_ignore_ascii_case("connect") {
        let provider = match tokens.next() {
            Some(p) if !p.is_empty() => p,
            _ => return input.to_string(),
        };
        return match tokens.next() {
            Some(k) if !k.is_empty() => format!("{lead}/connect {provider} ***"),
            _ => input.to_string(),
        };
    }
    // `/credentials store <svc> <value>` (aliases /creds, /cred; store
    // spellings store/add/set — all parsed to `CredentialAction::Store`).
    // Recorded form is normalized to the canonical `/credentials store`.
    if matches!(
        cmd.to_ascii_lowercase().as_str(),
        "credentials" | "creds" | "cred"
    ) {
        let sub = tokens.next().map(str::to_ascii_lowercase);
        if !matches!(sub.as_deref(), Some("store" | "add" | "set")) {
            return input.to_string();
        }
        let service = match tokens.next() {
            Some(s) if !s.is_empty() => s,
            _ => return input.to_string(),
        };
        // The handler takes everything after <service> as the value
        // (`splitn(3, ' ')`), so any remaining text is part of the secret.
        if tokens.next().is_none() {
            return input.to_string();
        }
        return format!("{lead}/credentials store {service} ***");
    }
    input.to_string()
}

// ── Inline `!shell` execution (P0-1) ────────────────────────────────────
//
// "!cmd" used to run synchronously inside the REPL submit path; in raw mode
// there was no timeout and no cancel, so `!sleep infinity` froze the whole
// TUI forever. The command now runs on a background worker thread while the
// event loop stays responsive. Completion is delivered over a channel the
// main loop drains without blocking (`poll_inline_shell_jobs`), the
// placeholder tool message is finalized in place, and Esc — or the
// wall-clock timeout — SIGKILLs the child's whole process group: the child
// is spawned as a group leader (`process_group(0)`), so descendants die
// with it and cannot keep the output pipes open.

/// Default wall-clock budget for an inline `!shell` job, in seconds.
const INLINE_SHELL_DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Worker poll interval while the child runs (cheap next to the 50ms UI tick).
const INLINE_SHELL_WAIT_POLL: std::time::Duration = std::time::Duration::from_millis(20);

/// Outcome of an inline `!shell` job, sent by the worker thread to the UI.
/// Exactly one outcome is delivered per job.
#[derive(Debug)]
pub enum ShellJobOutcome {
    /// Child exited on its own.
    Completed {
        success: bool,
        exit_code: Option<i32>,
        stdout: String,
        stderr: String,
    },
    /// Wall-clock budget expired; the worker killed the process group.
    TimedOut { timeout_secs: u64 },
    /// The user pressed Esc; the UI thread killed the process group and the
    /// worker observed the death.
    Cancelled,
    /// `sh` could not be spawned (also the only spawn-shaped outcome on
    /// non-unix targets, where `sh` does not exist) — mirrors the legacy
    /// "Failed to execute" message.
    SpawnFailed(String),
}

/// Handle to an in-flight inline `!shell` job, kept on `ReplState`. The
/// worker thread owns the child; the UI thread only ever touches the
/// channel receiver and (on Esc) the cancel flag + process-group kill, so
/// the REPL event loop never blocks on the job. The channel is a tokio
/// unbounded one because `Repl` must stay `Sync` (see `UiAdapter`) — same
/// reason `ReplState` carries the other UI-bound receivers.
#[derive(Debug)]
pub struct ShellJob {
    /// Delivers exactly one outcome from the worker thread.
    pub rx: tokio::sync::mpsc::UnboundedReceiver<ShellJobOutcome>,
    /// Set by the UI thread right before the Esc kill so the worker labels
    /// the observed child death as a user cancel, not a normal exit.
    pub cancel_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Process-group id (the child pid: spawned with `process_group(0)`).
    /// `None` on non-unix targets, where group kill is unavailable — there
    /// Esc cannot cancel; the timeout (direct-child kill) still applies.
    pub pgid: Option<u32>,
    /// Chat index of the placeholder tool message, finalized in place.
    pub placeholder_idx: usize,
    /// Command text as typed (leading `!` stripped).
    pub cmd: String,
    /// When execution started (duration display on the finalized message).
    pub started_at: chrono::DateTime<chrono::Utc>,
}

/// Read the inline-shell timeout from `SHANNON_INLINE_SHELL_TIMEOUT`
/// (seconds). Unset, non-numeric, or non-positive values fall back to the
/// 30s default so a bad value can never disable the timeout.
pub(crate) fn inline_shell_timeout_from_env() -> u64 {
    std::env::var("SHANNON_INLINE_SHELL_TIMEOUT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(INLINE_SHELL_DEFAULT_TIMEOUT_SECS)
}

/// Launch an inline `!shell` command: add the placeholder tool message,
/// spawn the child as a process-group leader (unix), and move the wait to a
/// worker thread. `timeout_secs` is a parameter so tests can inject a short
/// budget; production call sites pass `inline_shell_timeout_from_env()`.
pub(crate) fn start_inline_shell(repl: &mut Repl, shell_cmd: &str, timeout_secs: u64) {
    // Single-flight: one job slot on `ReplState` (the brief pins a single
    // placeholder + single Esc target). Point the user at Esc instead of
    // silently queueing or racing a second child.
    if repl.state.shell_job.is_some() {
        repl.chat.add_message(
            ChatRole::System,
            "An inline command is already running — press Esc to cancel it first.".to_string(),
        );
        return;
    }

    let started_at = chrono::Utc::now();
    let placeholder_idx = repl.chat.add_tool_message(
        shell_cmd.to_string(),
        format!("$ {shell_cmd} — running, Esc to cancel"),
        false,
        Some(started_at),
    );

    // Spawn (fork/exec — never the command duration) on the UI thread so the
    // process-group id is known immediately for the Esc path. A failed spawn
    // posts the legacy message synchronously, exactly like the old code.
    let mut command = std::process::Command::new("sh");
    command
        .arg("-c")
        .arg(shell_cmd)
        .current_dir(&repl.state.working_directory)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Group leader: kill(-pgid) then takes down the whole tree.
        let _ = command.process_group(0);
    }

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    match command.spawn() {
        Ok(child) => {
            #[cfg(unix)]
            let pgid = Some(child.id());
            #[cfg(not(unix))]
            let pgid: Option<u32> = None;
            let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let worker_flag = cancel_flag.clone();
            match std::thread::Builder::new()
                .name("inline-shell".to_string())
                .spawn(move || wait_inline_shell_child(child, timeout_secs, worker_flag, tx))
            {
                Ok(_handle) => {
                    repl.state.shell_job = Some(ShellJob {
                        rx,
                        cancel_flag,
                        pgid,
                        placeholder_idx,
                        cmd: shell_cmd.to_string(),
                        started_at,
                    });
                }
                Err(e) => {
                    // No worker means no channel round-trip; finalize the
                    // placeholder directly with the failure.
                    let job = ShellJob {
                        rx,
                        cancel_flag,
                        pgid: None,
                        placeholder_idx,
                        cmd: shell_cmd.to_string(),
                        started_at,
                    };
                    finalize_inline_shell_outcome(
                        repl,
                        &job,
                        ShellJobOutcome::SpawnFailed(format!("failed to spawn worker: {e}")),
                    );
                }
            }
        }
        Err(e) => {
            // Same shape as the legacy synchronous failure path.
            let job = ShellJob {
                rx,
                cancel_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                pgid: None,
                placeholder_idx,
                cmd: shell_cmd.to_string(),
                started_at,
            };
            finalize_inline_shell_outcome(repl, &job, ShellJobOutcome::SpawnFailed(e.to_string()));
        }
    }
}

/// Cancel the in-flight inline `!shell` job from the UI thread (Esc): set
/// the cancel flag, then SIGKILL the whole process group. The worker
/// observes the death and reports `Cancelled`; the main-loop drain then
/// finalizes the placeholder. Safe to call with no job in flight.
pub(crate) fn cancel_inline_shell(repl: &mut Repl) {
    let Some(job) = repl.state.shell_job.as_ref() else {
        return;
    };
    job.cancel_flag
        .store(true, std::sync::atomic::Ordering::SeqCst);
    #[cfg(unix)]
    if let Some(pgid) = job.pgid {
        kill_inline_shell_group(pgid);
    }
}

/// Cancel the in-flight inline `!shell` job as part of a chat clear
/// (review fix): `/clear` wipes the message list, which strands the job's
/// `placeholder_idx` — the worker's asynchronous `Cancelled` report would
/// otherwise finalize at that stale index later and overwrite an
/// UNRELATED message (bounds-checked, so no panic, just wrong). This
/// kills the process group exactly like the Esc path, then finalizes the
/// placeholder SYNCHRONOUSLY — while the index is still valid — and frees
/// the job slot, so the main-loop drain can never touch the wiped list.
/// The worker's own report lands on a dropped receiver and is discarded.
/// Safe to call with no job in flight.
pub(crate) fn cancel_inline_shell_for_clear(repl: &mut Repl) {
    cancel_inline_shell(repl);
    let Some(mut job) = repl.state.shell_job.take() else {
        return;
    };
    // Honor an outcome the worker already delivered (the child may have
    // exited naturally between the last drain and this cancel); otherwise
    // the cancel above IS the outcome.
    let outcome = job.rx.try_recv().ok().unwrap_or(ShellJobOutcome::Cancelled);
    finalize_inline_shell_outcome(repl, &job, outcome);
}

/// Non-blocking drain of finished inline `!shell` jobs — called every main
/// loop iteration. Finalizes the placeholder tool message in place when the
/// worker reports, then frees the single job slot.
pub(crate) fn poll_inline_shell_jobs(repl: &mut Repl) {
    let Some(mut job) = repl.state.shell_job.take() else {
        return;
    };
    match job.rx.try_recv() {
        Ok(outcome) => finalize_inline_shell_outcome(repl, &job, outcome),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
            // Still running — put the job back for the next iteration.
            repl.state.shell_job = Some(job);
        }
        Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
            // Worker died without reporting (panic): surface it instead of
            // leaving the placeholder spinning forever.
            finalize_inline_shell_outcome(
                repl,
                &job,
                ShellJobOutcome::SpawnFailed("worker thread exited unexpectedly".to_string()),
            );
        }
    }
}

/// Worker body for an inline `!shell` job: wait for the child under the
/// given wall-clock budget while helper threads drain stdout/stderr (the
/// same concurrent-drain semantics as `Command::output()`), so a child
/// emitting more than the pipe buffer can never deadlock the wait. Reports
/// exactly one outcome on `tx`.
fn wait_inline_shell_child(
    mut child: std::process::Child,
    timeout_secs: u64,
    cancel_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    tx: tokio::sync::mpsc::UnboundedSender<ShellJobOutcome>,
) {
    let stdout_pipe = child
        .stdout
        .take()
        .map(|p| std::thread::spawn(move || drain_pipe_to_string(p)));
    let stderr_pipe = child
        .stderr
        .take()
        .map(|p| std::thread::spawn(move || drain_pipe_to_string(p)));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = join_pipe_thread(stdout_pipe);
                let stderr = join_pipe_thread(stderr_pipe);
                if cancel_flag.load(std::sync::atomic::Ordering::SeqCst) {
                    let _ = tx.send(ShellJobOutcome::Cancelled);
                } else {
                    let _ = tx.send(ShellJobOutcome::Completed {
                        success: status.success(),
                        exit_code: status.code(),
                        stdout,
                        stderr,
                    });
                }
                return;
            }
            Ok(None) if std::time::Instant::now() >= deadline => {
                kill_inline_shell_job(&mut child);
                let _ = child.wait(); // reap the killed leader
                // Drain what made it through so the helper threads end; the
                // timeout message itself carries no output.
                let _ = join_pipe_thread(stdout_pipe);
                let _ = join_pipe_thread(stderr_pipe);
                let _ = tx.send(ShellJobOutcome::TimedOut { timeout_secs });
                return;
            }
            Ok(None) => std::thread::sleep(INLINE_SHELL_WAIT_POLL),
            Err(e) => {
                let _ = join_pipe_thread(stdout_pipe);
                let _ = join_pipe_thread(stderr_pipe);
                let _ = tx.send(ShellJobOutcome::SpawnFailed(format!(
                    "wait on child failed: {e}"
                )));
                return;
            }
        }
    }
}

/// Read a child output pipe to EOF and decode lossily as UTF-8. Runs on a
/// helper thread so a child producing more than the pipe buffer can keep
/// writing (and therefore exit) while we wait.
fn drain_pipe_to_string<R: std::io::Read + Send + 'static>(mut pipe: R) -> String {
    let mut buf = Vec::new();
    let _ = pipe.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Join an output-drain helper thread; a panicked reader contributes empty
/// output rather than taking the worker down.
fn join_pipe_thread(handle: Option<std::thread::JoinHandle<String>>) -> String {
    handle.and_then(|h| h.join().ok()).unwrap_or_default()
}

/// Kill an inline-shell job: SIGKILL the whole process group — the child was
/// spawned as a group leader via `process_group(0)`, so descendants (which
/// may hold the output pipes open) die with it. Mirrors the eval runner's
/// `kill_process_tree` semantics.
#[cfg(unix)]
fn kill_inline_shell_job(child: &mut std::process::Child) {
    let negated = -(child.id() as i32);
    // SAFETY: kill(2) with SIGKILL on our own spawned process group; ESRCH
    // for an already-dead group is an acceptable non-error.
    if unsafe { libc::kill(negated, libc::SIGKILL) } != 0 {
        // Group already gone or never set — fall back to the direct child.
        let _ = child.kill();
    }
}

/// Non-unix builds have no process groups; terminate the direct child.
#[cfg(not(unix))]
fn kill_inline_shell_job(child: &mut std::process::Child) {
    let _ = child.kill();
}

/// SIGKILL a spawned process group by id (the child is its group leader).
#[cfg(unix)]
fn kill_inline_shell_group(pgid: u32) {
    let negated = -(pgid as i32);
    // SAFETY: kill(2) with SIGKILL on our own spawned process group; ESRCH
    // for an already-dead group is an acceptable non-error.
    let _ = unsafe { libc::kill(negated, libc::SIGKILL) };
}

/// Combine stdout/stderr into the tool-message body the synchronous path has
/// always produced: stdout, then a `[stderr]` section, or an exit-code line
/// when both are empty.
fn format_inline_shell_message(
    cmd: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
) -> String {
    let mut parts = String::new();
    if !stdout.is_empty() {
        parts.push_str(stdout);
    }
    if !stderr.is_empty() {
        if !parts.is_empty() {
            parts.push('\n');
        }
        parts.push_str("[stderr]\n");
        parts.push_str(stderr);
    }
    if parts.is_empty() {
        format!("$ {cmd}\n(exit {})", exit_code.unwrap_or(-1))
    } else {
        format!("$ {cmd}\n{parts}")
    }
}

/// Replace the placeholder tool message in place with the job's final
/// content, preserving the legacy formatting and error marking.
fn finalize_inline_shell_outcome(repl: &mut Repl, job: &ShellJob, outcome: ShellJobOutcome) {
    let (content, is_error, exit_code) = match outcome {
        ShellJobOutcome::Completed {
            success,
            exit_code,
            stdout,
            stderr,
        } => (
            format_inline_shell_message(&job.cmd, &stdout, &stderr, exit_code),
            !success,
            exit_code,
        ),
        ShellJobOutcome::TimedOut { timeout_secs } => (
            format!(
                "$ {}\n(timeout after {timeout_secs}s — process group killed)",
                job.cmd
            ),
            true,
            None,
        ),
        ShellJobOutcome::Cancelled => (
            format!("$ {}\n(cancelled — Esc killed the process group)", job.cmd),
            true,
            None,
        ),
        ShellJobOutcome::SpawnFailed(e) => {
            (format!("$ {}\nFailed to execute: {e}", job.cmd), true, None)
        }
    };

    // The message list is a public field; set the completion fields first,
    // then `update_message` re-syncs the (private) renderable cell with the
    // full message — its content/timestamp writes land on the same values.
    let now = chrono::Utc::now();
    if let Some(msg) = repl.chat.messages.get_mut(job.placeholder_idx) {
        msg.content = content.clone();
        msg.timestamp = now;
        msg.is_error = is_error;
        msg.exit_code = exit_code;
        msg.duration_secs = Some((now - job.started_at).num_milliseconds() as f64 / 1000.0);
    }
    repl.chat.update_message(job.placeholder_idx, content);
}

/// Submit the current input
pub fn submit_input(repl: &mut Repl, mut terminal: Option<&mut super::query::Term>) -> Result<()> {
    let raw_input = repl.prompt.input().to_string();

    if raw_input.trim().is_empty() {
        return Ok(());
    }

    // Detect URLs in input for potential @-reference expansion
    if let Some(_url) = crate::repl::at_reference::detect_url_in_input(&raw_input) {
        tracing::debug!(url = %_url, "URL detected in input");
    }

    // Expand pasted text references: [Pasted Text #N X lines] -> actual content
    let expanded = expand_pasted_texts(&raw_input, &mut repl.state.pasted_texts);

    // Add user message to chat. Redact inline secrets (e.g. an API key passed
    // to /connect) so the plaintext never lands in the chat widget or the
    // session JSON written by save_session. The unredacted `expanded` below is
    // what the command handler actually receives.
    let chat_text = redact_secret_command(&raw_input);
    repl.chat.add_message(ChatRole::User, chat_text);

    // Increment turn counter for context visualization
    repl.state.turn_count += 1;

    // Push to command history (up-arrow recall). Redact the same way so a
    // recalled command can't leak the key either.
    let history_entry = redact_secret_command(&expanded);
    repl.command_history.push(&history_entry);
    repl.saved_input.clear();
    repl.prompt.clear();

    // Clear paste state for next input
    repl.state.pasted_texts.clear();
    repl.state.paste_counter = 0;

    // Process command or query with expanded text
    if expanded.starts_with('!') {
        // Inline shell execution: "!command" or "! command". Launched in the
        // background (P0-1) — the event loop stays responsive and the
        // completion lands via poll_inline_shell_jobs in the main loop.
        let shell_cmd = expanded.trim_start_matches('!').trim();
        if !shell_cmd.is_empty() {
            start_inline_shell(repl, shell_cmd, inline_shell_timeout_from_env());
        }
    } else if expanded.starts_with('/') {
        repl.commands_run += 1;
        handle_command(repl, &expanded)?;
    } else {
        super::query::handle_query(repl, &expanded, &mut terminal)?;
    }

    // Drain queued follow-up messages in a flat loop.
    // This avoids recursive handle_query calls that could leave the
    // query engine unavailable.
    while !repl.state.queued_messages.is_empty() {
        let queued = repl.state.queued_messages.remove(0);
        if queued.trim().is_empty() {
            continue;
        }
        repl.state.toast = Some((
            "Sending queued message…".to_string(),
            std::time::Instant::now(),
        ));
        submit_input_with_text(repl, &queued, &mut terminal);
    }

    Ok(())
}

/// Submit pre-formed text as if the user typed and entered it.
/// Used for queued follow-up messages.
pub fn submit_input_with_text(
    repl: &mut Repl,
    text: &str,
    terminal: &mut Option<&mut super::query::Term>,
) {
    let expanded = expand_pasted_texts(text, &mut repl.state.pasted_texts);
    // Redact inline secrets (e.g. /connect <provider> <key>) before recording
    // — same contract as submit_input. The unredacted `expanded` is executed.
    let chat_text = redact_secret_command(text);
    repl.chat.add_message(ChatRole::User, chat_text);
    repl.state.turn_count += 1;
    let history_entry = redact_secret_command(&expanded);
    repl.command_history.push(&history_entry);
    repl.prompt.clear();
    repl.state.pasted_texts.clear();
    repl.state.paste_counter = 0;

    if expanded.starts_with('!') {
        // Inline shell execution — same background launch as submit_input
        // (P0-1); a queued "!" line must not block the UI either.
        let shell_cmd = expanded.trim_start_matches('!').trim();
        if !shell_cmd.is_empty() {
            start_inline_shell(repl, shell_cmd, inline_shell_timeout_from_env());
        }
    } else if expanded.starts_with('/') {
        repl.commands_run += 1;
        if let Err(e) = handle_command(repl, &expanded) {
            repl.chat
                .add_message(ChatRole::System, format!("Error: {e}"));
        }
    } else if let Err(e) = super::query::handle_query(repl, &expanded, terminal) {
        repl.chat
            .add_message(ChatRole::System, format!("Error: {e}"));
    }
}

/// Handle a command (starts with /)
pub fn handle_command(repl: &mut Repl, input: &str) -> Result<()> {
    let parsed = match repl.command_parser.parse(input) {
        Ok(p) => p,
        Err(_) => {
            let parts: Vec<&str> = input.splitn(2, ' ').collect();
            let name = parts
                .first()
                .copied()
                .unwrap_or("")
                .strip_prefix('/')
                .unwrap_or("");
            shannon_commands::ParsedCommand::new(
                name.to_string(),
                parts.get(1).copied().unwrap_or("").to_string(),
                input.to_string(),
            )
        }
    };

    let cmd_name = parsed.name.as_str();
    let args = parsed.args.as_str();

    // Check if command exists in the registry
    let command_exists = repl.runtime.block_on(async {
        repl.shared_executor
            .registry()
            .await
            .contains(cmd_name)
            .await
    });
    // Commands handled in the match block but not in the global registry
    let repl_only_commands = [
        "help",
        "clear",
        "quit",
        "exit",
        "model",
        "models",
        "provider",
        "prov",
        "init",
        "config",
        "connect",
        "disconnect",
        "sessions",
        "resume",
        "history",
        "worktree",
        "credentials",
        "creds",
        "cred",
        "status",
        "st",
        "git-status",
        "export",
        "save",
        "import",
        "load",
        "diff",
        "search",
        "?",
        "hist",
        "history-search",
        "find",
        "grep",
        "conv-search",
        "browse",
        "files",
        "select-tools",
        "tools",
        "notools",
        "debug",
        "dbg",
        "dev",
        "doctor",
        "check",
        "diagnostics",
        "terminal-setup",
        "compact",
        "handoff",
        "cost",
        "billing",
        "usage",
        "suggest",
        "permissions",
        "perms",
        "perm",
        "plan",
        "team",
        "agents",
        "agent",
        "route",
        "mcp",
        "branch",
        "fork",
        "web-search",
        "websearch",
        "search-web",
        "review",
        "stage",
        "stats",
        "perf",
        "loop",
        "ralph",
        "sandbox",
        "local-models",
        "local",
        "ci",
        "gh-actions",
        "hooks",
        "remember",
        "mem",
        "memo",
        "recall",
        "search-memory",
        "forget",
        "memory",
        "image",
        "img",
        "browser",
        "screenshot",
        "mode",
        "context",
        "undo",
        "rewind",
        "remote",
        "checkpoint",
        "notify",
        "webhook",
        "routine",
        "schedule",
        "cron",
        "create-pr",
        "patch",
        "copy",
        "clip",
        "paste",
        "add",
        "add-dir",
        "adddir",
        "watch",
        "bind",
        "project",
        "theme",
        "session",
        "rename",
        "recap",
        "effort",
        "focus",
        "goal",
        "accessibility",
        "a11y",
        "color",
        "diag",
        "commands",
        "statusline",
        "lang",
        "language",
    ];
    let is_repl_command = repl_only_commands.contains(&cmd_name);

    if command_exists || is_repl_command {
        match cmd_name {
            "help" => handle_help(repl, args)?,
            "clear" => handle_clear(repl)?,
            "quit" | "exit" => handle_quit(repl)?,
            "model" | "models" => config::handle_model(repl, args)?,
            "provider" | "prov" => config::handle_provider(repl, args)?,
            "init" => config::handle_init(repl)?,
            "config" => config::handle_config(repl, args)?,
            "connect" => config::handle_connect(repl, args)?,
            "disconnect" => config::handle_disconnect(repl, args)?,
            "sessions" => session::handle_sessions(repl, args)?,
            "resume" => session::handle_resume(repl, args)?,
            "history" => session::handle_history(repl, args)?,
            "worktree" => git::handle_worktree(repl, args)?,
            "credentials" | "creds" | "cred" => extensions::handle_credentials(repl, args)?,
            "status" | "st" | "git-status" => git::handle_status(repl, args)?,
            "export" | "save" => file_ops::handle_export(repl, args)?,
            "import" | "load" => file_ops::handle_import(repl, args)?,
            "diff" => git::handle_diff(repl, args)?,
            // /search spans every stored session; ?, /hist, /history-search
            // stay command-history search.
            "search" => session::handle_search(repl, args)?,
            "?" | "hist" | "history-search" => file_ops::handle_search(repl, args)?,
            "find" | "grep" | "conv-search" => file_ops::handle_find(repl, args)?,
            "browse" | "files" => media::handle_browse(repl, args)?,
            "notools" => {
                repl.state.tools_enabled = false;
                repl.chat
                    .add_message(ChatRole::System, t!("repl.tools_disabled").to_string());
            }
            "select-tools" | "tools" => {
                if !repl.state.tools_enabled {
                    repl.state.tools_enabled = true;
                    repl.chat
                        .add_message(ChatRole::System, t!("repl.tools_enabled").to_string());
                } else {
                    debug::handle_select_tools(repl)?;
                }
            }
            "debug" | "dbg" | "dev" => debug::handle_debug(repl, args)?,
            "doctor" | "check" | "diagnostics" => debug::handle_doctor(repl, args)?,
            "terminal-setup" => config::handle_terminal_setup(repl)?,
            "compact" => session::handle_compact(repl, args)?,
            "handoff" => session::handle_handoff(repl, args)?,
            "cost" => cost::handle_cost(repl, args)?,
            "billing" | "usage" => cost::handle_billing(repl, args)?,
            "suggest" => cost::handle_suggest(repl, args)?,
            // R1-6 (decision ② step 1): /permissions is the first-class home
            // of the permission-profile command — the same handler /profile
            // resolves to. The tool allow/deny/status view keeps its short
            // aliases /perms and /perm.
            "permissions" => handle_permissions_profiles(repl, args)?,
            "perms" | "perm" => cost::handle_permissions(repl, args)?,
            // /profile keeps working during the naming transition (R1-6) but
            // warns once per session that permission profiles now live at
            // /permissions; in a future release it becomes the provider
            // profile command (/profiles).
            "profile" => {
                maybe_profile_migration_hint(repl);
                handle_other_command(repl, "profile", args)?;
            }
            "plan" => session::handle_plan(repl, args)?,
            "team" => extensions::handle_team(repl, args)?,
            "agents" => extensions::handle_agents(repl, args)?,
            "agent" => loop_engine::handle_agent(repl, args)?,
            "route" => extensions::handle_route(repl, args)?,
            "mcp" => extensions::handle_mcp(repl, args)?,
            "branch" | "fork" => session::handle_branch(repl, args)?,
            "web-search" | "websearch" | "search-web" => web::handle_web_search(repl, args)?,
            "review" => git::handle_review(repl, args)?,
            "stage" => git::handle_stage(repl, args)?,
            "stats" | "perf" => loop_engine::handle_stats(repl)?,
            "loop" => loop_engine::handle_loop(repl, args)?,
            "ralph" => loop_engine::handle_ralph(repl, args)?,
            "sandbox" => loop_engine::handle_sandbox(repl, args)?,
            "local-models" | "local" => config::handle_local_models(repl)?,
            "ci" | "gh-actions" => git::handle_ci(repl, args)?,
            "hooks" => hooks::handle_hooks(repl, args)?,
            "remember" | "mem" | "memo" => memory::handle_remember(repl, args)?,
            "recall" | "search-memory" => memory::handle_recall(repl, args)?,
            "forget" => memory::handle_forget(repl, args)?,
            "memory" => memory::handle_memory(repl, args)?,
            "image" | "img" | "screenshot" => media::handle_image(repl, args)?,
            "browser" => browser::handle_browser(repl, args)?,
            "mode" => config::handle_mode(repl, args)?,
            "context" => config::handle_context(repl, args)?,
            "undo" => session::handle_undo(repl, args)?,
            "rewind" | "checkpoint" => session::handle_rewind(repl, args)?,
            "remote" => remote::handle_remote(repl, args)?,
            "notify" => web::handle_notify(repl, args)?,
            "webhook" => web::handle_webhook(repl, args)?,
            "routine" => loop_engine::handle_routine(repl, args)?,
            "schedule" | "cron" => loop_engine::handle_schedule(repl, args)?,
            "create-pr" => git::handle_create_pr(repl, args)?,
            "patch" => git::handle_patch(repl, args)?,
            "copy" | "clip" => media::handle_copy(repl, args)?,
            "paste" => media::handle_paste(repl)?,
            "add" => file_ops::handle_add(repl, args)?,
            "add-dir" | "adddir" => file_ops::handle_add_dir(repl, args)?,
            "watch" => file_ops::handle_watch(repl, args)?,
            "bind" => loop_engine::handle_bind(repl, args)?,
            "project" => loop_engine::handle_project(repl, args)?,
            "theme" => config::handle_theme(repl, args)?,
            "session" => session::handle_session(repl, args)?,
            "rename" => session::handle_rename(repl, args)?,
            "recap" => session::handle_recap(repl, args)?,
            "effort" => session::handle_effort(repl, args)?,
            "focus" => session::handle_focus(repl, args)?,
            "goal" => goal::handle_goal(repl, args)?,
            "accessibility" | "a11y" => config::handle_accessibility(repl, args)?,
            "color" => config::handle_color(repl, args)?,
            "diag" => debug::handle_diag(repl, args)?,
            "commands" => hooks::handle_commands(repl, args)?,
            "statusline" => config::handle_statusline(repl, args)?,
            "lang" | "language" => config::handle_lang(repl, args)?,
            _ => handle_other_command(repl, cmd_name, args)?,
        }
        Ok(())
    } else {
        repl.chat.add_message(
            ChatRole::System,
            t!("repl.unknown_command", name = cmd_name).to_string(),
        );
        Ok(())
    }
}

fn handle_help(repl: &mut Repl, args: &str) -> Result<()> {
    use crate::repl::state::HelpOverlayState;
    let filter = if args.is_empty() {
        None
    } else {
        Some(args.trim().to_string())
    };
    repl.state.help_overlay = Some(HelpOverlayState {
        filter,
        ..Default::default()
    });
    Ok(())
}

fn handle_clear(repl: &mut Repl) -> Result<()> {
    // Review fix: cancelling here (both branches) kills an in-flight
    // `!shell` job's process group and finalizes its placeholder while
    // the index is still valid — after the wipe below, the worker's
    // asynchronous report would finalize at the stranded placeholder
    // index and overwrite an unrelated message.
    cancel_inline_shell_for_clear(repl);
    if repl.chat.len() > 1 {
        repl.show_confirm_dialog(
            "Clear Chat",
            "Clear all messages? This cannot be undone.",
            "clear_chat",
        );
    } else {
        repl.chat.clear();
        repl.chat
            .add_message(ChatRole::System, t!("repl.chat_cleared").to_string());
        if let Some(ref mut engine) = repl.query_engine {
            engine.new_session();
        }
        repl.current_turn = 0;
        repl.state.tokens_used = 0;
    }
    Ok(())
}

fn handle_quit(repl: &mut Repl) -> Result<()> {
    repl.running = false;
    Ok(())
}

/// `/permissions` — first-class alias of the `/profile` permission-profile
/// command (R1-6, decision ② step 1: permission management lives at
/// /permissions, matching the Claude Code ecosystem; in a future release
/// /profile itself switches to the provider profiles command, /profiles).
/// Dispatches through the same registry path as /profile. The tool
/// allow/deny/status view that previously owned this name remains reachable
/// via its aliases `/perms` and `/perm`.
fn handle_permissions_profiles(repl: &mut Repl, args: &str) -> Result<()> {
    handle_other_command(repl, "profile", args)
}

/// One-time `/profile` migration hint (R1-6): on the first `/profile` of a
/// REPL session, print a note above the command's output that permission
/// profiles moved to `/permissions`. Never repeated.
fn maybe_profile_migration_hint(repl: &mut Repl) {
    if repl.state.profile_migration_hint_shown {
        return;
    }
    repl.state.profile_migration_hint_shown = true;
    repl.chat.add_message(
        ChatRole::System,
        t!("commands.profile.migration_hint").to_string(),
    );
}

fn handle_other_command(repl: &mut Repl, cmd_name: &str, args: &str) -> Result<()> {
    let registry = repl.runtime.block_on(repl.shared_executor.registry());
    if let Ok(command) = repl.runtime.block_on(registry.get(cmd_name)) {
        match &*command {
            shannon_commands::Command::Prompt(prompt_cmd) => {
                if let Some(ref template) = prompt_cmd.prompt_template {
                    let args_val = if args.is_empty() { "" } else { args };
                    let arg_parts: Vec<&str> = args_val.split_whitespace().collect();
                    let mut prompt = template.clone();
                    // Replace indexed placeholders: $ARGUMENTS[0], $ARGUMENTS[1], ...
                    for (i, part) in arg_parts.iter().enumerate() {
                        prompt = prompt.replace(&format!("$ARGUMENTS[{i}]"), part);
                    }
                    // Also replace {args[0]}, {args[1]}, ...
                    for (i, part) in arg_parts.iter().enumerate() {
                        prompt = prompt.replace(&format!("{{args[{i}]}}"), part);
                    }
                    // Replace full placeholders last (so indexed ones take priority)
                    prompt = prompt
                        .replace("$ARGUMENTS", args_val)
                        .replace("{args}", args_val);
                    // Expand built-in template variables
                    prompt = prompt.replace(
                        "$DIR",
                        &std::env::current_dir()
                            .unwrap_or_default()
                            .display()
                            .to_string(),
                    );
                    prompt = prompt.replace(
                        "$DATE",
                        &chrono::Local::now().format("%Y-%m-%d").to_string(),
                    );
                    prompt = prompt.replace(
                        "$TIME",
                        &chrono::Local::now().format("%H:%M:%S").to_string(),
                    );

                    // Run native pre-analysis for supported commands
                    let native_context = match cmd_name {
                        "diff" | "git-diff" => {
                            Some(shannon_commands::diff_utils::run_diff_analysis(args_val))
                        }
                        "review-pr" | "pr-review" => {
                            Some(shannon_commands::review_utils::run_pr_analysis(args_val))
                        }
                        _ => None,
                    };

                    if let Some(ref analysis) = native_context {
                        prompt = format!(
                            "{analysis}\n\n---\n\nBased on the above native analysis, provide additional insights:\n\n{prompt}"
                        );
                    }

                    repl.chat
                        .add_message(ChatRole::System, format!("Running /{cmd_name}..."));
                    super::query::handle_query(repl, &prompt, &mut None)?;
                } else {
                    repl.chat.add_message(
                        ChatRole::System,
                        format!("/{cmd_name} — {}", prompt_cmd.base.description),
                    );
                }
            }
            _ => {
                let desc = command.description();
                repl.chat
                    .add_message(ChatRole::System, format!("/{cmd_name} — {desc}"));
            }
        }
    }
    Ok(())
}

/// Execute a pending dialog action after confirmation
pub fn execute_pending_action(repl: &mut Repl, action: &str) -> Result<()> {
    match action {
        "clear_chat" => {
            // Same discipline as handle_clear (review fix): a job started
            // (or still lingering) between the confirm dialog opening and
            // this confirmation must not finalize into the wiped list.
            cancel_inline_shell_for_clear(repl);
            repl.chat.clear();
            repl.chat
                .add_message(ChatRole::System, t!("repl.chat_cleared").to_string());
            if let Some(ref mut engine) = repl.query_engine {
                engine.new_session();
            }
            repl.current_turn = 0;
            repl.state.tokens_used = 0;
        }
        "quit" => {
            repl.running = false;
        }
        "set_bypass_mode" => {
            if let Some(ref query_engine) = repl.query_engine {
                let mut perms = recover_lock(query_engine.permissions().write());
                perms.set_approval_mode(
                    shannon_engine::permissions::ApprovalMode::BypassPermissions,
                );
                drop(perms);
                repl.state.approval_mode_label = "FULL".to_string();
                repl.state.status = "Mode: FULL".to_string();
                repl.state.toast = Some(("  Mode: FULL  ".to_string(), std::time::Instant::now()));
                repl.chat.add_message(
                    ChatRole::System,
                    "Permission bypass enabled — all checks skipped.".to_string(),
                );
            }
        }
        _ => {}
    }
    Ok(())
}

// Helper trait methods on Repl for dialog display
impl Repl {
    pub(crate) fn show_confirm_dialog(&mut self, title: &str, message: &str, action: &str) {
        use crate::widgets::dialog::ConfirmDialog;
        let dialog = ConfirmDialog::new(title.to_string())
            .with_message(message.to_string())
            .build();
        self.state.active_dialog = Some(dialog);
        self.state.pending_dialog_action = Some(action.to_string());
    }

    pub(crate) fn show_input_dialog(&mut self, title: &str, placeholder: &str, action: &str) {
        use crate::widgets::dialog::InputDialog;
        let dialog = InputDialog::new(title.to_string()).with_placeholder(placeholder.to_string());
        self.state.input_dialog = Some(Box::new(dialog));
        self.state.input_dialog_action = Some(action.to_string());
    }

    pub(crate) fn show_alert_dialog(&mut self, title: &str, message: &str, danger: bool) {
        use crate::widgets::dialog::AlertDialog;
        let mut builder = AlertDialog::new(title.to_string()).with_message(message.to_string());
        if danger {
            builder = builder.with_danger();
        }
        self.state.active_dialog = Some(builder.build());
        self.state.pending_dialog_action = None;
    }
}

#[cfg(test)]
mod tests {
    use super::redact_secret_command;

    #[test]
    fn redact_connect_key_replaces_inline_key_with_marker() {
        // The plaintext key must never appear in the recorded form.
        let out = redact_secret_command("/connect minimax sk-secret-12345");
        assert_eq!(out, "/connect minimax ***");
        assert!(!out.contains("sk-secret-12345"));
    }

    #[test]
    fn redact_connect_key_preserves_provider_casing_and_leading_whitespace() {
        // Provider echoes back as-typed; leading whitespace is preserved.
        let out = redact_secret_command("   /connect MiniMax abc-KEY-xyz");
        assert_eq!(out, "   /connect MiniMax ***");
    }

    #[test]
    fn redact_connect_key_is_case_insensitive_on_command_name() {
        // Command name matching is case-insensitive; the recorded form is
        // normalized to the canonical lowercase `/connect`.
        let out = redact_secret_command("/CONNECT anthropic sk-ant-9");
        assert_eq!(out, "/connect anthropic ***");
        assert!(!out.contains("sk-ant-9"));
    }

    #[test]
    fn redact_connect_key_handles_whitespace_runs_like_parser() {
        // The real /connect parser (parse_connect_args) treats runs of
        // whitespace as a single separator, so the redactor must too —
        // otherwise a double-spaced key would leak into history verbatim.
        let out = redact_secret_command("/connect    minimax    sk-secret");
        assert_eq!(out, "/connect minimax ***");
        assert!(!out.contains("sk-secret"));
        // Tab-separated form is also covered by split_whitespace.
        assert_eq!(
            redact_secret_command("/connect\tminimax\tsk-secret"),
            "/connect minimax ***"
        );
    }

    #[test]
    fn redact_connect_without_key_is_unchanged() {
        // No inline key → nothing to redact. Must not fabricate a `***`.
        assert_eq!(
            redact_secret_command("/connect minimax"),
            "/connect minimax"
        );
        assert_eq!(redact_secret_command("/connect"), "/connect");
        // A blank key argument is treated as "no key".
        assert_eq!(
            redact_secret_command("/connect minimax "),
            "/connect minimax "
        );
    }

    #[test]
    fn redact_credentials_store_replaces_value_with_marker() {
        // The plaintext value must never appear in the recorded form (review
        // P0-5: /credentials used to echo the raw secret into chat, history,
        // and session JSON).
        let out = redact_secret_command("/credentials store anthropic sk-ant-secret-9");
        assert_eq!(out, "/credentials store anthropic ***");
        assert!(!out.contains("sk-ant-secret-9"));
    }

    #[test]
    fn redact_credentials_aliases_normalize_and_redact() {
        // Every alias resolves to Store in the handler, so all of them redact;
        // the recorded form is the canonical `/credentials store`.
        assert_eq!(
            redact_secret_command("/creds store svc sk-123"),
            "/credentials store svc ***"
        );
        assert_eq!(
            redact_secret_command("/cred add svc sk-123"),
            "/credentials store svc ***"
        );
        assert_eq!(
            redact_secret_command("/CREDENTIALS set svc tok_abc"),
            "/credentials store svc ***"
        );
    }

    #[test]
    fn redact_credentials_preserves_service_and_leading_whitespace() {
        let out = redact_secret_command("   /credentials store GitHub ghp-XyZ123");
        assert_eq!(out, "   /credentials store GitHub ***");
        assert!(!out.contains("ghp-XyZ123"));
    }

    #[test]
    fn redact_credentials_non_store_subcommands_unchanged() {
        // get/list/delete/count carry no inline secret.
        assert_eq!(
            redact_secret_command("/credentials get svc"),
            "/credentials get svc"
        );
        assert_eq!(
            redact_secret_command("/credentials delete svc"),
            "/credentials delete svc"
        );
        // `store` without a value has nothing to redact — no fabricated `***`.
        assert_eq!(
            redact_secret_command("/credentials store svc"),
            "/credentials store svc"
        );
    }

    #[test]
    fn redact_leaves_other_commands_and_free_text_untouched() {
        assert_eq!(redact_secret_command("/model gpt-4o"), "/model gpt-4o");
        assert_eq!(
            redact_secret_command("how do I parse JSON?"),
            "how do I parse JSON?"
        );
        // Inline shell, env-var dumps, etc. are not /connect.
        assert_eq!(redact_secret_command("!echo $HOME"), "!echo $HOME");
    }

    /// Regression guard for the `/notools` dispatch drift (2026-08-28
    /// review PM-1): the `match cmd_name` arm existed, but neither
    /// `repl_only_commands` nor the builtin registry provided the name, so
    /// the gate rejected `/notools` as an unknown command. Every alias
    /// group in the dispatch match must be reachable through at least one
    /// alias: the repl-only list or a registered builtin (name or alias).
    #[test]
    fn every_dispatch_match_arm_is_reachable_from_the_gate() {
        let src = include_str!("mod.rs");

        // Pull the repl-only list body out of this very file so the check
        // can never drift from the compiled list.
        let list_start = src
            .find("let repl_only_commands = [")
            .expect("repl_only_commands array present");
        let list_slice = &src[list_start..];
        let list_end = list_slice
            .find("];")
            .expect("repl_only_commands array terminated");
        let list: std::collections::HashSet<String> = list_slice[..list_end]
            .split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect();
        assert!(
            list.contains("notools"),
            "sanity: the fixed drift entry must stay in the list"
        );

        // Bound the `match cmd_name { ... }` block with a brace-counting
        // scan that skips string literals (arm bodies carry format strings).
        let match_start = src
            .find("match cmd_name {")
            .expect("dispatch match present");
        let bytes = src.as_bytes();
        let mut depth = 0i32;
        let mut in_string = false;
        let mut cursor = match_start;
        let mut match_end = None;
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'"' if !in_string => in_string = true,
                b'"' => in_string = false,
                b'\\' if in_string => {
                    cursor += 1; // skip escaped character
                }
                b'{' if !in_string => depth += 1,
                b'}' if !in_string => {
                    depth -= 1;
                    if depth == 0 {
                        match_end = Some(cursor);
                        break;
                    }
                }
                _ => {}
            }
            cursor += 1;
        }
        let match_body = &src[match_start..match_end.expect("dispatch match terminated")];

        // Collect alias groups from arm heads at the outermost arm level
        // (`"a" | "b" => ...`, i.e. brace depth 1 of the dispatch match).
        // Nested matches sit deeper and are ignored.
        let mut groups: Vec<Vec<String>> = Vec::new();
        let mut line_depth = 0i32;
        for line in match_body.lines() {
            let mut opens = 0i32;
            let mut closes = 0i32;
            let mut chars = line.chars().peekable();
            let mut line_in_string = false;
            while let Some(ch) = chars.next() {
                match ch {
                    '"' => line_in_string = !line_in_string,
                    '\\' if line_in_string => {
                        chars.next();
                    }
                    '{' if !line_in_string => opens += 1,
                    '}' if !line_in_string => closes += 1,
                    _ => {}
                }
            }
            let trimmed = line.trim_start();
            if line_depth == 1
                && !line_in_string
                && trimmed.starts_with('"')
                && trimmed.contains("=>")
            {
                let head = &trimmed[..trimmed.find("=>").expect("arrow checked")];
                let aliases: Vec<String> = head
                    .split('"')
                    .skip(1)
                    .step_by(2)
                    .map(str::to_string)
                    .collect();
                if !aliases.is_empty() {
                    groups.push(aliases);
                }
            }
            line_depth += opens - closes;
        }

        // Non-vacuous coverage: known-first and known-drifted arms.
        let has = |name: &str| groups.iter().any(|g| g.iter().any(|a| a == name));
        assert!(has("help"), "dispatch arm extraction failed (no `help`)");
        assert!(
            has("notools"),
            "dispatch arm extraction failed (no `notools`)"
        );

        let registry_names: std::collections::HashSet<String> =
            shannon_commands::builtin_commands::all_commands()
                .iter()
                .flat_map(|command| {
                    let mut names = vec![command.name().to_string()];
                    names.extend(command.aliases().iter().cloned());
                    names
                })
                .collect();

        let unreachable: Vec<String> = groups
            .into_iter()
            .filter(|group| {
                !group
                    .iter()
                    .any(|alias| list.contains(alias) || registry_names.contains(alias))
            })
            .flatten()
            .collect();
        assert!(
            unreachable.is_empty(),
            "dispatch arms unreachable from the gate (add to repl_only_commands \
             or the builtin registry): {unreachable:?}"
        );
    }
}

// ── Inline `!shell` (P0-1) tests — real `/bin/sh`, per repo precedent ──
#[cfg(test)]
mod inline_shell_tests {
    use super::{
        INLINE_SHELL_DEFAULT_TIMEOUT_SECS, cancel_inline_shell, format_inline_shell_message,
        inline_shell_timeout_from_env, poll_inline_shell_jobs, start_inline_shell,
    };
    use crate::repl::Repl;
    use std::time::{Duration, Instant};

    /// Drive the main-loop drain until the job reports, bounded by `budget`.
    fn wait_for_job(repl: &mut Repl, budget: Duration) -> bool {
        let began = Instant::now();
        while began.elapsed() < budget {
            poll_inline_shell_jobs(repl);
            if repl.state.shell_job.is_none() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// Poll /proc/<pid> until the process is gone (SIGKILL is asynchronous).
    #[cfg(unix)]
    fn wait_pid_gone(pid: u32, budget: Duration) -> bool {
        let began = Instant::now();
        while began.elapsed() < budget {
            if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn inline_shell_completes_and_captures_stdout_in_place() {
        let mut repl = Repl::new().expect("test repl");
        start_inline_shell(&mut repl, "echo shannon-inline-stdout", 10);
        assert!(repl.state.shell_job.is_some(), "job must be in flight");
        let idx = repl.chat.message_count() - 1;
        // Placeholder occupies the tool slot immediately, before completion.
        assert!(
            repl.chat.messages[idx]
                .content
                .contains("running, Esc to cancel"),
            "placeholder expected, got: {:?}",
            repl.chat.messages[idx].content
        );

        assert!(
            wait_for_job(&mut repl, Duration::from_secs(5)),
            "job completed"
        );
        let msg = &repl.chat.messages[idx];
        assert_eq!(
            msg.content,
            "$ echo shannon-inline-stdout\nshannon-inline-stdout\n"
        );
        assert!(!msg.is_error);
        assert_eq!(msg.exit_code, Some(0));
        // Finalized in place: no extra message was appended.
        assert_eq!(repl.chat.message_count(), idx + 1);
    }

    #[test]
    fn inline_shell_captures_stderr_section_and_exit_fallback() {
        let mut repl = Repl::new().expect("test repl");
        start_inline_shell(&mut repl, "echo out-line; echo err-line 1>&2", 10);
        assert!(
            wait_for_job(&mut repl, Duration::from_secs(5)),
            "job completed"
        );
        let msg = &repl.chat.messages[repl.chat.message_count() - 1];
        // "out-line\n" keeps its trailing newline, so the section separator
        // doubles it — identical to the legacy synchronous path.
        assert_eq!(
            msg.content,
            "$ echo out-line; echo err-line 1>&2\nout-line\n\n[stderr]\nerr-line\n"
        );

        // Empty output falls back to the exit-code line, like the old path.
        let mut repl = Repl::new().expect("test repl");
        start_inline_shell(&mut repl, "true", 10);
        assert!(
            wait_for_job(&mut repl, Duration::from_secs(5)),
            "job completed"
        );
        let msg = &repl.chat.messages[repl.chat.message_count() - 1];
        assert_eq!(msg.content, "$ true\n(exit 0)");
    }

    #[test]
    #[cfg(unix)]
    fn inline_shell_timeout_kills_child_and_posts_notice() {
        let mut repl = Repl::new().expect("test repl");
        let began = Instant::now();
        start_inline_shell(&mut repl, "sleep 30", 1);
        let pid = repl
            .state
            .shell_job
            .as_ref()
            .expect("job in flight")
            .pgid
            .expect("unix spawn has a pgid");

        // Injected 1s budget: the whole job must settle well under 2s wall.
        assert!(
            wait_for_job(&mut repl, Duration::from_secs(2)),
            "job returned within ~2s"
        );
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "timeout enforced early"
        );
        assert!(
            wait_pid_gone(pid, Duration::from_secs(2)),
            "child process must be gone after timeout kill"
        );
        let msg = &repl.chat.messages[repl.chat.message_count() - 1];
        assert!(
            msg.content.contains("timeout after 1s"),
            "notice: {:?}",
            msg.content
        );
        assert!(msg.is_error);
    }

    #[test]
    #[cfg(unix)]
    fn esc_cancel_kills_the_whole_process_group() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let pidfile = dir.path().join("sleep.pid");
        // sh spawns a grandchild sleep; the pidfile records it so the test
        // can verify the group kill took the descendant down too.
        let cmd = format!("sleep 30 & echo $! > {}; wait", pidfile.display());
        let mut repl = Repl::new().expect("test repl");
        start_inline_shell(&mut repl, &cmd, 30);

        // Wait until the inner sleep exists.
        let began = Instant::now();
        let sleep_pid: u32 = loop {
            if let Ok(text) = std::fs::read_to_string(&pidfile) {
                if let Ok(pid) = text.trim().parse() {
                    break pid;
                }
            }
            assert!(
                began.elapsed() < Duration::from_secs(5),
                "grandchild sleep spawned"
            );
            std::thread::sleep(Duration::from_millis(10));
        };

        // The Esc path: kill the group from the UI thread.
        cancel_inline_shell(&mut repl);
        assert!(
            wait_pid_gone(sleep_pid, Duration::from_secs(2)),
            "grandchild sleep must die with the process group"
        );

        // The worker observed the kill and the placeholder was finalized.
        assert!(
            wait_for_job(&mut repl, Duration::from_secs(2)),
            "job settled after cancel"
        );
        let msg = &repl.chat.messages[repl.chat.message_count() - 1];
        assert!(
            msg.content.contains("cancelled"),
            "notice: {:?}",
            msg.content
        );
    }

    #[test]
    #[cfg(unix)]
    fn clear_cancels_an_inflight_inline_shell_without_a_later_overwrite() {
        // Review fix: `/clear` used to leave the job slot occupied, so the
        // worker's asynchronous Cancelled report finalized at the stranded
        // placeholder index after the wipe — overwriting whatever message
        // had taken its place.
        let mut repl = Repl::new().expect("test repl");
        start_inline_shell(&mut repl, "sleep 30", 30);
        let job = repl.state.shell_job.as_ref().expect("job in flight");
        let pgid = job.pgid.expect("unix spawn has a pgid");
        let placeholder_idx = job.placeholder_idx;
        assert!(
            repl.chat.messages[placeholder_idx]
                .content
                .contains("running, Esc to cancel"),
            "placeholder expected before the clear"
        );

        // `/clear`: the placeholder is the only message, so handle_clear
        // takes the direct (no-dialog) branch.
        super::handle_clear(&mut repl).expect("clear");
        // The job slot is freed synchronously — the main-loop drain can
        // never finalize into the wiped list — and the group is dead.
        assert!(repl.state.shell_job.is_none(), "job slot freed by /clear");
        assert!(
            wait_pid_gone(pgid, Duration::from_secs(2)),
            "/clear must kill the job's process group"
        );
        // The chat was wiped down to the system notice.
        assert_eq!(repl.chat.len(), 1);

        // The dangerous window: new traffic grows the list past the stale
        // placeholder index while the (dropped) worker report would have
        // arrived. Repeated drains must be no-ops and touch nothing.
        repl.chat
            .add_message(crate::widgets::ChatRole::User, "unrelated user message".into());
        std::thread::sleep(Duration::from_millis(60));
        for _ in 0..10 {
            poll_inline_shell_jobs(&mut repl);
        }
        assert_eq!(repl.chat.len(), 2, "no message appended by the drain");
        assert_eq!(
            repl.chat.messages[1].content, "unrelated user message",
            "the stale placeholder finalization must not overwrite new traffic"
        );
    }

    #[test]
    fn inline_shell_second_job_rejected_while_first_in_flight() {
        let mut repl = Repl::new().expect("test repl");
        start_inline_shell(&mut repl, "sleep 5", 10);
        assert!(repl.state.shell_job.is_some());
        let count = repl.chat.message_count();
        start_inline_shell(&mut repl, "echo second", 10);
        assert!(
            repl.chat.message_count() > count,
            "rejection message posted"
        );
        // Cleanup so no child outlives the test.
        cancel_inline_shell(&mut repl);
        assert!(wait_for_job(&mut repl, Duration::from_secs(2)));
    }

    #[test]
    #[serial_test::serial]
    fn inline_shell_timeout_env_invalid_value_falls_back_to_default() {
        // SAFETY: test-only env mutation, serialized via serial_test.
        unsafe { std::env::set_var("SHANNON_INLINE_SHELL_TIMEOUT", "not-a-number") };
        assert_eq!(
            inline_shell_timeout_from_env(),
            INLINE_SHELL_DEFAULT_TIMEOUT_SECS
        );
        unsafe { std::env::set_var("SHANNON_INLINE_SHELL_TIMEOUT", "0") };
        assert_eq!(
            inline_shell_timeout_from_env(),
            INLINE_SHELL_DEFAULT_TIMEOUT_SECS
        );
        unsafe { std::env::set_var("SHANNON_INLINE_SHELL_TIMEOUT", " 7 ") };
        assert_eq!(inline_shell_timeout_from_env(), 7);
        unsafe { std::env::remove_var("SHANNON_INLINE_SHELL_TIMEOUT") };
        assert_eq!(
            inline_shell_timeout_from_env(),
            INLINE_SHELL_DEFAULT_TIMEOUT_SECS
        );
    }

    #[test]
    fn inline_shell_message_layout_matches_legacy_synchronous_path() {
        assert_eq!(
            format_inline_shell_message("ls", "out\n", "", None),
            "$ ls\nout\n"
        );
        assert_eq!(
            format_inline_shell_message("ls", "out", "err", Some(2)),
            "$ ls\nout\n[stderr]\nerr"
        );
        assert_eq!(
            format_inline_shell_message("true", "", "", Some(0)),
            "$ true\n(exit 0)"
        );
    }
}
