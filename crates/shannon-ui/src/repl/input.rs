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

    // If model picker is active, handle model picker input
    if repl.state.model_picker.is_some() {
        return handle_model_picker_input(repl, key);
    }

    // If diff viewer overlay is active, handle viewer keys
    if repl.state.diff_viewer.is_some() {
        return handle_diff_viewer_input(repl, key);
    }

    // If incremental search (Ctrl+R) is active, handle search keys
    if repl.state.incremental_search_active {
        return handle_incremental_search(repl, key);
    }

    // If leader key mode is active, handle the second key
    if repl.state.leader_active {
        return handle_leader_key(repl, key);
    }

    let kb = &repl.state.keybindings;
    match key.code {
        _ if kb.reverse_search.matches(&key) => {
            // Activate incremental reverse search
            repl.state.incremental_search_active = true;
            repl.state.incremental_search_query.clear();
            repl.state.incremental_search_match_index = 0;
            repl.state.incremental_search_saved_input = repl.prompt.input().to_string();
            repl.state.status = "(reverse-i-search) ``: ".to_string();
            Ok(())
        }
        _ if kb.command_palette.matches(&key) => {
            open_command_palette(repl);
            Ok(())
        }
        _ if kb.quit.matches(&key) => {
            repl.running = false;
            Ok(())
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            repl.running = false;
            Ok(())
        }
        KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+V: try clipboard image paste, fall back to text insert
            // For image detection, delegate to /image paste
            super::commands::handle_image_paste_from_input(repl)?;
            Ok(())
        }
        _ if kb.toggle_sidebar.matches(&key) => {
            // Toggle right sidebar panel
            repl.state.sidebar_visible = !repl.state.sidebar_visible;
            Ok(())
        }
        _ if kb.toggle_tool_collapse.matches(&key) => {
            // Toggle tool output collapse
            repl.chat.collapsed_tools = !repl.chat.collapsed_tools;
            Ok(())
        }
        _ if kb.leader.matches(&key) => {
            // Enter leader key mode — wait for second key
            repl.state.leader_active = true;
            repl.state.status = "C-x  (d)iff (s)idebar (m)odel (c)ollapse (b)ottom (t)heme (g)top".to_string();
            Ok(())
        }
        KeyCode::Enter => {
            // If completion suggestions are visible, apply the selected one
            if !repl.state.completion_suggestions.is_empty() {
                let idx = repl.state.completion_suggestion_index;
                if let Some(suggestion) = repl.state.completion_suggestions.get(idx).cloned() {
                    apply_completion(repl, &suggestion);
                }
                clear_completions(repl);
                return Ok(());
            }
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                repl.prompt.insert_newline();
            } else {
                super::commands::submit_input(repl)?;
            }
            Ok(())
        }
        KeyCode::Char(c) => {
            repl.prompt.add_char(c);
            repl.prompt.set_vim_mode("INSERT");
            repl.state.vim_mode = "INSERT".to_string();
            // Detect `@` typed at end of prompt to open file picker
            if c == '@' {
                let input = repl.prompt.input();
                if input.ends_with('@') && (input.len() == 1 || input.as_bytes()[input.len() - 2] == b' ') {
                    let selector = crate::widgets::select::FileSelectorWidget::new(
                        " @ File Reference ".to_string(),
                    );
                    repl.state.file_selector = Some(selector);
                    repl.state.file_selector_for_at = true;
                }
            }
            update_inline_completions(repl);
            Ok(())
        }
        KeyCode::Backspace => {
            repl.prompt.backspace();
            // Dismiss completion popup on text change; will be recomputed on next Tab
            repl.state.completion_suggestions.clear();
            repl.state.completion_suggestion_index = 0;
            repl.tab_completion_state.candidates.clear();
            repl.tab_completion_state.last_prefix.clear();
            Ok(())
        }
        KeyCode::Up => {
            // If completion suggestions are visible, navigate up
            if !repl.state.completion_suggestions.is_empty() {
                if repl.state.completion_suggestion_index > 0 {
                    repl.state.completion_suggestion_index -= 1;
                } else {
                    repl.state.completion_suggestion_index =
                        repl.state.completion_suggestions.len().saturating_sub(1);
                }
                return Ok(());
            }
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
            // If completion suggestions are visible, navigate down
            if !repl.state.completion_suggestions.is_empty() {
                if repl.state.completion_suggestion_index < repl.state.completion_suggestions.len().saturating_sub(1) {
                    repl.state.completion_suggestion_index += 1;
                } else {
                    repl.state.completion_suggestion_index = 0;
                }
                return Ok(());
            }
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
            // If completion suggestions are visible, dismiss them
            if !repl.state.completion_suggestions.is_empty() {
                clear_completions(repl);
                return Ok(());
            }
            repl.prompt.set_vim_mode("NORMAL");
            repl.state.vim_mode = "NORMAL".to_string();
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

/// Handle keys during incremental reverse search (Ctrl+R)
fn handle_incremental_search(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    match key.code {
        // Ctrl+R again: cycle to next older match
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let matches = repl.command_history.search_history(&repl.state.incremental_search_query);
            if !matches.is_empty() {
                let idx = repl.state.incremental_search_match_index;
                let next_idx = if idx + 1 < matches.len() { idx + 1 } else { 0 };
                repl.state.incremental_search_match_index = next_idx;
                let matched = matches[next_idx].to_string();
                repl.prompt.set_input(matched.clone());
                repl.state.status = format!(
                    "(reverse-i-search) `{}`: {}",
                    repl.state.incremental_search_query, matched
                );
            }
        }
        // Enter: accept current match, exit search
        KeyCode::Enter => {
            repl.state.incremental_search_active = false;
            repl.state.status = "Ready".to_string();
        }
        // Escape: cancel search, restore saved input
        KeyCode::Esc => {
            repl.prompt.set_input(repl.state.incremental_search_saved_input.clone());
            repl.state.incremental_search_active = false;
            repl.state.status = "Ready".to_string();
        }
        // Backspace: remove last char from search query
        KeyCode::Backspace => {
            repl.state.incremental_search_query.pop();
            repl.state.incremental_search_match_index = 0;
            let matches = repl.command_history.search_history(&repl.state.incremental_search_query);
            if let Some(m) = matches.first() {
                repl.prompt.set_input(m.to_string());
                repl.state.status = format!(
                    "(reverse-i-search) `{}`: {}",
                    repl.state.incremental_search_query, m
                );
            } else {
                repl.state.status = format!(
                    "(reverse-i-search) `{}`: <no match>",
                    repl.state.incremental_search_query
                );
            }
        }
        // Regular char: append to search query and find match
        KeyCode::Char(c) => {
            repl.state.incremental_search_query.push(c);
            repl.state.incremental_search_match_index = 0;
            let matches = repl.command_history.search_history(&repl.state.incremental_search_query);
            if let Some(m) = matches.first() {
                repl.prompt.set_input(m.to_string());
                repl.state.status = format!(
                    "(reverse-i-search) `{}`: {}",
                    repl.state.incremental_search_query, m
                );
            } else {
                repl.state.status = format!(
                    "(reverse-i-search) `{}`: <no match>",
                    repl.state.incremental_search_query
                );
            }
        }
        _ => {}
    }
    Ok(())
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

/// Clear all completion suggestions and reset state.
fn clear_completions(repl: &mut Repl) {
    repl.state.completion_suggestions.clear();
    repl.state.completion_suggestion_index = 0;
    repl.tab_completion_state.candidates.clear();
    repl.tab_completion_state.current_index = 0;
    repl.tab_completion_state.last_prefix.clear();
}

/// Apply a completion suggestion to the current input, replacing the
/// word under the cursor with the selected suggestion.
fn apply_completion(repl: &mut Repl, suggestion: &str) {
    let input = repl.prompt.input().to_string();
    let (_prefix, word_start, word_end) = extract_completion_word(&input, repl);

    let mut new_input = String::new();
    if word_start > 0 && word_start <= input.len() {
        new_input.push_str(&input[..word_start]);
    }
    new_input.push_str(suggestion);
    if word_end < input.len() {
        new_input.push_str(&input[word_end..]);
    }
    repl.prompt.set_input(new_input);
}

/// Compute completion candidates for the current input and populate the
/// visual suggestion popup. Called on every keystroke so suggestions
/// appear inline without requiring Tab.
fn update_inline_completions(repl: &mut Repl) {
    let input = repl.prompt.input().to_string();
    let (prefix, _word_start, _word_end) = extract_completion_word(&input, repl);

    // Only recompute when the prefix actually changed
    if repl.tab_completion_state.last_prefix == prefix && !repl.tab_completion_state.candidates.is_empty() {
        return;
    }

    let mut command_names = repl.runtime.block_on(async {
        repl.shared_executor.registry().await.list_names().await
    });
    for cmd in repl.plugin_manager.get_plugin_commands() {
        if !command_names.iter().any(|n| n == &cmd.name) {
            command_names.push(cmd.name.clone());
        }
    }

    let candidates = compute_candidates(&input, &prefix, &command_names);

    repl.tab_completion_state.last_prefix = prefix;
    repl.tab_completion_state.candidates = candidates.clone();
    repl.tab_completion_state.current_index = 0;
    repl.state.completion_suggestions = candidates;
    repl.state.completion_suggestion_index = 0;
}

/// Pure function: compute completion candidates for the given input.
fn compute_candidates(input: &str, prefix: &str, available_commands: &[String]) -> Vec<String> {
    let has_space = input.trim_end_matches(' ').contains(' ');

    if !has_space && prefix.starts_with('/') {
        available_commands
            .iter()
            .filter(|cmd| {
                let with_slash = format!("/{cmd}");
                with_slash.starts_with(prefix)
            })
            .map(|cmd| format!("/{cmd}"))
            .collect()
    } else if !has_space && prefix.is_empty() {
        available_commands.iter().map(|c| format!("/{c}")).collect()
    } else if has_space && looks_like_path(prefix) {
        complete_file_path(prefix)
    } else if has_space {
        let cmd_name = input.split_whitespace().next().unwrap_or("").trim_start_matches('/');
        complete_command_args(cmd_name, prefix)
    } else {
        Vec::new()
    }
}

/// Handle tab completion — cycles through existing inline suggestions.
fn handle_tab_completion(repl: &mut Repl) -> Result<()> {
    let input = repl.prompt.input().to_string();

    // Ensure candidates are fresh
    update_inline_completions(repl);

    let candidates = &repl.tab_completion_state.candidates;
    if candidates.is_empty() {
        return Ok(());
    }

    let idx = repl.tab_completion_state.current_index;
    let completion = &candidates[idx];
    let (_prefix, word_start, word_end) = extract_completion_word(&input, repl);

    let mut new_input = String::new();
    if word_start > 0 && word_start <= input.len() {
        new_input.push_str(&input[..word_start]);
    }
    new_input.push_str(completion);
    if word_end < input.len() {
        new_input.push_str(&input[word_end..]);
    }
    repl.prompt.set_input(new_input);

    // Advance index for next Tab press
    repl.tab_completion_state.current_index = (idx + 1) % candidates.len();
    // Highlight the suggestion just selected (not the next one)
    repl.state.completion_suggestion_index = idx;

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
    // Model names come from the static catalog (dynamic)
    if cmd_name == "model" || cmd_name == "models" {
        let mut ids: Vec<String> = shannon_core::model_registry::MODEL_CATALOG
            .iter()
            .map(|m| m.id.to_string())
            .collect();
        // Append local Ollama models
        for m in shannon_core::model_registry::detect_local_models() {
            if !ids.contains(&m.id.to_string()) {
                ids.push(m.id.to_string());
            }
        }
        let p = prefix.to_lowercase();
        return ids.into_iter().filter(|id| id.to_lowercase().starts_with(&p)).collect();
    }

    let candidates: &[&str] = match cmd_name {
        "team" => &["create", "add", "task", "assign", "status", "list", "run", "shutdown", "help"],
        "doctor" | "check" | "diagnostics" => &[],
        "compact" => &["status", "truncate", "micro", "group"],
        "cost" => &[],
        "permissions" | "perms" | "perm" => &["status", "allow", "deny", "reset", "help"],
        "plan" => &["status", "approve", "reject", "done", "help"],
        "config" => &["list", "get", "set", "reset", "help"],
        "credentials" | "creds" | "cred" => &["list", "store", "get", "delete", "count", "help"],
        "worktree" => &["enter", "exit", "status"],
        "debug" | "dbg" | "dev" => &["info", "log", "profile", "trace", "help"],
        "web-search" | "websearch" | "search-web" => &[],
        "review" => &["HEAD~1", "main...HEAD"],
        "local-models" | "local" => &[],
        "diff" => &["view", "--staged", "--stat", "--overview", "--word-diff", "-w", "HEAD~1", "main...HEAD"],
        "ci" | "gh-actions" => &["status", "runs", "workflows", "view", "trigger", "help"],
        "history" => &["--export"],
        "export" | "save" => &["--format json", "--format markdown"],
        _ => &[],
    };

    candidates
        .iter()
        .filter(|c| c.starts_with(prefix))
        .map(|c| (*c).to_string())
        .collect()
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
            let word_start = space_pos + 1;
            let word = &trimmed[word_start..];
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
                            crate::repl::preferences::save_preferences(
                                &crate::repl::preferences::Preferences {
                                    model: repl.state.model.clone(),
                                    provider: repl.state.selected_provider.clone(),
                                    theme: Some(repl.state.theme.name.to_string()),
                                },
                            );
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
    let mut items: Vec<SelectItem<String>> = Vec::new();

    // Slash commands
    let command_names = repl.runtime.block_on(repl.command_registry.list_names());
    for name in &command_names {
        let display = format!("/{name}");
        items.push(SelectItem::new(format!("  Command: {display}"), display.clone()));
    }

    // Theme entries
    for theme_name in &["dark", "light", "dracula"] {
        let cmd = format!("/theme {theme_name}");
        let indicator = if repl.state.theme.name == *theme_name { " (active)" } else { "" };
        items.push(SelectItem::new(format!("  Theme: {theme_name}{indicator}"), cmd));
    }

    // Current model shortcut
    if let Some(ref model) = repl.state.model {
        items.push(SelectItem::new(
            format!("  Model: {model} (current)"),
            format!("/model {model}"),
        ));
    }

    // Model picker shortcut
    items.push(SelectItem::new("  Model: Browse all...".to_string(), "/model".to_string()));

    let picker = crate::widgets::select::FuzzyPickerWidget::new("Command Palette  (Ctrl+P)".to_string())
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
    // In @ mode, alphanumeric characters filter the file list
    if repl.state.file_selector_for_at {
        match key.code {
            KeyCode::Char(c) if c.is_alphanumeric() || c == '_' || c == '.' || c == '/' => {
                if let Some(ref mut sel) = repl.state.file_selector {
                    let current_filter = sel.get_filter().unwrap_or("").to_string();
                    let new_filter = format!("{current_filter}{c}");
                    sel.set_filter_pattern(&new_filter);
                }
                return Ok(());
            }
            KeyCode::Backspace => {
                if let Some(ref mut sel) = repl.state.file_selector {
                    let current_filter = sel.get_filter().unwrap_or("").to_string();
                    if !current_filter.is_empty() {
                        sel.set_filter_pattern(&current_filter[..current_filter.len() - 1]);
                    }
                }
                return Ok(());
            }
            _ => {} // fall through to normal handling
        }
    }

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
            let was_at_mode = repl.state.file_selector_for_at;
            repl.state.file_selector = None;
            repl.state.file_selector_for_at = false;

            if let Some(path) = selected_path {
                if was_at_mode {
                    // Check if the selected file is an image — auto-attach instead of inserting path
                    let is_image = path.rsplit('.').next()
                        .map(|ext| matches!(ext.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg"))
                        .unwrap_or(false);

                    if is_image {
                        // Remove the trailing '@' from prompt
                        let input = repl.prompt.input().to_string();
                        if let Some(pos) = input.rfind('@') {
                            let mut new_input = String::with_capacity(input.len());
                            new_input.push_str(&input[..pos]);
                            new_input.push_str(&input[pos + 1..]);
                            repl.prompt.set_input(new_input);
                        }
                        // Attach image automatically
                        let _ = super::commands::handle_image(repl, &path);
                    } else {
                        // Insert path at cursor, replacing the trailing '@'
                        let input = repl.prompt.input().to_string();
                        if let Some(pos) = input.rfind('@') {
                            let mut new_input = String::with_capacity(input.len() + path.len());
                            new_input.push_str(&input[..pos]);
                            new_input.push_str(&path);
                            new_input.push_str(&input[pos + 1..]);
                            repl.prompt.set_input(new_input);
                        }
                    }
                } else {
                    repl.prompt.set_input(path);
                    repl.chat.add_message(
                        ChatRole::System,
                        "File selected — press Enter to send as query, or edit the path.".to_string(),
                    );
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
            // If opened by @, remove the trailing '@' from prompt
            if repl.state.file_selector_for_at {
                let input = repl.prompt.input().to_string();
                if let Some(pos) = input.rfind('@') {
                    let mut new_input = String::with_capacity(input.len());
                    new_input.push_str(&input[..pos]);
                    new_input.push_str(&input[pos + 1..]);
                    repl.prompt.set_input(new_input);
                }
                repl.state.file_selector_for_at = false;
            }
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

/// Handle keyboard input when the diff viewer overlay is active.
fn handle_diff_viewer_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    // Compute max entries count from diff_data
    let max_entries = {
        let diff = &repl.diff_data;
        let mut count = 0usize;
        let mut seen = std::collections::HashSet::new();
        for turn in diff.get_session_diffs() {
            for fc in &turn.files_modified {
                if seen.insert(fc.path.clone()) {
                    count += 1;
                }
            }
            count += turn.files_created.len() + turn.files_deleted.len();
        }
        count
    };

    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(ref mut viewer) = repl.state.diff_viewer {
                viewer.move_down(max_entries);
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(ref mut viewer) = repl.state.diff_viewer {
                viewer.move_up();
            }
        }
        KeyCode::Enter => {
            if let Some(ref mut viewer) = repl.state.diff_viewer {
                viewer.toggle_expand(&repl.diff_data);
            }
        }
        KeyCode::Esc | KeyCode::Char('q') => {
            repl.state.diff_viewer = None;
        }
        _ => {}
    }
    Ok(())
}

/// Handle the second key after leader (Ctrl+X) was pressed.
fn handle_leader_key(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    repl.state.leader_active = false;
    repl.state.status = "Ready".to_string();

    match key.code {
        KeyCode::Char('d') => {
            // Open diff viewer
            let file_count = {
                let diff = &repl.diff_data;
                let mut count = 0;
                for turn in diff.get_session_diffs() {
                    count += turn.files_modified.len() + turn.files_created.len() + turn.files_deleted.len();
                }
                count
            };
            let mut viewer = crate::widgets::diff_viewer::DiffViewerWidget::new();
            viewer.sync_expanded(file_count);
            repl.state.diff_viewer = Some(viewer);
        }
        KeyCode::Char('s') => {
            repl.state.sidebar_visible = !repl.state.sidebar_visible;
        }
        KeyCode::Char('m') => {
            open_command_palette(repl);
        }
        KeyCode::Char('c') => {
            repl.chat.collapsed_tools = !repl.chat.collapsed_tools;
        }
        KeyCode::Char('b') => {
            // Scroll chat to bottom (latest)
            repl.chat.scroll_to_latest();
        }
        KeyCode::Char('t') => {
            // Cycle theme: dark → light → dracula → dark
            let current = repl.state.theme.name.as_str();
            let next = match current {
                "dark" => crate::theme::Theme::default_light(),
                "light" => crate::theme::Theme::dracula(),
                _ => crate::theme::Theme::default_dark(),
            };
            repl.state.theme = next;
            repl.state.toast = Some((format!("Theme: {}", repl.state.theme.name), std::time::Instant::now()));
        }
        KeyCode::Char('g') => {
            // Scroll chat to top (oldest)
            repl.chat.scroll_to_top();
        }
        KeyCode::Esc => {
            // Cancelled — already reset above
        }
        _ => {
            repl.state.toast = Some(("Unknown leader key".to_string(), std::time::Instant::now()));
        }
    }
    Ok(())
}

/// Handle keyboard input when the model picker is active.
fn handle_model_picker_input(repl: &mut Repl, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(ref mut picker) = repl.state.model_picker {
                picker.move_up();
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(ref mut picker) = repl.state.model_picker {
                picker.move_down();
            }
        }
        KeyCode::Left => {
            if let Some(ref mut picker) = repl.state.model_picker {
                picker.prev_provider();
            }
        }
        KeyCode::Right => {
            if let Some(ref mut picker) = repl.state.model_picker {
                picker.next_provider();
            }
        }
        KeyCode::Enter => {
            let selected = repl.state.model_picker
                .as_ref()
                .and_then(|p| p.selected_model())
                .map(|m| (m.id.to_string(), m.provider.clone()));
            repl.state.model_picker = None;

            if let Some((model_id, provider)) = selected {
                repl.state.model = Some(model_id.clone());
                repl.state.selected_provider = Some(provider.clone());
                crate::repl::preferences::save_preferences(
                    &crate::repl::preferences::Preferences {
                        model: Some(model_id.clone()),
                        provider: Some(provider),
                        theme: Some(repl.state.theme.name.to_string()),
                    },
                );
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Model set to: {model_id}"),
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
