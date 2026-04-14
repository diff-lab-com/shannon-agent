//! REPL keyboard input handling

use crate::{
    widgets::ChatRole,
    Result,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crate::vim::{Direction, VimAction};
use crate::widgets::select::SelectItem;

use super::Repl;

/// Handle keyboard input — dispatches to the appropriate sub-handler.
pub fn handle_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
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

    // If multi-select is active, handle multi-select input
    if repl.state.multi_select.is_some() {
        return handle_multi_select_input(repl, key);
    }

    match key.code {
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            open_command_palette(repl);
            Ok(())
        }
        KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            repl.running = false;
            Ok(())
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            repl.running = false;
            Ok(())
        }
        KeyCode::Enter => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                repl.prompt.insert_newline();
            } else {
                super::commands::submit_input(repl)?;
            }
            Ok(())
        }
        KeyCode::Char(c) => {
            repl.prompt.add_char(c);
            Ok(())
        }
        KeyCode::Backspace => {
            repl.prompt.backspace();
            Ok(())
        }
        KeyCode::Up => {
            if repl.prompt.input().contains('\n') {
                repl.prompt.cursor_up();
            } else if !repl.prompt.input().is_empty() || repl.command_history.cursor() >= 0 {
                if repl.command_history.cursor() < 0 {
                    repl.saved_input = repl.prompt.input().to_string();
                }
                if let Some(cmd) = repl.command_history.up() {
                    repl.prompt.set_input(cmd.to_string());
                }
            } else {
                repl.chat.scroll_up();
            }
            Ok(())
        }
        KeyCode::Down => {
            if repl.prompt.input().contains('\n') {
                repl.prompt.cursor_down();
            } else if repl.command_history.cursor() >= 0 {
                if let Some(cmd) = repl.command_history.down() {
                    repl.prompt.set_input(cmd.to_string());
                } else {
                    repl.command_history.reset_cursor();
                    repl.prompt.set_input(repl.saved_input.clone());
                }
            } else {
                repl.chat.scroll_down();
            }
            Ok(())
        }
        KeyCode::Esc => {
            let action = repl.vim_handler.process_key(key);
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
            handle_tab_completion(repl)?;
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
            if let Err(e) = super::commands::submit_input(repl) {
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
                    Direction::LineStart | Direction::FileStart => {
                        let col = repl.prompt.cursor_position();
                        for _ in 0..col { repl.prompt.cursor_left(); }
                    }
                    Direction::LineEnd | Direction::FileEnd => {
                        for _ in 0..100 { repl.prompt.cursor_right(); }
                    }
                    Direction::WordForward | Direction::WordBackward => {}
                }
            }
        }
        VimAction::DeleteLine { .. } => {
            let line = repl.prompt.current_line();
            repl.vim_handler.set_yank_buffer(line);
            repl.prompt.clear();
        }
        VimAction::ClearInput => {
            repl.prompt.clear();
        }
        _ => {}
    }
}

/// Handle tab completion
fn handle_tab_completion(repl: &mut Repl) -> Result<()> {
    let input = repl.prompt.input().to_string();

    let mut command_names = repl.runtime.block_on(async {
        repl.shared_executor.registry().await.list_names().await
    });

    // Also include plugin commands in completion candidates
    for cmd in repl.plugin_manager.get_plugin_commands() {
        if !command_names.iter().any(|n| n == &cmd.name) {
            command_names.push(cmd.name.clone());
        }
    }

    if let Some((completion, start, end)) = tab_complete_command(repl, &input, &command_names) {
        let mut new_input = String::new();
        if start > 0 && start <= input.len() {
            new_input.push_str(&input[..start]);
        }
        new_input.push_str(&completion);
        if end < input.len() {
            new_input.push_str(&input[end..]);
        }
        repl.prompt.set_input(new_input);
    }

    Ok(())
}

/// Perform tab completion on the current input
///
/// Returns the completed text and the range to replace (start, end).
fn tab_complete_command(repl: &mut Repl, input: &str, available_commands: &[String]) -> Option<(String, usize, usize)> {
    let (prefix, word_start, word_end) = extract_completion_word(input, repl);

    // Reset completion state if the prefix changed
    if repl.tab_completion_state.last_prefix != prefix {
        repl.tab_completion_state.last_prefix = prefix.clone();
        repl.tab_completion_state.current_index = 0;

        repl.tab_completion_state.candidates = if prefix.starts_with('/') {
            available_commands
                .iter()
                .filter(|cmd| {
                    let with_slash = format!("/{cmd}");
                    with_slash.starts_with(&prefix)
                })
                .map(|cmd| format!("/{cmd}"))
                .collect()
        } else if prefix.is_empty() {
            available_commands.iter().map(|c| format!("/{c}")).collect()
        } else {
            Vec::new()
        };
    }

    if repl.tab_completion_state.candidates.is_empty() {
        return None;
    }

    let completion = &repl.tab_completion_state.candidates[repl.tab_completion_state.current_index];
    repl.tab_completion_state.current_index = (repl.tab_completion_state.current_index + 1)
        % repl.tab_completion_state.candidates.len();

    Some((completion.clone(), word_start, word_end))
}

/// Extract the word to complete from input
pub(crate) fn extract_completion_word(input: &str, _repl: &Repl) -> (String, usize, usize) {
    let start = if let Some(last_slash) = input.rfind('/') {
        last_slash
    } else if let Some(last_space) = input.rfind(' ') {
        last_space + 1
    } else {
        0
    };

    let end = input.len();
    (input[start..].to_string(), start, end)
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
                            // Safety: REPL event loop is single-threaded — no concurrent reads of SHANNON_API_KEY.
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
    let command_names = repl.runtime.block_on(repl.command_registry.list_names());
    let items: Vec<SelectItem<String>> = command_names.into_iter().map(|name| {
        let display = format!("/{name}");
        SelectItem::new(display.clone(), display)
    }).collect();

    let picker = crate::widgets::select::FuzzyPickerWidget::new("Command Palette".to_string())
        .with_items(items);
    repl.state.fuzzy_picker = Some(picker);
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
            repl.state.fuzzy_picker = None;

            if let Some(cmd) = selected {
                repl.prompt.set_input(cmd);
                super::commands::submit_input(repl)?;
            }
        }
        KeyCode::Esc => {
            repl.state.fuzzy_picker = None;
        }
        _ => {}
    }
    Ok(())
}

fn handle_file_selector_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Up => {
            if let Some(ref mut sel) = repl.state.file_selector {
                sel.move_up();
            }
        }
        KeyCode::Down => {
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
                repl.prompt.set_input(path);
                repl.chat.add_message(
                    ChatRole::System,
                    "File selected — press Enter to send as query, or edit the path.".to_string(),
                );
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

fn handle_multi_select_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Up => {
            if let Some(ref mut sel) = repl.state.multi_select {
                sel.move_up();
            }
        }
        KeyCode::Down => {
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
