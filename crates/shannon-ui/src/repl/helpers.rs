//! REPL UI helper methods: approval mode, editor, focus, search, pager, notifications, reload.

use shannon_types::recover_lock;

impl super::Repl {
    /// Cycle the approval mode and sync UI state.
    ///
    /// Advances through: Suggest -> AutoEdit -> Plan -> FullAuto -> Readonly -> Suggest.
    /// BypassPermissions requires a confirmation dialog before applying.
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

    /// Open the current input in an external editor ($EDITOR / $VISUAL).
    ///
    /// Writes the current prompt text to a temp file, spawns the editor,
    /// waits for it to exit, then reads the file back and updates the prompt.
    pub fn open_external_editor(&mut self) {
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".to_string());

        let tmp_dir = std::env::temp_dir();
        let tmp_path = tmp_dir.join("shannon-input.md");

        // Build file content: prepend last assistant response as comments
        let current_input = self.prompt.input().to_string();
        let file_content = if current_input.is_empty() {
            // Inject last assistant message as context when opening blank editor
            let mut buf = String::new();
            if let Some(last) = self.chat.last_assistant_message() {
                buf.push_str("# AI's last response (for context, edit below):\n");
                for line in last.content.lines() {
                    buf.push_str("# ");
                    buf.push_str(line);
                    buf.push('\n');
                }
                buf.push('\n');
            }
            buf
        } else {
            current_input
        };
        if let Err(e) = std::fs::write(&tmp_path, &file_content) {
            self.state.toast = Some((format!("  Failed to write temp file: {e}  "), std::time::Instant::now()));
            return;
        }

        // Suspend raw mode, spawn editor, wait
        let _ = crossterm::terminal::disable_raw_mode();
        let result = std::process::Command::new(&editor)
            .arg(&tmp_path)
            .status();
        let _ = crossterm::terminal::enable_raw_mode();

        match result {
            Ok(status) if status.success() => {
                if let Ok(new_text) = std::fs::read_to_string(&tmp_path) {
                    let trimmed = new_text.trim_end().to_string();
                    self.prompt.set_input(trimmed);
                    self.state.toast = Some(("  Editor saved  ".to_string(), std::time::Instant::now()));
                }
            }
            Ok(_) => {
                self.state.toast = Some(("  Editor exited with error  ".to_string(), std::time::Instant::now()));
            }
            Err(e) => {
                self.state.toast = Some((format!("  Failed to launch editor: {e}  "), std::time::Instant::now()));
            }
        }

        let _ = std::fs::remove_file(&tmp_path);
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

    /// Navigate to the next search match.
    pub fn chat_search_next(&mut self) {
        if self.state.chat_search_total_matches > 0 {
            self.state.chat_search_match_index =
                (self.state.chat_search_match_index + 1) % self.state.chat_search_total_matches;
        }
    }

    /// Navigate to the previous search match.
    pub fn chat_search_prev(&mut self) {
        if self.state.chat_search_total_matches > 0 {
            self.state.chat_search_match_index = if self.state.chat_search_match_index == 0 {
                self.state.chat_search_total_matches - 1
            } else {
                self.state.chat_search_match_index - 1
            };
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

    /// Scroll the pager by `delta` lines (negative = up, positive = down).
    pub fn pager_scroll(&mut self, delta: isize) {
        let total = self.chat.message_count();
        if let Some(area_height) = self.terminal_height() {
            let max_scroll = total.saturating_sub(area_height);
            let new = self.state.pager_scroll as isize + delta;
            self.state.pager_scroll = new.clamp(0, max_scroll as isize) as usize;
        }
    }

    /// Scroll pager to top.
    pub fn pager_scroll_top(&mut self) {
        self.state.pager_scroll = 0;
    }

    /// Scroll pager to bottom.
    pub fn pager_scroll_bottom(&mut self) {
        let total = self.chat.message_count();
        if let Some(area_height) = self.terminal_height() {
            self.state.pager_scroll = total.saturating_sub(area_height);
        }
    }

    /// Get the terminal height (content area, excluding borders).
    pub(crate) fn terminal_height(&self) -> Option<usize> {
        // Approximate: use 80% of terminal height for content
        crossterm::terminal::size().ok().map(|(_, h)| (h as usize).saturating_sub(6))
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
}
