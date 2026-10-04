//! REPL UI helper methods: approval mode, editor, focus, search, pager, notifications, reload.

use shannon_types::recover_lock;

impl super::Repl {
    /// Cycle the approval mode and sync UI state.
    ///
    /// Cycles the three autonomy-ladder stops: ASK → EDIT → FULL → ASK.
    /// Plan is a workflow tier, not a ladder stop (design §5.5): while a plan
    /// is active, the FIRST Shift+Tab exits plan mode and restores the
    /// snapshotted ladder mode; expert modes (RO/CI/BYPASS) reset to ASK.
    pub fn cycle_approval_mode(&mut self) {
        use shannon_engine::permissions::ApprovalMode;

        if self.state.plan.active {
            self.exit_plan_restore_mode("Shift+Tab");
            return;
        }

        let current = if let Some(ref query_engine) = self.query_engine {
            let perms = recover_lock(query_engine.permissions().read());
            perms.approval_mode()
        } else {
            self.state.approval_mode
        };

        let next = current.cycle_next();

        if next == ApprovalMode::BypassPermissions {
            self.show_confirm_dialog(
                "Bypass Permissions",
                "This will skip ALL permission checks. Only use in trusted environments.\n\nAre you sure?",
                "set_bypass_mode",
            );
        } else {
            if let Some(ref query_engine) = self.query_engine {
                let mut perms = recover_lock(query_engine.permissions().write());
                perms.set_approval_mode(next);
                drop(perms);
            }
            self.state.approval_mode = next;
            let label = next.short_label();
            self.state.status = format!("Mode: {label}");
            self.state.toast = Some((format!("  Mode: {label}  "), std::time::Instant::now()));
        }
    }

    /// Design §5.5 / P0-2: leave plan mode, restore the snapshotted ladder
    /// mode, and lift the plan write gate. `via` names the trigger for the
    /// toast (`/plan off`, `Shift+Tab`, `rejected`).
    pub fn exit_plan_restore_mode(&mut self, via: &str) {
        let restored = if let Some(ref query_engine) = self.query_engine {
            let session_id = query_engine.session_id();
            let restored = {
                let mut perms = recover_lock(query_engine.permissions().write());
                perms.exit_plan_mode(session_id)
            };
            if let Ok(mut flag) = self.plan_mode_flag.write() {
                *flag = false;
            }
            restored
        } else {
            shannon_engine::permissions::ApprovalMode::Ask
        };
        self.state.plan = super::PlanState::default();
        self.state.approval_mode = restored;
        let label = restored.short_label();
        self.state.status = format!("Mode: {label}");
        self.state.toast = Some((
            format!("  Exited plan mode ({via}) — restored {label}  "),
            std::time::Instant::now(),
        ));
    }

    /// Sync the approval mode label from the PermissionManager to UI state.
    pub(crate) fn sync_approval_mode_label(&mut self) {
        if let Some(ref query_engine) = self.query_engine {
            let mode = {
                let perms = recover_lock(query_engine.permissions().read());
                perms.approval_mode()
            };
            self.state.approval_mode = mode;
        }
    }

    /// Toggle focus mode (hide/show header and statusbar).
    pub fn toggle_focus_mode(&mut self) {
        self.state.focus_mode = !self.state.focus_mode;
        if self.state.focus_mode {
            // Entering focus mode disables fullscreen (focus is a subset)
            self.state.fullscreen_mode = false;
        }
        let label = if self.state.focus_mode {
            "Focus ON"
        } else {
            "Focus OFF"
        };
        self.state.toast = Some((format!("  {label}  "), std::time::Instant::now()));
    }

    /// Cycle view mode (Default ↔ Verbose). Bound to Ctrl+O.
    pub fn cycle_view_mode(&mut self) {
        self.state.view_mode = self.state.view_mode.cycle();
        let verbose = self.state.view_mode == super::state::ViewMode::Verbose;
        self.chat.collapsed_tools = !verbose;
        let label = self.state.view_mode.label();
        self.state.toast = Some((format!("  View: {label}  "), std::time::Instant::now()));
    }

    /// Toggle fullscreen mode (hide ALL chrome, chat fills terminal).
    /// Bound to F11.
    pub fn toggle_fullscreen_mode(&mut self) {
        self.state.fullscreen_mode = !self.state.fullscreen_mode;
        if self.state.fullscreen_mode {
            // Fullscreen implies focus mode too
            self.state.focus_mode = true;
        }
        let label = if self.state.fullscreen_mode {
            "Fullscreen ON (F11)"
        } else {
            "Fullscreen OFF"
        };
        self.state.toast = Some((format!("  {label}  "), std::time::Instant::now()));
    }

    /// Toggle chat search mode (highlight matches in chat).
    pub fn toggle_chat_search(&mut self) {
        if self.state.chat_search_active {
            // Deactivate search
            self.state.chat_search_active = false;
            self.state.chat_search_query.clear();
            self.state.chat_search_match_index = 0;
            self.state.chat_search_total_matches = 0;
        } else {
            // Activate search
            self.state.chat_search_active = true;
            self.state.chat_search_query.clear();
            self.state.chat_search_match_index = 0;
            self.state.chat_search_total_matches = 0;
        }
    }

    /// Update chat search results based on current query.
    pub fn update_chat_search(&mut self) {
        if !self.state.chat_search_active || self.state.chat_search_query.is_empty() {
            self.state.chat_search_total_matches = 0;
            self.state.chat_search_match_index = 0;
            return;
        }
        let matches = self.chat.find_search_matches(&self.state.chat_search_query);
        self.state.chat_search_total_matches = matches.len();
        if self.state.chat_search_match_index >= matches.len() {
            self.state.chat_search_match_index = 0;
        }
    }

    /// Navigate to the next search match and scroll to it.
    pub fn chat_search_next(&mut self) {
        if self.state.chat_search_total_matches > 0 {
            self.state.chat_search_match_index =
                (self.state.chat_search_match_index + 1) % self.state.chat_search_total_matches;
            self.scroll_to_search_match();
        }
    }

    /// Navigate to the previous search match and scroll to it.
    pub fn chat_search_prev(&mut self) {
        if self.state.chat_search_total_matches > 0 {
            self.state.chat_search_match_index = if self.state.chat_search_match_index == 0 {
                self.state.chat_search_total_matches - 1
            } else {
                self.state.chat_search_match_index - 1
            };
            self.scroll_to_search_match();
        }
    }

    /// Scroll the chat to the message containing the current search match.
    fn scroll_to_search_match(&mut self) {
        let matches = self.chat.find_search_matches(&self.state.chat_search_query);
        if let Some(&(msg_idx, _, _)) = matches.get(self.state.chat_search_match_index) {
            self.chat.scroll_offset = msg_idx;
            self.state.auto_follow = false;
        }
    }

    /// Push a notification into the pending queue (shown in status bar).
    /// Old notifications (>30s) are pruned automatically.
    pub fn notify(&mut self, message: impl Into<String>) {
        let msg = message.into();
        self.state
            .pending_notifications
            .retain(|(_, t)| t.elapsed().as_secs() < 30);
        self.state
            .pending_notifications
            .push((msg, std::time::Instant::now()));
    }

    /// Check if this is a first run (no config files) and activate onboarding.
    pub fn check_first_run(&mut self) {
        let local = std::path::Path::new(".shannon.toml").exists();
        let home = std::path::Path::new(&format!(
            "{}/.shannon/config.toml",
            std::env::var("HOME").unwrap_or_default()
        ))
        .exists();
        if !local && !home {
            self.state.onboarding_active = true;
        }
    }

    /// Toggle the transcript pager on/off.
    pub fn toggle_pager(&mut self) {
        self.state.pager_active = !self.state.pager_active;
        self.state.pager_scroll = 0;
    }

    /// Scroll the pager by `delta` messages (negative = up, positive = down).
    pub fn pager_scroll(&mut self, delta: isize) {
        let total = self.chat.message_count();
        if total == 0 {
            return;
        }
        let max_scroll = total.saturating_sub(1);
        let new = self.state.pager_scroll as isize + delta;
        self.state.pager_scroll = new.clamp(0, max_scroll as isize) as usize;
    }

    /// Scroll pager to top.
    pub fn pager_scroll_top(&mut self) {
        self.state.pager_scroll = 0;
    }

    /// Scroll pager to bottom.
    pub fn pager_scroll_bottom(&mut self) {
        let total = self.chat.message_count();
        self.state.pager_scroll = total.saturating_sub(1);
    }

    /// Check if project instruction files have changed and hot-reload them.
    ///
    /// Returns true if instructions were reloaded, false if unchanged.
    pub fn check_reload_instructions(&mut self) -> bool {
        let changed_info = match self.instruction_watcher.as_mut() {
            Some(w) => w.check_and_reload(),
            None => return false,
        };

        match changed_info {
            Some((files, new_content)) => {
                if let Some(ref mut engine) = self.query_engine {
                    // Reset system prompt to base + reloaded instructions
                    // The engine's append_system_prompt adds cumulatively, so we
                    // need to be smarter: just log the change and append a note.
                    tracing::info!("Hot-reloaded project instructions: {:?}", files);
                    if !new_content.is_empty() {
                        let reload_msg = format!(
                            "\n\n[SYSTEM: Project instructions were hot-reloaded from: {}]",
                            files.join(", ")
                        );
                        engine.append_system_prompt(&reload_msg);
                    }
                }
                true
            }
            None => false,
        }
    }

    /// Check if custom command files have changed and hot-reload them.
    pub fn check_reload_commands(&mut self) {
        if let Some(ref mut watcher) = self.command_watcher {
            let count = watcher.check_and_reload(&self.command_registry);
            if count > 0 {
                self.chat.add_message(
                    crate::widgets::ChatRole::System,
                    format!("[Custom commands hot-reloaded: {count} command(s)]"),
                );
            }
        }
    }

    /// Check if settings files have changed and notify the user.
    pub fn check_reload_settings(&mut self) {
        if let Some(ref watcher) = self.settings_watcher {
            if let Some(changed) = watcher.check_and_reload() {
                self.chat.add_message(
                    crate::widgets::ChatRole::System,
                    format!(
                        "[Settings changed: {} — reload with /config or restart to apply]",
                        changed.join(", ")
                    ),
                );
            }
        }
    }

    /// Check if source files have changed since last check.
    /// Returns changed file paths for display or diagnostic triggering.
    pub fn check_source_changes(&mut self) -> Vec<String> {
        if let Some(ref watcher) = self.source_watcher {
            watcher.check_changes()
        } else {
            Vec::new()
        }
    }

    /// Refresh the git branch name from the working directory.
    /// Throttled to once every 10 seconds to avoid excessive subprocess calls.
    pub fn refresh_git_branch(&mut self) {
        // Throttle: no need to check more than once every 10s
        static LAST_CHECK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last = LAST_CHECK.load(std::sync::atomic::Ordering::Relaxed);
        if now.saturating_sub(last) < 10 {
            return;
        }
        LAST_CHECK.store(now, std::sync::atomic::Ordering::Relaxed);

        let branch = std::process::Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&self.state.working_directory)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        self.state.git_branch = branch;
    }

    /// Refresh the custom statusline by running the configured command.
    /// Called periodically from the main event loop (every ~5s).
    pub fn refresh_statusline(&mut self) {
        // Always refresh git branch (lightweight, throttled implicitly)
        self.refresh_git_branch();

        let Some(ref cmd) = self.state.statusline_command else {
            return;
        };

        // Throttle to once every 5 seconds
        if let Some(t) = self.state.statusline_last_update {
            if t.elapsed().as_secs() < 5 {
                return;
            }
        }

        // Build JSON payload with current session state
        let json_payload = serde_json::json!({
            "model": self.state.model,
            "status": self.state.status,
            "tokens_used": self.state.tokens_used,
            "input_tokens": self.state.input_tokens,
            "output_tokens": self.state.output_tokens,
            "cost_usd": self.state.total_cost_usd,
            "turn_count": self.state.turn_count,
            "streaming_active": self.state.streaming_active,
            "approval_mode": self.state.approval_mode.to_string(),
        });

        // P0-2: bounded, concurrent-read execution — see
        // `run_statusline_command`. On timeout or failure the result is
        // `None` and the previous cached statusline is kept, exactly like
        // the old failure path.
        let result =
            run_statusline_command(cmd, &json_payload.to_string(), STATUSLINE_DEFAULT_TIMEOUT);

        if let Some(output) = result {
            self.state.cached_statusline = Some(output);
        }
        self.state.statusline_last_update = Some(std::time::Instant::now());
    }

    /// Save UI state to ~/.shannon/ui_state.json for session restore.
    pub(crate) fn save_ui_state(&mut self) {
        let state = super::state::PersistedUiState {
            collapsed_tools: self.state.view_mode == super::state::ViewMode::Default,
            view_mode: self.state.view_mode.label().to_string(),
            theme_name: self.state.theme.name.clone(),
            scroll_offset: self.chat.scroll_offset,
            focus_mode: self.state.focus_mode,
            fullscreen_mode: self.state.fullscreen_mode,
        };

        let path = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".shannon")
            .join("ui_state.json");

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(&state) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    tracing::debug!("Failed to save UI state: {e}");
                }
            }
            Err(e) => tracing::debug!("Failed to serialize UI state: {e}"),
        }
    }

    /// Load UI state from ~/.shannon/ui_state.json and apply to current session.
    pub(crate) fn load_ui_state(&mut self) {
        let path = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".shannon")
            .join("ui_state.json");

        let data = match std::fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => return,
        };

        let state: super::state::PersistedUiState = match serde_json::from_str(&data) {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!("Failed to parse UI state: {e}");
                return;
            }
        };

        self.state.view_mode = match state.view_mode.as_str() {
            "Verbose" => super::state::ViewMode::Verbose,
            _ => super::state::ViewMode::Default,
        };
        self.state.focus_mode = state.focus_mode;
        self.state.fullscreen_mode = state.fullscreen_mode;

        // Restore theme if name matches a known theme
        if !state.theme_name.is_empty() && state.theme_name != self.state.theme.name {
            if let Some(theme) = crate::theme::Theme::named(&state.theme_name) {
                self.state.theme = theme;
                self.renderer.set_theme(&self.state.theme);
            }
        }

        self.state.persisted_ui_state = Some(state);
    }
}

// ── Custom statusline execution (P0-2) ──────────────────────────────────
//
// The old runner wrote the payload to stdin and then `wait()`ed BEFORE
// reading stdout: any script emitting more than the ~64KiB pipe buffer
// deadlocked both sides, a hung script hung the UI thread forever, and
// stdin was never closed (dropped only at scope end), so scripts reading
// stdin to EOF hung too. `run_statusline_command` fixes all three with
// `wait_with_output` semantics plus a hard wall-clock budget.

/// Hard wall-clock budget for one statusline run. The tick loop calls this
/// synchronously (throttled to ~5s), so the budget bounds the worst-case UI
/// stall. Parameterized on `run_statusline_command` so tests can inject a
/// shorter deadline.
const STATUSLINE_DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Cap captured statusline stdout at 32KiB: a statusline renders one line,
/// so larger output is truncated. The pipe is still drained to EOF (a
/// mid-run EPIPE would kill an otherwise successful script) — only the
/// first `STATUSLINE_MAX_CAPTURE_BYTES` are retained.
const STATUSLINE_MAX_CAPTURE_BYTES: usize = 32 * 1024;

/// Run the custom statusline command under a hard wall-clock budget.
///
/// stdin is written fully and CLOSED before waiting, and stdout is drained
/// to EOF on a helper thread concurrently with the wait (`wait_with_output`
/// semantics), so a chatty script can always exit. On timeout the child is
/// killed and `None` is returned; the caller keeps the previous cached
/// statusline. The `SHANNON_STATUSLINE=1` env marker and the exit-success
/// filter preserve the original contract.
fn run_statusline_command(
    cmd: &str,
    payload: &str,
    timeout: std::time::Duration,
) -> Option<String> {
    // Windows fallback (PR #166): resolve the platform shell instead of a
    // hardcoded `sh`, which does not exist on stock Windows.
    let (program, args) = shannon_types::shell::local_shell(cmd);
    let mut child = std::process::Command::new(program)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .env("SHANNON_STATUSLINE", "1")
        .spawn()
        .ok()?;

    // Concurrent stdout drain — the fix for the >pipe-buffer deadlock: the
    // child can write until it exits while we wait. `None` means the pipe
    // was unexpectedly missing (legacy behavior: overall failure).
    let stdout_pipe = child.stdout.take();
    let (captured_tx, captured_rx) = std::sync::mpsc::channel::<Option<String>>();
    std::thread::spawn(move || {
        let _ = captured_tx.send(read_capped_to_string(
            stdout_pipe,
            STATUSLINE_MAX_CAPTURE_BYTES,
        ));
    });

    // wait_with_output semantics: write stdin fully and close it (drop at
    // block end) BEFORE waiting, so scripts reading stdin to EOF terminate.
    // The payload is far below the pipe buffer, so the write cannot block.
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(payload.as_bytes());
    }

    let deadline = std::time::Instant::now() + timeout;
    let outcome: Option<(std::process::ExitStatus, Option<String>)> = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Bound the reader join by the remaining budget: a
                // grandchild holding the pipe open must not extend it.
                let remaining = deadline
                    .saturating_duration_since(std::time::Instant::now())
                    .max(std::time::Duration::from_millis(1));
                break match captured_rx.recv_timeout(remaining) {
                    Ok(captured) => Some((status, captured)),
                    Err(_) => None, // reader overran the budget
                };
            }
            Ok(None) if std::time::Instant::now() >= deadline => {
                // Budget expired: kill the child, reap it, report failure.
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };

    let (status, captured) = outcome?;
    if !status.success() {
        return None; // same success filter as the legacy runner
    }
    captured.map(|s| s.trim().to_string())
}

/// Read `reader` to EOF, retaining only the first `cap` bytes — output
/// beyond the cap is drained and discarded (truncation, not failure). The
/// full drain matters: closing early would hand a still-writing script an
/// EPIPE and turn a success into a failure. `None` reader (pipe missing)
/// yields `None`, matching the legacy failure shape.
fn read_capped_to_string<R: std::io::Read>(mut reader: Option<R>, cap: usize) -> Option<String> {
    let reader = reader.as_mut()?;
    let mut kept: Vec<u8> = Vec::with_capacity(cap.min(64 * 1024));
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break, // EOF — pipe fully drained
            Ok(n) => {
                if kept.len() < cap {
                    let take = (cap - kept.len()).min(n);
                    kept.extend_from_slice(&chunk[..take]);
                }
            }
            Err(_) => break,
        }
    }
    Some(String::from_utf8_lossy(&kept).into_owned())
}

// ── Statusline (P0-2) tests — real `/bin/sh`, per repo precedent ────────
#[cfg(test)]
mod statusline_tests {
    use super::{STATUSLINE_MAX_CAPTURE_BYTES, run_statusline_command};
    use std::time::{Duration, Instant};

    #[test]
    fn statusline_normal_output_passes_through_trimmed() {
        let out = run_statusline_command("echo ' main * 12k '", "{}", Duration::from_secs(5));
        assert_eq!(out.as_deref(), Some("main * 12k"));
    }

    #[test]
    fn statusline_large_output_returns_truncated_well_under_timeout() {
        // ~200KiB — three times the capture cap and well beyond the ~64KiB
        // pipe buffer that used to deadlock the old wait-then-read runner.
        let began = Instant::now();
        let out = run_statusline_command(
            "head -c 200000 /dev/zero | tr '\\0' 'x'",
            "{}",
            Duration::from_secs(5),
        );
        let elapsed = began.elapsed();
        let out = out.expect("large-output script must still succeed");
        assert_eq!(out.len(), STATUSLINE_MAX_CAPTURE_BYTES, "capped at 32KiB");
        assert!(
            out.chars().all(|c| c == 'x'),
            "truncated tail kept, not garbage"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "returned well under the injected timeout: {elapsed:?}"
        );
    }

    #[test]
    fn statusline_hung_script_times_out_and_returns_none() {
        let began = Instant::now();
        let out = run_statusline_command("sleep 30", "{}", Duration::from_millis(500));
        let elapsed = began.elapsed();
        assert!(out.is_none(), "hung script must return None");
        assert!(elapsed >= Duration::from_millis(500), "budget respected");
        assert!(
            elapsed < Duration::from_secs(3),
            "returned near the budget, not at script end: {elapsed:?}"
        );
    }

    #[test]
    fn statusline_failing_script_returns_none() {
        // The legacy success filter: non-zero exit → None, cache untouched.
        let out = run_statusline_command("echo partial; exit 3", "{}", Duration::from_secs(5));
        assert!(out.is_none());
    }

    #[test]
    fn statusline_closes_stdin_and_sets_shannon_statusline_env() {
        // `cat` terminates only because stdin is closed after the payload is
        // written (wait_with_output semantics); the script must also see the
        // SHANNON_STATUSLINE=1 marker from the original contract.
        let out = run_statusline_command(
            "payload=$(cat); printf '%s' \"$SHANNON_STATUSLINE:$payload\"",
            "{\"k\":1}",
            Duration::from_secs(5),
        );
        assert_eq!(out.as_deref(), Some("1:{\"k\":1}"));
    }
}
