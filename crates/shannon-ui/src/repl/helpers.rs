//! REPL UI helper methods: approval mode, editor, focus, search, pager, notifications, reload.

use shannon_types::recover_lock;

impl super::Repl {
    /// Cycle the approval mode and sync UI state.
    ///
    /// Cycles through 4 core modes: ASK → EDIT → PLAN → AUTO → ASK.
    /// Other modes (FULL, etc.) are set explicitly via /mode.
    pub fn cycle_approval_mode(&mut self) {
        if let Some(ref query_engine) = self.query_engine {
            let current = {
                let perms = recover_lock(query_engine.permissions().read());
                perms.approval_mode()
            };

            let next = current.cycle_next();

            if next == shannon_core::permissions::ApprovalMode::BypassPermissions {
                self.show_confirm_dialog(
                    "Bypass Permissions",
                    "This will skip ALL permission checks. Only use in trusted environments.\n\nAre you sure?",
                    "set_bypass_mode",
                );
            } else {
                let mut perms = recover_lock(query_engine.permissions().write());
                perms.set_approval_mode(next);
                let label = next.short_label().to_string();
                drop(perms);
                self.state.status = format!("Mode: {label}");
                self.state.toast = Some((format!("  Mode: {label}  "), std::time::Instant::now()));
                self.state.approval_mode_label = label;
            }
        }
    }

    /// Sync the approval mode label from the PermissionManager to UI state.
    pub(crate) fn sync_approval_mode_label(&mut self) {
        if let Some(ref query_engine) = self.query_engine {
            let label = {
                let perms = recover_lock(query_engine.permissions().read());
                perms.approval_mode().short_label().to_string()
            };
            self.state.approval_mode_label = label;
        }
    }

    /// Toggle focus mode (hide/show header and statusbar).
    pub fn toggle_focus_mode(&mut self) {
        self.state.focus_mode = !self.state.focus_mode;
        if self.state.focus_mode {
            // Entering focus mode disables fullscreen (focus is a subset)
            self.state.fullscreen_mode = false;
        }
        let label = if self.state.focus_mode { "Focus ON" } else { "Focus OFF" };
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
        let label = if self.state.fullscreen_mode { "Fullscreen ON (F11)" } else { "Fullscreen OFF" };
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
        self.state.pending_notifications.retain(|(_, t)| t.elapsed().as_secs() < 30);
        self.state.pending_notifications.push((msg, std::time::Instant::now()));
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
        if total == 0 { return; }
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
                    format!("[Settings changed: {} — reload with /config or restart to apply]", changed.join(", ")),
                );
            }
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

        let Some(ref cmd) = self.state.statusline_command else { return };

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
            "approval_mode": self.state.approval_mode_label,
        });

        let result = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .env("SHANNON_STATUSLINE", "1")
            .spawn()
            .ok()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    let _ = stdin.write_all(json_payload.to_string().as_bytes());
                }
                child.wait().ok().filter(|s| s.success())?;
                child.stdout.and_then(|mut out| {
                    let mut buf = String::new();
                    std::io::Read::read_to_string(&mut out, &mut buf).ok()?;
                    Some(buf.trim().to_string())
                })
            });

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
