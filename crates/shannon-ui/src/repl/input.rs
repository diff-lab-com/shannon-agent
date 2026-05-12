//! REPL keyboard input handling

use crate::{
    widgets::ChatRole,
    Result,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use crate::vim::{Direction, VimAction};
use rust_i18n::t;

use super::Repl;

/// Open an external editor ($VISUAL or $EDITOR, fallback to vi) with a temp file.
/// Returns the edited content on success.
fn open_external_editor(content: &str) -> std::result::Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let dir = std::env::temp_dir();
    let path = dir.join("shannon-input.md");

    // RAII guard ensures temp file cleanup on any exit path
    struct TempGuard(std::path::PathBuf);
    impl Drop for TempGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _guard = TempGuard(path.clone());

    std::fs::write(&path, content)?;
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    // Split editor into command + args (handles "code --wait", "vim -u NONE", etc.)
    let mut parts = editor.split_whitespace();
    let cmd = parts.next().unwrap_or("vi");
    let args: Vec<&str> = parts.collect();
    let status = std::process::Command::new(cmd)
        .args(&args)
        .arg(&path)
        .status()?;
    if status.success() {
        Ok(std::fs::read_to_string(&path)?)
    } else {
        Err("Editor exited with error".into())
    }
}

/// Handle mouse events — scroll wheel to navigate chat history.
pub fn handle_mouse(repl: &mut Repl, mouse: MouseEvent) {
    let step = (repl.chat.chat_viewport_height() as usize / 4).max(3);
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            repl.chat.scroll_up_by(step);
            if repl.state.auto_follow {
                repl.state.messages_at_scroll_pause = repl.chat.message_count();
            }
            repl.state.auto_follow = false;
        }
        MouseEventKind::ScrollDown => {
            repl.chat.scroll_down_by(step);
            if repl.chat.is_at_bottom() {
                repl.state.auto_follow = true;
            }
        }
        _ => {}
    }
}

/// Handle keyboard input — dispatches to the appropriate sub-handler.
pub fn handle_input(repl: &mut Repl, key: KeyEvent, terminal: Option<&mut super::query::Term>) -> Result<()> {
    // Dismiss onboarding overlay on user interaction
    if repl.state.onboarding_active {
        match key.code {
            KeyCode::Enter | KeyCode::Esc => {
                repl.state.onboarding_active = false;
                return Ok(());
            }
            KeyCode::Char(_) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                repl.state.onboarding_active = false;
                // Fall through to let the character be typed into the prompt
            }
            _ => return Ok(()),
        }
    }

    // If permission dialog is active, handle dialog-specific keys
    if repl.state.permission_dialog.is_some() {
        return handle_permission_dialog_input(repl, key);
    }

    // If a confirm/alert dialog is active, handle dialog keys
    if repl.state.active_dialog.is_some() {
        return handle_active_dialog_input(repl, key);
    }

    // If an input dialog is active, handle text input
    if repl.state.input_dialog.is_some() {
        return handle_input_dialog_input(repl, key);
    }

    // If fuzzy picker is active, handle picker input
    if repl.state.fuzzy_picker.is_some() {
        return handle_fuzzy_picker_input(repl, key);
    }

    // If file selector is active, handle file selector input
    if repl.state.file_selector.is_some() {
        return handle_file_selector_input(repl, key);
    }

    // If model picker is active, handle model picker input
    if repl.state.model_picker.is_some() {
        return handle_model_picker_input(repl, key);
    }

    // If theme picker is active, handle theme picker input
    if repl.state.theme_picker.is_some() {
        return handle_theme_picker_input(repl, key);
    }

    // If multi-select is active, handle multi-select input
    if repl.state.multi_select.is_some() {
        return handle_multi_select_input(repl, key);
    }

    // If tool approval overlay is active, handle approval keys
    if repl.state.tool_approval.is_active() {
        return handle_tool_approval_input(repl, key);
    }

    // If command palette overlay is active, handle palette keys
    if repl.state.command_palette.is_some() {
        return handle_command_palette_input(repl, key);
    }

    // If plan overlay is active and not yet approved, handle scroll
    if repl.state.plan.active && !repl.state.plan.approved {
        return handle_plan_input(repl, key);
    }

    // If diff viewer overlay is active, handle diff viewer keys
    if repl.state.diff_viewer.is_some() {
        return handle_diff_viewer_input(repl, key);
    }

    // If key hints overlay is active, dismiss on any key
    if repl.state.show_key_hints {
        repl.state.show_key_hints = false;
        return Ok(());
    }

    // If transcript pager is active, handle pager keys
    if repl.state.pager_active {
        return handle_pager_input(repl, key);
    }

    // If incremental search (Ctrl+R) is active, handle search keys
    if repl.state.incremental_search_active {
        return handle_incremental_search_input(repl, key);
    }

    // If chat search (activated by / in pager) is active, handle search input
    if repl.state.chat_search_active {
        return handle_chat_search_input(repl, key);
    }

    // In non-Insert vim modes, route all keys through the vim handler
    use crate::vim::VimMode;
    if repl.vim_handler.mode() != VimMode::Insert {
        let action = repl.vim_handler.process_key(key);
        let mode_str = repl.vim_handler.mode().to_string();
        repl.state.vim_mode = mode_str.clone();
        repl.prompt.set_vim_mode(&mode_str);
        handle_vim_action(repl, action);
        return Ok(());
    }

    match key.code {
        // F1: show full keyboard shortcuts overlay
        KeyCode::F(1) => {
            repl.state.show_key_hints = true;
            Ok(())
        }
        // ?: show help when prompt is empty (avoids conflict with typing)
        KeyCode::Char('?') if repl.prompt.input().trim().is_empty() => {
            repl.state.show_key_hints = true;
            Ok(())
        }
        // F8: toggle mouse capture (when off, terminal handles text selection/copy)
        KeyCode::F(8) => {
            repl.state.mouse_capture_enabled = !repl.state.mouse_capture_enabled;
            let label = if repl.state.mouse_capture_enabled {
                "Mouse scroll ON (Shift+drag to copy)"
            } else {
                "Mouse scroll OFF — text selection enabled"
            };
            repl.state.toast = Some((format!("  {label}  "), std::time::Instant::now()));
            Ok(())
        }
        // Ctrl+V: paste image from system clipboard
        KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            super::commands::handle_image_paste_from_input(repl)?;
            Ok(())
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            open_command_palette(repl);
            Ok(())
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Sidebar disabled — no-op
            Ok(())
        }
        KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if repl.state.sidebar_visible {
                repl.state.sidebar_tab = repl.state.sidebar_tab.prev();
            }
            Ok(())
        }
        KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if repl.state.sidebar_visible {
                repl.state.sidebar_tab = repl.state.sidebar_tab.next();
            }
            Ok(())
        }
        KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            repl.running = false;
            Ok(())
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let input = repl.prompt.input().trim().to_string();
            if !input.is_empty() {
                // Stash draft to history so Up-arrow recovers it
                repl.command_history.push(&input);
                repl.prompt.set_input(String::new());
                repl.state.completion_suggestions.clear();
                repl.state.completion_suggestion_index = 0;
            } else {
                repl.running = false;
            }
            Ok(())
        }
        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // External editor: Ctrl+E opens $VISUAL/$EDITOR/vi with current input
            let current = repl.prompt.input().to_string();
            let editor_content = if current.is_empty() {
                // Inject last assistant message as context when opening blank editor
                let mut buf = String::new();
                if let Some(last) = repl.chat.last_assistant_message() {
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
                current
            };
            // Suspend raw mode so the editor can take over the terminal
            let _ = crossterm::terminal::disable_raw_mode();
            match open_external_editor(&editor_content) {
                Ok(edited) => {
                    // Trim trailing newline that editors often append
                    let trimmed = edited.trim_end_matches('\n').to_string();
                    if !trimmed.is_empty() {
                        repl.prompt.set_input(trimmed);
                    }
                }
                Err(e) => {
                    repl.chat.add_message(
                        ChatRole::System,
                        format!("Editor error: {e}"),
                    );
                }
            }
            // Re-enable raw mode for the TUI
            let _ = crossterm::terminal::enable_raw_mode();
            Ok(())
        }
        // Ctrl+R: activate incremental history search
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            repl.state.incremental_search_active = true;
            repl.state.incremental_search_query.clear();
            repl.state.incremental_search_match_index = 0;
            repl.state.incremental_search_match_count = 0;
            repl.state.incremental_search_saved_input = repl.prompt.input().to_string();
            Ok(())
        }
        KeyCode::Enter => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                repl.prompt.insert_newline();
            } else if !repl.state.completion_suggestions.is_empty() {
                accept_completion(repl);
            } else if repl.state.streaming_active {
                let input = repl.prompt.input().trim().to_string();
                if !input.is_empty() {
                    repl.state.queued_message = Some(input);
                    repl.prompt.clear();
                    repl.state.status = "Message queued (will send after current response)".to_string();
                    repl.state.toast = Some(("Queued".to_string(), std::time::Instant::now()));
                }
            } else {
                super::commands::submit_input(repl, terminal)?;
            }
            Ok(())
        }
        KeyCode::Char('\n') => {
            // Some terminals send Shift+Enter as Char('\n') instead of
            // Enter with SHIFT modifier. Insert a newline in that case.
            repl.prompt.insert_newline();
            Ok(())
        }
        // Ctrl+F: toggle fold/collapse of last tool message
        KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            repl.chat.toggle_last_tool_fold();
            Ok(())
        }
        // Ctrl+O: cycle view mode (Default ↔ Verbose)
        KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            repl.cycle_view_mode();
            Ok(())
        }
        // Alt+F: toggle all tool messages collapsed/expanded
        KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::ALT) => {
            repl.chat.collapsed_tools = !repl.chat.collapsed_tools;
            Ok(())
        }
        // Ctrl+G: toggle transcript pager
        KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            repl.toggle_pager();
            Ok(())
        }
        // PageUp: scroll chat up by viewport height
        KeyCode::PageUp => {
            let page = repl.chat.chat_viewport_height() as usize;
            repl.chat.scroll_up_by(page.saturating_sub(2).max(1));
            if repl.state.auto_follow {
                repl.state.messages_at_scroll_pause = repl.chat.message_count();
            }
            repl.state.auto_follow = false;
            Ok(())
        }
        // PageDown: scroll chat down by viewport height
        KeyCode::PageDown => {
            let page = repl.chat.chat_viewport_height() as usize;
            repl.chat.scroll_down_by(page.saturating_sub(2).max(1));
            if repl.chat.is_at_bottom() {
                repl.state.auto_follow = true;
            }
            Ok(())
        }
        // Home: jump to top of chat
        KeyCode::Home => {
            repl.chat.scroll_to_top();
            if repl.state.auto_follow {
                repl.state.messages_at_scroll_pause = repl.chat.message_count();
            }
            repl.state.auto_follow = false;
            Ok(())
        }
        // End: jump to bottom (latest message)
        KeyCode::End => {
            repl.chat.scroll_to_latest();
            repl.state.auto_follow = true;
            Ok(())
        }
        // Ctrl+T: toggle transcript pager (alternative keybinding)
        KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            repl.toggle_pager();
            Ok(())
        }
        // Ctrl+W: close current session tab
        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if repl.state.session_tab.visible {
                repl.state.session_tab.close_session();
            }
            Ok(())
        }
        // Alt+Left: previous session tab
        KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => {
            if repl.state.session_tab.visible {
                repl.state.session_tab.prev_session();
            }
            Ok(())
        }
        // Alt+Right: next session tab
        KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => {
            if repl.state.session_tab.visible {
                repl.state.session_tab.next_session();
            }
            Ok(())
        }
        KeyCode::Char('@') => {
            // Open fuzzy file picker
            let files = collect_project_files(&repl.state.working_directory);
            let items: Vec<crate::widgets::select::SelectItem<String>> = files.into_iter()
                .map(|f| crate::widgets::select::SelectItem::new(f.clone(), f))
                .collect();
            let mut picker = crate::widgets::select::FuzzyPickerWidget::new("Pick file...".to_string())
                .with_items(items);
            picker.start_search();
            repl.state.fuzzy_picker = Some(picker);
            repl.state.file_selector_for_at = true;
            Ok(())
        }
        // Ctrl+K: kill to end of line
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            repl.prompt.kill_line();
            update_auto_completions(repl);
            Ok(())
        }
        // Ctrl+U: kill to start of line
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            repl.prompt.kill_to_start();
            update_auto_completions(repl);
            Ok(())
        }
        // Ctrl+A: move to start of line (readline convention)
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let col = repl.prompt.cursor_position();
            for _ in 0..col { repl.prompt.cursor_left(); }
            Ok(())
        }
        KeyCode::Char(c) => {
            repl.prompt.add_char(c);
            update_auto_completions(repl);
            Ok(())
        }
        KeyCode::Backspace => {
            repl.prompt.backspace();
            update_auto_completions(repl);
            Ok(())
        }
        KeyCode::Delete => {
            repl.prompt.delete_forward();
            update_auto_completions(repl);
            Ok(())
        }
        KeyCode::Up => {
            if !repl.state.completion_suggestions.is_empty() {
                if repl.state.completion_suggestion_index > 0 {
                    repl.state.completion_suggestion_index -= 1;
                }
                return Ok(());
            }
            if repl.prompt.input().contains('\n') {
                repl.prompt.cursor_up();
            } else if !repl.command_history.is_empty() {
                if repl.command_history.cursor() < 0 {
                    repl.saved_input = repl.prompt.input().to_string();
                }
                if let Some(cmd) = repl.command_history.up() {
                    repl.state.completion_suggestions.clear();
                    repl.state.completion_suggestion_index = 0;
                    repl.prompt.set_input(cmd.to_string());
                }
            }
            Ok(())
        }
        KeyCode::Down => {
            if !repl.state.completion_suggestions.is_empty() {
                if repl.state.completion_suggestion_index + 1 < repl.state.completion_suggestions.len() {
                    repl.state.completion_suggestion_index += 1;
                }
                return Ok(());
            }
            if repl.prompt.input().contains('\n') {
                repl.prompt.cursor_down();
            } else if repl.command_history.cursor() >= 0 {
                if let Some(cmd) = repl.command_history.down() {
                    repl.state.completion_suggestions.clear();
                    repl.state.completion_suggestion_index = 0;
                    repl.prompt.set_input(cmd.to_string());
                } else {
                    repl.command_history.reset_cursor();
                    repl.prompt.set_input(repl.saved_input.clone());
                }
            }
            Ok(())
        }
        KeyCode::Esc => {
            if !repl.state.completion_suggestions.is_empty() {
                repl.state.completion_suggestions.clear();
                repl.state.completion_suggestion_index = 0;
                return Ok(());
            }
            // Double-Esc on empty input triggers /undo
            let now = std::time::Instant::now();
            let input_empty = repl.prompt.input().trim().is_empty();
            if input_empty {
                if let Some(last) = repl.state.last_esc_time {
                    if now.duration_since(last).as_millis() < 500 {
                        repl.state.last_esc_time = None;
                        super::commands::handle_command(repl, "/undo")?;
                        return Ok(());
                    }
                }
                repl.state.last_esc_time = Some(now);
            } else {
                repl.state.last_esc_time = None;
            }
            let action = repl.vim_handler.process_key(key);
            let mode_str = repl.vim_handler.mode().to_string();
            repl.state.vim_mode = mode_str.clone();
            repl.prompt.set_vim_mode(&mode_str);
            handle_vim_action(repl, action);
            Ok(())
        }
        KeyCode::Left => {
            repl.prompt.cursor_left();
            Ok(())
        }
        KeyCode::Right => {
            repl.prompt.cursor_right();
            Ok(())
        }
        KeyCode::Tab => {
            if repl.state.streaming_active {
                let input = repl.prompt.input();
                if !input.trim().is_empty() {
                    repl.state.queued_message = Some(input);
                    repl.prompt.clear();
                    repl.state.status = "Message queued (will send after current response)".to_string();
                }
            } else if !repl.state.completion_suggestions.is_empty() {
                accept_completion(repl);
            } else {
                handle_tab_completion(repl)?;
            }
            Ok(())
        }
        KeyCode::BackTab => {
            repl.cycle_approval_mode();
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Handle vim actions produced by the VimHandler
fn handle_vim_action(repl: &mut Repl, action: VimAction) {
    match action {
        VimAction::YankLine { count } => {
            let line = repl.prompt.current_line();
            let yanked = if count > 1 { line.repeat(count) } else { line };
            repl.vim_handler.set_yank_buffer(yanked);
        }
        VimAction::PasteAfter => {
            let text = repl.vim_handler.yank_buffer().to_string();
            if !text.is_empty() {
                repl.prompt.insert_text(&text);
            }
        }
        VimAction::InsertChar { c } => {
            repl.prompt.add_char(c);
        }
        VimAction::Backspace => {
            repl.prompt.backspace();
        }
        VimAction::SubmitInput => {
            if let Err(e) = super::commands::submit_input(repl, None) {
                repl.chat.add_message(ChatRole::System, format!("Input error: {e}"));
            }
        }
        VimAction::MoveCursor { direction, count } => {
            for _ in 0..count {
                match direction {
                    Direction::Left => repl.prompt.cursor_left(),
                    Direction::Right => repl.prompt.cursor_right(),
                    Direction::Up => repl.prompt.cursor_up(),
                    Direction::Down => repl.prompt.cursor_down(),
                    Direction::LineStart => {
                        let col = repl.prompt.cursor_position();
                        for _ in 0..col { repl.prompt.cursor_left(); }
                    }
                    Direction::LineEnd => {
                        let len = repl.prompt.current_line_len();
                        let col = repl.prompt.cursor_position();
                        for _ in 0..len.saturating_sub(col) { repl.prompt.cursor_right(); }
                    }
                    Direction::FileStart => {
                        // Move to first line first
                        let row = repl.prompt.cursor_row();
                        for _ in 0..row { repl.prompt.cursor_up(); }
                        // Then move to start of line
                        let col = repl.prompt.cursor_position();
                        for _ in 0..col { repl.prompt.cursor_left(); }
                    }
                    Direction::FileEnd => {
                        // Move to last line first
                        let row = repl.prompt.cursor_row();
                        let last_row = repl.prompt.line_count().saturating_sub(1);
                        for _ in 0..last_row.saturating_sub(row) { repl.prompt.cursor_down(); }
                        // Then move to end of line
                        let len = repl.prompt.current_line_len();
                        let col = repl.prompt.cursor_position();
                        for _ in 0..len.saturating_sub(col) { repl.prompt.cursor_right(); }
                    }
                    Direction::WordForward => {
                        repl.prompt.cursor_word_forward();
                    }
                    Direction::WordBackward => {
                        repl.prompt.cursor_word_back();
                    }
                }
            }
        }
        VimAction::DeleteLine { .. } => {
            let line = repl.prompt.delete_current_line();
            repl.vim_handler.set_yank_buffer(line);
        }
        VimAction::DeleteChar { count } => {
            for _ in 0..count {
                repl.prompt.delete_forward();
            }
        }
        VimAction::ClearInput => {
            repl.prompt.clear();
        }
        VimAction::Scroll { direction, count } => {
            use crate::vim::ScrollDirection;
            let vh = repl.chat.chat_viewport_height() as usize;
            for _ in 0..count.max(1) {
                match direction {
                    ScrollDirection::Up => repl.chat.scroll_up(),
                    ScrollDirection::Down => repl.chat.scroll_down(),
                    ScrollDirection::HalfPageUp => {
                        repl.chat.scroll_up_by(vh / 2);
                    }
                    ScrollDirection::HalfPageDown => {
                        repl.chat.scroll_down_by(vh / 2);
                    }
                    ScrollDirection::FullPageUp => {
                        repl.chat.scroll_up_by(vh.saturating_sub(2).max(1));
                    }
                    ScrollDirection::FullPageDown => {
                        repl.chat.scroll_down_by(vh.saturating_sub(2).max(1));
                    }
                }
            }
        }
        VimAction::EnterInsertModeAppend => {
            repl.prompt.cursor_right();
        }
        VimAction::EnterInsertModeBelow => {
            let len = repl.prompt.current_line_len();
            let col = repl.prompt.cursor_position();
            for _ in 0..len.saturating_sub(col) { repl.prompt.cursor_right(); }
            repl.prompt.insert_newline();
        }
        VimAction::EnterInsertModeAbove => {
            let col = repl.prompt.cursor_position();
            for _ in 0..col { repl.prompt.cursor_left(); }
            repl.prompt.insert_newline();
            repl.prompt.cursor_up();
        }
        VimAction::DeleteWord { count } => {
            let mut deleted = String::new();
            for _ in 0..count {
                let word = repl.prompt.current_word();
                if word.is_empty() { break; }
                deleted.push_str(&word);
                for _ in 0..word.chars().count() {
                    repl.prompt.delete_forward();
                }
            }
            if !deleted.is_empty() {
                repl.vim_handler.set_yank_buffer(deleted);
            }
        }
        VimAction::YankWord { count } => {
            let mut yanked = String::new();
            for _ in 0..count {
                let word = repl.prompt.current_word();
                if word.is_empty() { break; }
                yanked.push_str(&word);
            }
            if !yanked.is_empty() {
                repl.vim_handler.set_yank_buffer(yanked);
            }
        }
        VimAction::Quit => {
            repl.running = false;
        }
        _ => {}
    }
}

/// Handle tab completion
fn handle_tab_completion(repl: &mut Repl) -> Result<()> {
    let input = repl.prompt.input().to_string();

    let command_names = repl.runtime.block_on(async {
        repl.shared_executor.registry().await.list_names_with_aliases().await
    });

    // Plugin commands are included via shared_executor registry above

    if let Some((completion, start, end)) = tab_complete(repl, &input, &command_names) {
        let mut new_input = String::new();
        if start > 0 {
            let safe_start = input.char_indices()
                .take_while(|(i, _)| *i < start)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            new_input.push_str(&input[..safe_start]);
        }
        new_input.push_str(&completion);
        if end < input.len() {
            let safe_end = input.char_indices()
                .take_while(|(i, _)| *i < end)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(input.len());
            if safe_end < input.len() {
                new_input.push_str(&input[safe_end..]);
            }
        }
        repl.prompt.set_input(new_input);
    }

    Ok(())
}

/// Determine if a word looks like a file path
pub(crate) fn looks_like_path(word: &str) -> bool {
    word.starts_with('/')
        || word.starts_with("./")
        || word.starts_with("../")
        || word.starts_with('~')
        || (word.contains('/') && !word.starts_with('/'))
}

/// Complete a file system path.
///
/// Expands `~` to the home directory, lists matching entries in the parent
/// directory, and appends `/` to directories. Returns up to 20 candidates.
pub(crate) fn complete_file_path(prefix: &str) -> Vec<String> {
    use std::path::PathBuf;

    let expanded = if prefix.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            prefix.replacen('~', &home.to_string_lossy(), 1)
        } else {
            prefix.to_string()
        }
    } else {
        prefix.to_string()
    };

    let path = PathBuf::from(&expanded);
    let (parent_dir, file_prefix) = if expanded.ends_with('/') {
        (path.clone(), String::new())
    } else {
        (
            path.parent().unwrap_or_else(|| std::path::Path::new(".")).to_path_buf(),
            path.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default(),
        )
    };

    let entries = match std::fs::read_dir(&parent_dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut candidates: Vec<String> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with(&file_prefix) {
                return None;
            }
            // Reconstruct with original prefix style
            let suffix = &name[file_prefix.len()..];
            if entry.path().is_dir() {
                Some(format!("{prefix}{suffix}/"))
            } else {
                Some(format!("{prefix}{suffix}"))
            }
        })
        .take(20)
        .collect();

    candidates.sort();
    candidates
}

/// Complete command arguments based on the command name.
///
/// Known commands return subcommand or value suggestions:
/// - `/team` → subcommands (create, add, task, assign, status, list, run, shutdown)
/// - `/model` → common model names
/// - `/doctor` / `/check` → check names
/// - `/config` → actions (list, get, set, reset)
/// - `/credentials` → actions (list, store, get, delete, count)
/// - `/worktree` → actions (enter, exit, status)
/// - `/debug` → subcommands (info, log, profile, trace)
pub(crate) fn complete_command_args(cmd_name: &str, prefix: &str) -> Vec<String> {
    let candidates: &[&str] = match cmd_name {
        "team" => &["create", "add", "task", "assign", "status", "list", "run", "shutdown", "help"],
        "model" => &[
            "claude-sonnet-4-6", "claude-opus-4-7", "claude-haiku-4-5",
            "claude-sonnet-4-5", "claude-opus-4-5",
            "claude-3-5-sonnet", "claude-3-opus",
            "gpt-4o", "gpt-4o-mini", "gpt-4-turbo", "o3", "o4-mini",
            "ollama/llama3", "ollama/mistral", "ollama/codellama", "ollama/qwen3",
        ],
        "doctor" | "check" | "diagnostics" => &[],
        "config" => &["list", "get", "set", "reset", "help"],
        "credentials" | "creds" | "cred" => &["list", "store", "get", "delete", "count", "help"],
        "worktree" => &["enter", "exit", "status"],
        "debug" | "dbg" | "dev" => &["info", "log", "profile", "trace", "help"],
        "diff" => &["--staged", "--stat", "--overview", "--word-diff"],
        "ci" => &["status", "runs", "workflows", "view", "trigger"],
        "compact" => &["status", "truncate", "micro", "group", "--preview"],
        "permissions" | "perm" | "perms" => &["allow", "deny", "reset", "status"],
        "plan" => &["create", "approve", "reject", "done", "status"],
        "review" => &["HEAD~1", "main...HEAD", "--staged", "--full"],
        "history" => &["--export"],
        "export" | "save" => &["--format json", "--format markdown"],
        "theme" => &[
            "dark", "light", "dracula", "tokyonight", "catppuccin_mocha",
            "gruvbox_dark", "nord", "kanagawa", "monokai", "onedark",
            "everforest", "ayu", "flexoki", "dark_daltonized", "light_daltonized",
            "pick",
        ],
        _ => &[],
    };

    candidates
        .iter()
        .filter(|c| c.starts_with(prefix))
        .map(|c| (*c).to_string())
        .collect()
}

/// Generate completion candidates based on current input for auto-popup display.
pub(crate) fn update_auto_completions(repl: &mut Repl) {
    let input = repl.prompt.input().to_string();
    if input.is_empty() {
        repl.state.completion_suggestions.clear();
        repl.state.completion_suggestion_index = 0;
        return;
    }

    let command_names = repl.runtime.block_on(async {
        repl.shared_executor.registry().await.list_names_with_aliases().await
    });

    let (prefix, _word_start, _word_end) = extract_completion_word(&input, repl);

    let has_space = input.trim_end_matches(' ').contains(' ');
    let candidates: Vec<String> = if !has_space && prefix.starts_with('/') {
        command_names.iter()
            .filter(|cmd| format!("/{cmd}").starts_with(&prefix))
            .map(|cmd| format!("/{cmd}"))
            .collect()
    } else if has_space && looks_like_path(&prefix) {
        complete_file_path(&prefix)
    } else if has_space {
        let cmd_name = input.split_whitespace().next().unwrap_or("").trim_start_matches('/');
        complete_command_args(cmd_name, &prefix)
    } else {
        Vec::new()
    };

    repl.tab_completion_state.last_prefix = prefix;
    repl.tab_completion_state.candidates = candidates.clone();
    repl.tab_completion_state.current_index = 0;
    repl.state.completion_suggestions = candidates;
    repl.state.completion_suggestion_index = 0;
}

/// Accept the currently selected completion suggestion and dismiss the popup.
fn accept_completion(repl: &mut Repl) {
    let idx = repl.state.completion_suggestion_index;
    if let Some(selected) = repl.state.completion_suggestions.get(idx).cloned() {
        let input = repl.prompt.input().to_string();
        let (_prefix, word_start, _word_end) = extract_completion_word(&input, repl);

        let mut new_input = String::new();
        if word_start > 0 {
            let safe_end = input.char_indices()
                .take_while(|(i, _)| *i < word_start)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            new_input.push_str(&input[..safe_end]);
        }
        new_input.push_str(&selected);
        if selected.starts_with('/') && !selected.contains(' ') {
            new_input.push(' ');
        }

        repl.prompt.set_input(new_input);
    }

    repl.state.completion_suggestions.clear();
    repl.state.completion_suggestion_index = 0;
    repl.tab_completion_state.candidates.clear();
    repl.tab_completion_state.current_index = 0;
}

/// Perform tab completion on the current input.
///
/// Routes between three completion contexts:
/// 1. Command name completion (input starts with `/`, no space)
/// 2. File path completion (argument word looks like a path)
/// 3. No completion (fallback)
///
/// Returns the completed text and the range to replace (start, end).
fn tab_complete(repl: &mut Repl, input: &str, available_commands: &[String]) -> Option<(String, usize, usize)> {
    let (prefix, word_start, word_end) = extract_completion_word(input, repl);

    // Reset completion state if the prefix changed
    if repl.tab_completion_state.last_prefix != prefix || repl.tab_completion_state.candidates.is_empty() {
        repl.tab_completion_state.last_prefix = prefix.clone();
        repl.tab_completion_state.current_index = 0;

        // Determine completion context
        let has_space = input.trim_end_matches(' ').contains(' ');

        repl.tab_completion_state.candidates = if !has_space && prefix.starts_with('/') {
            // Command name completion mode
            available_commands
                .iter()
                .filter(|cmd| {
                    let with_slash = format!("/{cmd}");
                    with_slash.starts_with(&prefix)
                })
                .map(|cmd| format!("/{cmd}"))
                .collect()
        } else if !has_space && prefix.is_empty() {
            // Empty input — show all commands
            available_commands.iter().map(|c| format!("/{c}")).collect()
        } else if has_space && looks_like_path(&prefix) {
            // File path completion mode
            complete_file_path(&prefix)
        } else if has_space {
            // Command argument completion
            let cmd_name = input.split_whitespace().next().unwrap_or("").trim_start_matches('/');
            complete_command_args(cmd_name, &prefix)
        } else {
            Vec::new()
        };

        // Update visual suggestions
        repl.state.completion_suggestions = repl.tab_completion_state.candidates.clone();
    }

    if repl.tab_completion_state.candidates.is_empty() {
        repl.state.completion_suggestions.clear();
        return None;
    }

    let completion = &repl.tab_completion_state.candidates[repl.tab_completion_state.current_index];
    repl.state.completion_suggestion_index = repl.tab_completion_state.current_index;
    repl.tab_completion_state.current_index = (repl.tab_completion_state.current_index + 1)
        % repl.tab_completion_state.candidates.len();

    Some((completion.clone(), word_start, word_end))
}

/// Extract the word to complete from input.
///
/// Returns `(prefix, word_start, word_end)` where:
/// - `prefix` is the text to match against candidates
/// - `word_start`/`word_end` is the byte range to replace in the input
///
/// Context detection:
/// - No space in input → command completion mode (whole input is the prefix)
/// - Space present → argument mode (last word after space is the prefix)
pub(crate) fn extract_completion_word(input: &str, _repl: &Repl) -> (String, usize, usize) {
    let trimmed = input.trim_end_matches(' ');
    if trimmed.is_empty() {
        return (String::new(), 0, 0);
    }

    let last_space = trimmed.rfind(' ');

    match last_space {
        None => {
            // No space — command completion mode
            (trimmed.to_string(), 0, input.len())
        }
        Some(space_pos) => {
            // Argument mode — extract last word after space
            let byte_pos = trimmed[..=space_pos].len();
            let word_start = byte_pos.min(trimmed.len());
            let word = trimmed.char_indices()
                .take_while(|(i, _)| *i < word_start)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .and_then(|safe_start| {
                    if safe_start < trimmed.len() { Some(&trimmed[safe_start..]) } else { None }
                })
                .unwrap_or("");
            (word.to_string(), word_start, input.len())
        }
    }
}

// ── Dialog Input Handlers ──────────────────────────────────────────

fn handle_permission_dialog_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    use shannon_core::permissions::PermissionChoice;

    match key.code {
        KeyCode::Enter => {
            send_permission_response(repl, PermissionChoice::AllowOnce);
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            send_permission_response(repl, PermissionChoice::AlwaysAllow);
        }
        KeyCode::Char('e') | KeyCode::Char('E') => {
            send_permission_response(repl, PermissionChoice::EditAndRun);
        }
        KeyCode::Esc => {
            send_permission_response(repl, PermissionChoice::Deny);
        }
        _ => {}
    }
    Ok(())
}

fn send_permission_response(repl: &mut Repl, choice: shannon_core::permissions::PermissionChoice) {
    if let Some(tx) = repl.state.permission_response_tx.take() {
        let _ = tx.send(choice);
    }
    repl.state.permission_dialog = None;
}

fn handle_active_dialog_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Left => {
            if let Some(ref mut dialog) = repl.state.active_dialog {
                dialog.prev_button();
            }
        }
        KeyCode::Right => {
            if let Some(ref mut dialog) = repl.state.active_dialog {
                dialog.next_button();
            }
        }
        KeyCode::Enter => {
            let action = repl.state.active_dialog.as_ref()
                .and_then(|d| d.selected_action().map(|a| a.to_string()));
            let pending = repl.state.pending_dialog_action.take();
            repl.state.active_dialog = None;

            if let Some(ref act) = action {
                match act.as_str() {
                    "confirm" => {
                        if let Some(cmd) = pending {
                            super::commands::execute_pending_action(repl, &cmd)?;
                        }
                    }
                    "cancel" | "ok" => {}
                    _ => {}
                }
            }
        }
        KeyCode::Esc => {
            repl.state.active_dialog = None;
            repl.state.pending_dialog_action = None;
        }
        _ => {}
    }
    Ok(())
}

fn handle_input_dialog_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char(c) => {
            if let Some(ref mut dlg) = repl.state.input_dialog {
                dlg.add_char(c);
            }
        }
        KeyCode::Backspace => {
            if let Some(ref mut dlg) = repl.state.input_dialog {
                dlg.backspace();
            }
        }
        KeyCode::Enter => {
            let value = repl.state.input_dialog.as_ref()
                .map(|d| d.value().to_string())
                .unwrap_or_default();
            let action = repl.state.input_dialog_action.take();
            repl.state.input_dialog = None;

            if let Some(ref act) = action {
                match act.as_str() {
                    "set_api_key" => {
                        if !value.is_empty() {
                            // SAFETY: REPL event loop is single-threaded; no concurrent reads of SHANNON_API_KEY.
                            unsafe { std::env::set_var("SHANNON_API_KEY", &value); }
                            repl.chat.add_message(
                                ChatRole::System,
                                "API key set for this session.".to_string(),
                            );
                        }
                    }
                    "set_model" => {
                        if !value.is_empty() {
                            repl.state.model = Some(value.clone());
                            repl.chat.add_message(
                                ChatRole::System,
                                format!("Model set to: {value}"),
                            );
                        }
                    }
                    _ => {
                        repl.chat.add_message(
                            ChatRole::System,
                            format!("Input received: {value}"),
                        );
                    }
                }
            }
        }
        KeyCode::Esc => {
            repl.state.input_dialog = None;
            repl.state.input_dialog_action = None;
        }
        _ => {}
    }
    Ok(())
}

fn open_command_palette(repl: &mut Repl) {
    let mut palette = crate::widgets::command_palette::CommandPaletteWidget::new();

    // Merge built-in commands from the registry into the palette
    let command_names = repl.runtime.block_on(repl.command_registry.list_names());
    for name in &command_names {
        let cmd = crate::widgets::command_palette::PaletteCommand {
            name: format!("/{name}"),
            description: String::new(),
            shortcut: None,
            category: crate::widgets::command_palette::CommandCategory::Tools,
            args_template: None,
            subcommands: vec![],
            use_count: 0,
        };
        palette.commands.push(cmd);
    }

    palette.show();
    repl.state.command_palette = Some(palette);
}

/// Handle key input for the command palette overlay
fn handle_command_palette_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    let action = match &mut repl.state.command_palette {
        Some(p) => match key.code {
            KeyCode::Up => { p.move_up(); None }
            KeyCode::Down => { p.move_down(); None }
            KeyCode::Char(c) => { p.add_char(c); None }
            KeyCode::Backspace => { p.backspace(); None }
            KeyCode::Enter => {
                let cmd = p.selected_command().map(|c| c.name.clone());
                Some(cmd)
            }
            KeyCode::Esc => Some(None),
            _ => None,
        },
        None => None,
    };

    match action {
        Some(Some(cmd)) => {
            repl.state.command_palette = None;
            repl.prompt.set_input(cmd);
            super::commands::submit_input(repl, None)?;
        }
        Some(None) => {
            repl.state.command_palette = None;
        }
        None => {}
    }
    Ok(())
}

/// Handle key input for the tool approval overlay
fn handle_tool_approval_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    let decision = repl.state.tool_approval.handle_key(key);

    match decision {
        Some(crate::widgets::tool_approval::ApprovalDecision::AllowOnce) => {
            repl.state.tool_approval.dismiss();
            // Forward to permission system
            if let Some(ref tx) = repl.state.permission_response_tx.take() {
                let _ = tx.send(shannon_core::permissions::PermissionChoice::AllowOnce);
            }
            repl.state.permission_dialog = None;
        }
        Some(crate::widgets::tool_approval::ApprovalDecision::AllowSession) => {
            // Auto-approve for the rest of the session
            if let Some(ref req) = repl.state.tool_approval.request {
                let tool_name = req.tool_name.clone();
                // For network tools with a domain, auto-approve only that domain
                let pattern = req.domain.clone().unwrap_or_else(|| "*".to_string());
                repl.state.tool_approval.auto_approve_rules.push(
                    crate::widgets::tool_approval::AutoApproveRule {
                        tool_name,
                        pattern,
                        approved: true,
                    }
                );
            }
            repl.state.tool_approval.dismiss();
            if let Some(ref tx) = repl.state.permission_response_tx.take() {
                let _ = tx.send(shannon_core::permissions::PermissionChoice::AlwaysAllow);
            }
            repl.state.permission_dialog = None;
        }
        Some(crate::widgets::tool_approval::ApprovalDecision::Deny) => {
            repl.state.tool_approval.dismiss();
            if let Some(ref tx) = repl.state.permission_response_tx.take() {
                let _ = tx.send(shannon_core::permissions::PermissionChoice::Deny);
            }
            repl.state.permission_dialog = None;
        }
        _ => {} // Pending — no decision yet
    }
    Ok(())
}

fn handle_fuzzy_picker_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Up => {
            if let Some(ref mut picker) = repl.state.fuzzy_picker {
                picker.move_up();
            }
        }
        KeyCode::Down => {
            if let Some(ref mut picker) = repl.state.fuzzy_picker {
                picker.move_down();
            }
        }
        KeyCode::Char(c) => {
            if let Some(ref mut picker) = repl.state.fuzzy_picker {
                picker.add_search_char(c);
            }
        }
        KeyCode::Backspace => {
            if let Some(ref mut picker) = repl.state.fuzzy_picker {
                picker.remove_search_char();
            }
        }
        KeyCode::Enter => {
            let selected = repl.state.fuzzy_picker.as_ref()
                .and_then(|p| p.selected_value().map(|v| v.to_string()));
            let is_file_pick = repl.state.file_selector_for_at;
            let is_session_pick = repl.state.session_picker_active;
            repl.state.fuzzy_picker = None;
            repl.state.file_selector_for_at = false;
            repl.state.session_picker_active = false;

            if let Some(value) = selected {
                if is_file_pick {
                    // Insert selected file path into prompt
                    let current = repl.prompt.input().to_string();
                    // Remove the trailing @ that triggered the picker
                    let trimmed = current.trim_end_matches('@');
                    repl.prompt.set_input(format!("{trimmed}@{value} "));
                } else if is_session_pick {
                    // Resume selected session
                    super::commands::handle_command(repl, &format!("/resume {value}"))?;
                } else {
                    repl.prompt.set_input(value);
                    super::commands::submit_input(repl, None)?;
                }
            }
        }
        KeyCode::Esc => {
            repl.state.fuzzy_picker = None;
            repl.state.file_selector_for_at = false;
        }
        _ => {}
    }
    Ok(())
}

fn handle_file_selector_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(ref mut sel) = repl.state.file_selector {
                sel.move_up();
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(ref mut sel) = repl.state.file_selector {
                sel.move_down();
            }
        }
        KeyCode::Enter => {
            if let Some(ref mut sel) = repl.state.file_selector {
                if let Some(selection) = sel.current_selection() {
                    let full_path = std::path::Path::new(sel.current_path()).join(&selection);
                    if full_path.is_dir() {
                        let dir_name = selection.clone();
                        if let Err(e) = sel.navigate_into(&dir_name) {
                            repl.chat.add_message(
                                ChatRole::System,
                                format!("Failed to navigate into {dir_name}: {e}"),
                            );
                        }
                        return Ok(());
                    }
                }
            }

            let selected_path = repl.state.file_selector.as_ref()
                .and_then(|s| s.current_selection())
                .map(|name| {
                    let base = repl.state.file_selector.as_ref()
                        .map(|s| s.current_path().to_string())
                        .unwrap_or_else(|| ".".to_string());
                    format!("{base}/{name}")
                });
            repl.state.file_selector = None;

            if let Some(path) = selected_path {
                // Enhance @ reference: detect PDF/URL/directory and process
                let result = super::at_reference::detect_reference_kind(&path);
                match result {
                    super::at_reference::AtReferenceKind::Pdf => {
                        let processed = super::at_reference::extract_pdf_text(&path);
                        if processed.is_error {
                            repl.chat.add_message(ChatRole::System, processed.status_message.unwrap_or_default());
                            repl.prompt.set_input(path);
                        } else {
                            repl.prompt.set_input(processed.injected_text);
                            if let Some(msg) = processed.status_message {
                                repl.chat.add_message(ChatRole::System, msg);
                            }
                        }
                    }
                    super::at_reference::AtReferenceKind::Directory => {
                        let processed = super::at_reference::generate_directory_tree(&path, None);
                        if processed.is_error {
                            repl.chat.add_message(ChatRole::System, processed.status_message.unwrap_or_default());
                            repl.prompt.set_input(path);
                        } else {
                            repl.prompt.set_input(processed.injected_text);
                            if let Some(msg) = processed.status_message {
                                repl.chat.add_message(ChatRole::System, msg);
                            }
                        }
                    }
                    super::at_reference::AtReferenceKind::Url(url) => {
                        let processed = super::at_reference::fetch_url_content(&url);
                        if processed.is_error {
                            repl.chat.add_message(ChatRole::System, processed.status_message.unwrap_or_default());
                            repl.prompt.set_input(path);
                        } else {
                            repl.prompt.set_input(processed.injected_text);
                            if let Some(msg) = processed.status_message {
                                repl.chat.add_message(ChatRole::System, msg);
                            }
                        }
                    }
                    super::at_reference::AtReferenceKind::File => {
                        repl.prompt.set_input(path);
                        repl.chat.add_message(
                            ChatRole::System,
                            "File selected — press Enter to send as query, or edit the path.".to_string(),
                        );
                    }
                }
            }
        }
        KeyCode::Backspace => {
            if let Some(ref mut sel) = repl.state.file_selector {
                if let Err(e) = sel.navigate_up() {
                    repl.chat.add_message(
                        ChatRole::System,
                        format!("Failed to navigate up: {e}"),
                    );
                }
            }
        }
        KeyCode::Esc => {
            repl.state.file_selector = None;
        }
        _ => {}
    }
    Ok(())
}

fn handle_model_picker_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(ref mut mp) = repl.state.model_picker {
                mp.move_up();
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(ref mut mp) = repl.state.model_picker {
                mp.move_down();
            }
        }
        KeyCode::Left | KeyCode::Char('h') => {
            if let Some(ref mut mp) = repl.state.model_picker {
                mp.prev_provider();
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if let Some(ref mut mp) = repl.state.model_picker {
                mp.next_provider();
            }
        }
        KeyCode::Enter => {
            let model_id = repl.state.model_picker.as_ref()
                .and_then(|mp| mp.selected_model().map(|m| m.id.to_string()));
            repl.state.model_picker = None;

            if let Some(id) = model_id {
                repl.state.model = Some(id);
                crate::repl::preferences::save_preferences(
                    &crate::repl::preferences::Preferences {
                        model: repl.state.model.clone(),
                        provider: repl.state.selected_provider.clone(),
                        theme: Some(repl.state.theme.name.to_string()),
                    },
                );
                repl.chat.add_message(
                    ChatRole::System,
                    t!("commands.model.set", name = repl.state.model.as_deref().unwrap_or("")).to_string(),
                );
            }
        }
        KeyCode::Esc => {
            repl.state.model_picker = None;
        }
        _ => {}
    }
    Ok(())
}

fn handle_theme_picker_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(ref mut picker) = repl.state.theme_picker {
                picker.move_up();
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(ref mut picker) = repl.state.theme_picker {
                picker.move_down();
            }
        }
        KeyCode::Char(c) if c != 'j' && c != 'k' => {
            if let Some(ref mut picker) = repl.state.theme_picker {
                picker.add_search_char(c);
            }
        }
        KeyCode::Backspace => {
            if let Some(ref mut picker) = repl.state.theme_picker {
                picker.remove_search_char();
            }
        }
        KeyCode::Enter => {
            let selected = repl.state.theme_picker.as_ref()
                .and_then(|p| p.selected_value().map(|v| v.to_string()));
            repl.state.theme_picker = None;

            if let Some(name) = selected {
                if let Some(theme) = crate::theme::Theme::named(&name) {
                    repl.renderer.set_theme(&theme);
                    repl.state.theme = theme;
                    crate::repl::preferences::save_preferences(
                        &crate::repl::preferences::Preferences {
                            model: repl.state.model.clone(),
                            provider: repl.state.selected_provider.clone(),
                            theme: Some(name.clone()),
                        },
                    );
                    repl.chat.add_message(
                        crate::widgets::ChatRole::System,
                        format!("Theme switched to '{name}'."),
                    );
                }
            }
        }
        KeyCode::Esc => {
            repl.state.theme_picker = None;
        }
        _ => {}
    }
    Ok(())
}

fn handle_multi_select_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(ref mut sel) = repl.state.multi_select {
                sel.move_up();
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(ref mut sel) = repl.state.multi_select {
                sel.move_down();
            }
        }
        KeyCode::Char(' ') => {
            if let Some(ref mut sel) = repl.state.multi_select {
                sel.toggle_current();
            }
        }
        KeyCode::Char('a') => {
            if let Some(ref mut sel) = repl.state.multi_select {
                sel.select_all();
            }
        }
        KeyCode::Char('d') => {
            if let Some(ref mut sel) = repl.state.multi_select {
                sel.deselect_all();
            }
        }
        KeyCode::Enter => {
            let values: Vec<String> = repl.state.multi_select
                .as_ref()
                .map(|sel| sel.selected_values().iter().map(|v| v.to_string()).collect())
                .unwrap_or_default();
            repl.state.multi_select = None;

            if values.is_empty() {
                repl.chat.add_message(ChatRole::System, "No items selected.".to_string());
            } else {
                repl.chat.add_message(ChatRole::System, format!("Selected: {}", values.join(", ")));
            }
        }
        KeyCode::Esc => {
            repl.state.multi_select = None;
        }
        _ => {}
    }
    Ok(())
}

/// Dismiss the diff viewer overlay and reset interactive state.
fn dismiss_diff_viewer(repl: &mut Repl) {
    repl.state.diff_viewer = None;
    repl.state.diff_interactive = false;
    repl.state.interactive_hunks.clear();
    repl.state.interactive_selected = 0;
}

/// Handle key input for the plan review overlay.
fn handle_plan_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    let total_lines = repl.state.plan.content.lines().count();
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            repl.state.plan.scroll_offset = repl.state.plan.scroll_offset.saturating_add(1).min(total_lines);
            Ok(())
        }
        KeyCode::Char('k') | KeyCode::Up => {
            repl.state.plan.scroll_offset = repl.state.plan.scroll_offset.saturating_sub(1);
            Ok(())
        }
        KeyCode::Enter => {
            repl.state.plan.approved = true;
            repl.state.status = "Plan approved — executing".to_string();
            Ok(())
        }
        KeyCode::Esc => {
            repl.state.plan.active = false;
            repl.state.plan.scroll_offset = 0;
            repl.state.status = "Plan rejected".to_string();
            Ok(())
        }
        KeyCode::Char('p') | KeyCode::Char('P') => {
            repl.state.plan.active = false;
            repl.state.plan.scroll_offset = 0;
            repl.state.status = "Ready".to_string();
            Ok(())
        }
        _ => Ok(())
    }
}

/// Handle key input for the diff viewer overlay (both normal and interactive modes).
fn handle_diff_viewer_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    use crate::widgets::diff_viewer::HunkAction;

    match key.code {
        // Dismiss
        KeyCode::Esc | KeyCode::Char('q') => {
            dismiss_diff_viewer(repl);
        }

        // Navigation
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(ref mut viewer) = repl.state.diff_viewer {
                if repl.state.diff_interactive {
                    if repl.state.interactive_selected + 1 < repl.state.interactive_hunks.len() {
                        repl.state.interactive_selected += 1;
                    }
                } else if viewer.detail_file.is_some() {
                    viewer.scroll_offset = viewer.scroll_offset.saturating_add(1);
                } else {
                    // In file list mode: move selection down
                    let file_count = {
                        let mut count = 0usize;
                        let mut seen = std::collections::HashSet::new();
                        for turn in repl.diff_data.get_session_diffs() {
                            for fc in &turn.files_modified {
                                if seen.insert(fc.path.clone()) { count += 1; }
                            }
                            count += turn.files_created.len() + turn.files_deleted.len();
                        }
                        count.max(1)
                    };
                    viewer.move_down(file_count);
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(ref mut viewer) = repl.state.diff_viewer {
                if repl.state.diff_interactive {
                    repl.state.interactive_selected = repl.state.interactive_selected.saturating_sub(1);
                } else if viewer.detail_file.is_some() {
                    viewer.scroll_offset = viewer.scroll_offset.saturating_sub(1);
                } else {
                    // In file list mode: move selection up
                    viewer.move_up();
                    viewer.scroll_offset = viewer.scroll_offset.saturating_sub(1);
                }
            }
        }

        // Enter: drill into file detail (non-interactive, file list mode)
        KeyCode::Enter if !repl.state.diff_interactive => {
            if let Some(ref mut viewer) = repl.state.diff_viewer {
                if viewer.detail_file.is_none() {
                    // Enter file detail mode
                    if let Some(path) = viewer.get_selected_path(&repl.diff_data) {
                        viewer.load_diff(&path);
                        viewer.detail_file = Some(path);
                        viewer.scroll_offset = 0;
                    }
                }
            }
        }

        // Backspace: return to file list from detail view
        KeyCode::Backspace => {
            if let Some(ref mut viewer) = repl.state.diff_viewer {
                if viewer.detail_file.is_some() {
                    viewer.detail_file = None;
                    viewer.scroll_offset = 0;
                }
            }
        }

        // Interactive-only actions
        KeyCode::Char('a') if repl.state.diff_interactive => {
            let idx = repl.state.interactive_selected;
            if idx < repl.state.interactive_hunks.len() {
                repl.state.interactive_hunks[idx].action = HunkAction::Accepted;
            }
        }
        KeyCode::Char('r') if repl.state.diff_interactive => {
            let idx = repl.state.interactive_selected;
            if idx < repl.state.interactive_hunks.len() {
                repl.state.interactive_hunks[idx].action = HunkAction::Rejected;
            }
        }
        KeyCode::Char('A') if repl.state.diff_interactive => {
            for hunk in &mut repl.state.interactive_hunks {
                hunk.action = HunkAction::Accepted;
            }
        }
        KeyCode::Char('R') if repl.state.diff_interactive => {
            for hunk in &mut repl.state.interactive_hunks {
                hunk.action = HunkAction::Rejected;
            }
        }
        KeyCode::Enter if repl.state.diff_interactive => {
            // Collect accepted content per file and apply via git apply
            let accepted: Vec<_> = repl.state.interactive_hunks.iter()
                .filter(|h| h.action == HunkAction::Accepted)
                .collect();
            if accepted.is_empty() {
                dismiss_diff_viewer(repl);
            } else {
                // Reconstruct accepted diff and apply with git apply
                let mut patch = String::new();
                for hunk in &repl.state.interactive_hunks {
                    if hunk.action == HunkAction::Accepted {
                        for line in &hunk.lines {
                            patch.push_str(line);
                            patch.push('\n');
                        }
                    }
                }
                // Write patch to temp file and apply
                let tmp = std::env::temp_dir().join("shannon-interactive-diff.patch");
                if let Ok(()) = std::fs::write(&tmp, &patch) {
                    let output = std::process::Command::new("git")
                        .args(["apply", "--allow-empty", tmp.to_str().unwrap_or("")])
                        .current_dir(&repl.state.working_directory)
                        .output();
                    match output {
                        Ok(o) if o.status.success() => {
                            let count = accepted.len();
                            dismiss_diff_viewer(repl);
                            repl.chat.add_message(ChatRole::System, format!("Applied {count} accepted hunk(s)."));
                        }
                        Ok(o) => {
                            let err = String::from_utf8_lossy(&o.stderr);
                            repl.chat.add_message(ChatRole::System, format!("git apply failed: {err}"));
                        }
                        Err(e) => {
                            repl.chat.add_message(ChatRole::System, format!("Failed to run git apply: {e}"));
                        }
                    }
                    let _ = std::fs::remove_file(&tmp);
                } else {
                    repl.chat.add_message(ChatRole::System, "Failed to write patch file.".to_string());
                }
            }
        }

        _ => {}
    }
    Ok(())
}

/// Collect project files for the @ fuzzy picker.
/// Skips hidden directories (except useful config dirs), common build/artifact dirs, and .git.
fn collect_project_files(root: &str) -> Vec<String> {
    let mut files = Vec::new();
    let skip_dirs: &[&str] = &[".git", "node_modules", "target", "__pycache__", ".next", "dist", "build", ".cache"];
    let visible_dot_dirs: &[&str] = &[".claude", ".shannon", ".github", ".vscode", ".env", ".direnv"];

    fn walk(dir: &std::path::Path, root: &std::path::Path, skip_dirs: &[&str], visible_dot_dirs: &[&str], files: &mut Vec<String>, depth: usize) {
        if depth > 8 { return; }
        let entries = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') && !visible_dot_dirs.contains(&name) { continue; }
            if path.is_dir() {
                if skip_dirs.contains(&name) { continue; }
                walk(&path, root, skip_dirs, visible_dot_dirs, files, depth + 1);
            } else if let Ok(rel) = path.strip_prefix(root) {
                files.push(rel.to_string_lossy().to_string());
            }
        }
    }

    let root_path = std::path::Path::new(root);
    walk(root_path, root_path, skip_dirs, visible_dot_dirs, &mut files, 0);

    // Limit to 500 files for performance
    files.truncate(500);
    files.sort();
    files
}

/// Handle keys when incremental history search (Ctrl+R) is active.
fn handle_incremental_search_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            repl.state.incremental_search_active = false;
            repl.state.incremental_search_query.clear();
            repl.state.incremental_search_match_index = 0;
            repl.state.incremental_search_match_count = 0;
            repl.prompt.set_input(std::mem::take(&mut repl.state.incremental_search_saved_input));
            Ok(())
        }
        KeyCode::Enter => {
            let matches = repl.command_history.search_history(&repl.state.incremental_search_query);
            let chosen = if matches.is_empty() {
                std::mem::take(&mut repl.state.incremental_search_query)
            } else {
                let idx = repl.state.incremental_search_match_index.min(matches.len() - 1);
                matches[idx].to_string()
            };
            repl.state.incremental_search_active = false;
            repl.state.incremental_search_query.clear();
            repl.state.incremental_search_match_index = 0;
            repl.state.incremental_search_match_count = 0;
            repl.state.incremental_search_saved_input.clear();
            repl.prompt.set_input(chosen);
            Ok(())
        }
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let matches = repl.command_history.search_history(&repl.state.incremental_search_query);
            repl.state.incremental_search_match_count = matches.len();
            if !matches.is_empty() {
                repl.state.incremental_search_match_index =
                    (repl.state.incremental_search_match_index + 1) % matches.len();
                repl.prompt.set_input(matches[repl.state.incremental_search_match_index].to_string());
            }
            Ok(())
        }
        KeyCode::Up => {
            let matches = repl.command_history.search_history(&repl.state.incremental_search_query);
            repl.state.incremental_search_match_count = matches.len();
            if !matches.is_empty() && repl.state.incremental_search_match_index > 0 {
                repl.state.incremental_search_match_index -= 1;
                repl.prompt.set_input(matches[repl.state.incremental_search_match_index].to_string());
            }
            Ok(())
        }
        KeyCode::Down => {
            let matches = repl.command_history.search_history(&repl.state.incremental_search_query);
            repl.state.incremental_search_match_count = matches.len();
            if !matches.is_empty() {
                repl.state.incremental_search_match_index =
                    (repl.state.incremental_search_match_index + 1).min(matches.len() - 1);
                repl.prompt.set_input(matches[repl.state.incremental_search_match_index].to_string());
            }
            Ok(())
        }
        KeyCode::Backspace => {
            repl.state.incremental_search_query.pop();
            repl.state.incremental_search_match_index = 0;
            let matches = repl.command_history.search_history(&repl.state.incremental_search_query);
            repl.state.incremental_search_match_count = matches.len();
            if let Some(first) = matches.first() {
                repl.prompt.set_input(first.to_string());
            } else {
                repl.prompt.set_input(repl.state.incremental_search_query.clone());
            }
            Ok(())
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            repl.state.incremental_search_query.push(c);
            repl.state.incremental_search_match_index = 0;
            let matches = repl.command_history.search_history(&repl.state.incremental_search_query);
            repl.state.incremental_search_match_count = matches.len();
            if let Some(first) = matches.first() {
                repl.prompt.set_input(first.to_string());
            } else {
                repl.prompt.set_input(repl.state.incremental_search_query.clone());
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Handle keys when the transcript pager is active.
fn handle_pager_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    match key.code {
        // Close pager
        KeyCode::Esc | KeyCode::Char('q') => {
            repl.state.pager_active = false;
            Ok(())
        }
        // Scroll down
        KeyCode::Down | KeyCode::Char('j') => {
            repl.pager_scroll(1);
            Ok(())
        }
        // Scroll up
        KeyCode::Up | KeyCode::Char('k') => {
            repl.pager_scroll(-1);
            Ok(())
        }
        // Scroll to top
        KeyCode::Char('g') => {
            repl.pager_scroll_top();
            Ok(())
        }
        // Scroll to bottom (Shift+G)
        KeyCode::Char('G') => {
            repl.pager_scroll_bottom();
            Ok(())
        }
        // Page down (scroll by ~half viewport in messages)
        KeyCode::PageDown | KeyCode::Char(' ') => {
            repl.pager_scroll(5);
            Ok(())
        }
        // Page up
        KeyCode::PageUp | KeyCode::Backspace => {
            repl.pager_scroll(-5);
            Ok(())
        }
        // Search within pager
        KeyCode::Char('/') => {
            repl.state.chat_search_active = true;
            repl.state.chat_search_query.clear();
            Ok(())
        }
        // Next/prev search match
        KeyCode::Char('n') => {
            repl.chat_search_next();
            Ok(())
        }
        KeyCode::Char('N') => {
            repl.chat_search_prev();
            Ok(())
        }
        _ => Ok(())
    }
}

/// Handle input while chat search is active (query typing mode).
fn handle_chat_search_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    match key.code {
        // Escape: close search, clear highlights
        KeyCode::Esc => {
            repl.state.chat_search_active = false;
            repl.state.chat_search_query.clear();
            repl.state.chat_search_total_matches = 0;
            Ok(())
        }
        // Enter: close search input, keep highlights and jump to first match
        KeyCode::Enter => {
            repl.state.chat_search_active = false;
            if repl.state.chat_search_total_matches > 0 {
                repl.chat_search_next();
            }
            Ok(())
        }
        // Backspace: remove last char
        KeyCode::Backspace => {
            repl.state.chat_search_query.pop();
            repl.update_chat_search();
            // Auto-scroll to first match
            if repl.state.chat_search_total_matches > 0 {
                let matches = repl.chat.find_search_matches(&repl.state.chat_search_query);
                if let Some(&(msg_idx, _, _)) = matches.first() {
                    repl.chat.scroll_offset = msg_idx;
                    repl.state.auto_follow = false;
                }
            }
            Ok(())
        }
        // Typing: append to query
        KeyCode::Char(c) => {
            repl.state.chat_search_query.push(c);
            repl.update_chat_search();
            // Auto-scroll to first match when query changes
            if repl.state.chat_search_total_matches > 0 {
                let matches = repl.chat.find_search_matches(&repl.state.chat_search_query);
                if let Some(&(msg_idx, _, _)) = matches.first() {
                    repl.chat.scroll_offset = msg_idx;
                    repl.state.auto_follow = false;
                }
            }
            Ok(())
        }
        _ => Ok(())
    }
}
