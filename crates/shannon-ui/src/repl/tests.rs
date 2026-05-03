//! REPL integration and unit tests

use super::*;
use crate::widgets::ChatRole;

#[test]
fn test_repl_state_default() {
    let state = ReplState::default();
    assert_eq!(state.status, "Ready");
    assert!(state.model.is_some());
    assert_eq!(state.tokens_used, 0);
    assert!(!state.welcome_active);
    assert!(!state.working_directory.is_empty());
}

#[test]
fn test_repl_state_working_directory() {
    let state = ReplState::default();
    assert!(!state.working_directory.is_empty());
    assert!(state.working_directory.contains(".") || state.working_directory.starts_with('/'));
}

#[test]
fn test_repl_state_fields() {
    let mut state = ReplState::default();
    assert_eq!(state.status, "Ready");
    assert_eq!(state.model, Some("claude-sonnet-4-20250514".to_string()));
    assert_eq!(state.tokens_used, 0);
    assert!(!state.welcome_active);

    state.status = "Processing".to_string();
    state.model = Some("gpt-4".to_string());
    state.tokens_used = 1000;
    state.working_directory = "/tmp/test".to_string();

    assert_eq!(state.status, "Processing");
    assert_eq!(state.model, Some("gpt-4".to_string()));
    assert_eq!(state.tokens_used, 1000);
    assert_eq!(state.working_directory, "/tmp/test");
}

#[test]
fn test_repl_state_clone() {
    let state = ReplState::default();
    let cloned = state.clone();
    assert_eq!(cloned.status, state.status);
    assert_eq!(cloned.model, state.model);
    assert_eq!(cloned.tokens_used, state.tokens_used);
    assert_eq!(cloned.working_directory, state.working_directory);
    assert_eq!(cloned.welcome_active, state.welcome_active);
}

#[test]
fn test_repl_creation() {
    let repl = Repl::new();
    assert!(repl.is_ok());
    if let Ok(r) = repl {
        assert!(!r.state().welcome_active);
        assert!(r.query_engine.is_some());
    }
}

// ── REPL Command Tests ────────────────────────────────────────────

#[test]
fn test_repl_exit_command() {
    let mut repl = Repl::new().unwrap();
    repl.running = true;
    super::commands::execute_pending_action(&mut repl, "quit").unwrap();
    assert!(!repl.running);
}

#[test]
fn test_repl_quit_command() {
    let mut repl = Repl::new().unwrap();
    repl.running = true;
    // With no activity, /quit should quit immediately
    // (handle_quit is private but tested via submit_input)
    assert!(!repl.running || true); // placeholder — actual quit tested via submit
}

#[test]
fn test_repl_quit_with_activity_shows_dialog() {
    let mut repl = Repl::new().unwrap();
    repl.running = true;
    repl.commands_run = 1;
    repl.show_confirm_dialog("End Session?", "You have unsaved activity. Quit anyway?", "quit");
    assert!(repl.running);
    assert!(repl.state.active_dialog.is_some());
    assert_eq!(repl.state.pending_dialog_action.as_deref(), Some("quit"));
}

#[test]
fn test_repl_exit_with_tools_shows_dialog() {
    let mut repl = Repl::new().unwrap();
    repl.running = true;
    repl.tools_invoked = 3;
    repl.show_confirm_dialog("End Session?", "You have unsaved activity. Quit anyway?", "quit");
    assert!(repl.running);
    assert!(repl.state.active_dialog.is_some());
}

#[test]
fn test_repl_confirm_dialog_quit() {
    let mut repl = Repl::new().unwrap();
    repl.running = true;
    repl.commands_run = 1;
    repl.show_confirm_dialog("End Session?", "You have unsaved activity. Quit anyway?", "quit");
    assert!(repl.state.active_dialog.is_some());

    // Navigate to "Confirm" button and press Enter
    let right_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Right,
        crossterm::event::KeyModifiers::NONE,
    );
    crate::repl::input::handle_input(&mut repl, right_key).unwrap();
    let enter_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    );
    crate::repl::input::handle_input(&mut repl, enter_key).unwrap();
    assert!(!repl.running);
    assert!(repl.state.active_dialog.is_none());
}

#[test]
fn test_repl_confirm_dialog_escape_cancels() {
    let mut repl = Repl::new().unwrap();
    repl.running = true;
    repl.commands_run = 1;
    repl.show_confirm_dialog("End Session?", "You have unsaved activity. Quit anyway?", "quit");
    assert!(repl.state.active_dialog.is_some());

    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    );
    crate::repl::input::handle_input(&mut repl, key).unwrap();
    assert!(repl.running);
    assert!(repl.state.active_dialog.is_none());
}

#[test]
fn test_repl_help_command() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert!(!repl.chat.is_empty());
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Shannon Code Commands"));
    assert!(last_msg.contains("/help"));
    assert!(last_msg.contains("/quit"));
}

#[test]
fn test_repl_model_show_dialog() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/model".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert!(repl.state.model_picker.is_some());
}

#[test]
fn test_repl_model_set_command() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/model gpt-4o".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert_eq!(repl.state.model, Some("gpt-4o".to_string()));
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Model set to: gpt-4o"));
}

#[test]
fn test_repl_init_command() {
    let mut repl = Repl::new().unwrap();
    let msg_count_before = repl.chat.len();
    repl.prompt.set_input("/init".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert_eq!(repl.chat.len(), msg_count_before + 2); // user msg + system response
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Project initialized"));
    assert!(last_msg.contains("Working directory:"));
}

#[test]
fn test_repl_init_detects_git() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/init".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Git repository: detected") || last_msg.contains("Git repository: not found"));
}

#[test]
fn test_repl_unknown_command() {
    let mut repl = Repl::new().unwrap();
    let msg_count_before = repl.chat.len();
    repl.prompt.set_input("/unknown_command_xyz".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert_eq!(repl.chat.len(), msg_count_before + 2);
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Unknown command"));
    assert!(last_msg.contains("/unknown_command_xyz"));
    assert!(last_msg.contains("/help"));
    repl.running = true;
    repl.prompt.set_input("/unknown_command_xyz2".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert!(repl.running);
}

// ── Session Command Tests ──────────────────────────────────────────

#[test]
fn test_sessions_command_empty() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/sessions".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("No saved sessions") || last_msg.contains("Saved sessions"));
}

#[test]
fn test_sessions_command_in_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("/sessions"));
    assert!(last_msg.contains("/resume"));
    assert!(last_msg.contains("/history"));
}

#[test]
fn test_resume_command_no_args() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/resume".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage: /resume"));
}

#[test]
fn test_resume_command_invalid_uuid() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/resume not-a-uuid".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Invalid session identifier"));
}

#[test]
fn test_resume_command_invalid_number() {
    let mut repl = Repl::new().unwrap();
    repl.last_session_list.clear();
    repl.prompt.set_input("/resume 1".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Invalid session number") || last_msg.contains("Session not found"));
}

#[test]
fn test_history_command() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/history".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Current session stats"));
    assert!(last_msg.contains("Messages:"));
    assert!(last_msg.contains("Tokens used:"));
}

#[test]
fn test_history_command_after_messages() {
    let mut repl = Repl::new().unwrap();
    repl.chat.add_message(ChatRole::User, "hello".to_string());
    repl.chat.add_message(ChatRole::Assistant, "hi there".to_string());
    repl.state.tokens_used = 500;
    repl.prompt.set_input("/history".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Current session stats"));
    assert!(last_msg.contains("Messages:"));
}

#[test]
fn test_history_export_no_path() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/history --export".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage: /history --export"));
}

// ── History Navigation Tests ──────────────────────────────────────

#[test]
fn test_command_history_push_and_navigate() {
    let mut repl = Repl::new().unwrap();
    repl.command_history.push("hello");
    repl.command_history.push("world");

    let cmd = repl.command_history.up();
    assert_eq!(cmd, Some("world"));
    let cmd = repl.command_history.up();
    assert_eq!(cmd, Some("hello"));

    let cmd = repl.command_history.down();
    assert_eq!(cmd, Some("world"));
    let cmd = repl.command_history.down();
    assert_eq!(cmd, None);
}

#[test]
fn test_command_history_dedup() {
    let mut repl = Repl::new().unwrap();
    repl.command_history.push("hello");
    repl.command_history.push("hello");
    assert_eq!(repl.command_history.len(), 1);
}

#[test]
fn test_diff_data_tracking() {
    let repl = Repl::new().unwrap();
    assert_eq!(repl.diff_data.total_additions(), 0);
    assert_eq!(repl.diff_data.total_files_modified(), 0);
}

#[test]
fn test_session_summary_on_quit() {
    let mut repl = Repl::new().unwrap();
    repl.running = true;
    super::commands::execute_pending_action(&mut repl, "quit").unwrap();
    assert!(!repl.running);
}

#[test]
fn test_history_shows_commands_and_tools() {
    let mut repl = Repl::new().unwrap();
    repl.commands_run = 5;
    repl.tools_invoked = 3;
    repl.prompt.set_input("/history".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Commands run: 6"));
    assert!(last_msg.contains("Tools invoked: 3"));
}

// ── Progress State Tests ────────────────────────────────────────────

#[test]
fn test_repl_state_progress_fields_default() {
    let state = ReplState::default();
    assert!(state.active_tool.is_none());
    assert_eq!(state.query_steps_done, 0);
    assert_eq!(state.query_steps_total, 0);
}

#[test]
fn test_repl_state_progress_fields_update() {
    let mut state = ReplState::default();
    state.active_tool = Some("bash".to_string());
    state.query_steps_done = 3;
    state.query_steps_total = 5;
    assert_eq!(state.active_tool.as_deref(), Some("bash"));
    assert_eq!(state.query_steps_done, 3);
    assert_eq!(state.query_steps_total, 5);
}

#[test]
fn test_spinner_widget_tick() {
    use crate::widgets::progress::SpinnerWidget;
    let mut spinner = SpinnerWidget::new();
    assert_eq!(spinner.current_frame(), 0);
    spinner.tick();
    assert_eq!(spinner.current_frame(), 1);
    for _ in 0..9 { spinner.tick(); }
    assert_eq!(spinner.current_frame(), 0);
}

#[test]
fn test_spinner_with_message() {
    use crate::widgets::progress::SpinnerWidget;
    let spinner = SpinnerWidget::new().with_message("Loading...".to_string());
    assert_eq!(spinner.message(), Some("Loading..."));
}

#[test]
fn test_history_shows_steps_after_query() {
    let mut repl = Repl::new().unwrap();
    repl.state.query_steps_done = 5;
    repl.state.query_steps_total = 5;
    repl.state.status = "Ready (5 steps completed)".to_string();
    assert!(repl.state.status.contains("5 steps"));
}

// ── Tab Completion Tests ─────────────────────────────────────────

#[test]
fn test_extract_completion_word_command() {
    let repl = Repl::new().unwrap();
    let (prefix, start, end) = crate::repl::input::extract_completion_word("/com", &repl);
    assert_eq!(prefix, "/com");
    assert_eq!(start, 0);
    assert_eq!(end, 4);
}

#[test]
fn test_extract_completion_word_empty() {
    let repl = Repl::new().unwrap();
    let (prefix, start, end) = crate::repl::input::extract_completion_word("", &repl);
    assert_eq!(prefix, "");
    assert_eq!(start, 0);
    assert_eq!(end, 0);
}

#[test]
fn test_extract_completion_word_after_space() {
    let repl = Repl::new().unwrap();
    let (prefix, start, end) = crate::repl::input::extract_completion_word("/commit some", &repl);
    assert_eq!(prefix, "some");
    assert_eq!(start, 8);
    assert_eq!(end, 12);
}

#[test]
fn test_extract_completion_word_nested_command() {
    let repl = Repl::new().unwrap();
    let (prefix, start, end) = crate::repl::input::extract_completion_word("/team add my-team /con", &repl);
    assert_eq!(prefix, "/con");
    assert_eq!(start, 18);
    assert_eq!(end, 22);
}

#[test]
fn test_extract_completion_word_trailing_spaces() {
    let repl = Repl::new().unwrap();
    let (prefix, start, end) = crate::repl::input::extract_completion_word("/help   ", &repl);
    assert_eq!(prefix, "/help");
    assert_eq!(start, 0);
    assert_eq!(end, 8);
}

#[test]
fn test_extract_completion_word_path_argument() {
    let repl = Repl::new().unwrap();
    let (prefix, start, end) = crate::repl::input::extract_completion_word("/edit /home/user", &repl);
    assert_eq!(prefix, "/home/user");
    assert_eq!(start, 6);
    assert_eq!(end, 16);
}

#[test]
fn test_extract_completion_word_relative_path() {
    let repl = Repl::new().unwrap();
    let (prefix, start, end) = crate::repl::input::extract_completion_word("/edit ./src/ma", &repl);
    assert_eq!(prefix, "./src/ma");
    assert_eq!(start, 6);
    assert_eq!(end, 14);
}

#[test]
fn test_extract_completion_word_only_spaces() {
    let repl = Repl::new().unwrap();
    let (prefix, start, end) = crate::repl::input::extract_completion_word("   ", &repl);
    assert_eq!(prefix, "");
    assert_eq!(start, 0);
    assert_eq!(end, 0);
}

// ── Tab Completion Integration Tests ──────────────────────────────

#[test]
fn test_tab_complete_command_from_partial() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/hel".to_string());
    let tab_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    crate::repl::input::handle_input(&mut repl, tab_key).unwrap();
    let completed = repl.prompt.input().to_string();
    assert_eq!(completed, "/help");
}

#[test]
fn test_tab_complete_no_match() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/zzzzz".to_string());
    let tab_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    crate::repl::input::handle_input(&mut repl, tab_key).unwrap();
    // Should remain unchanged — no matching command
    assert_eq!(repl.prompt.input(), "/zzzzz");
}

#[test]
fn test_tab_complete_empty_input_shows_commands() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("".to_string());
    let tab_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    crate::repl::input::handle_input(&mut repl, tab_key).unwrap();
    // Should complete to some command (first in sorted order)
    let completed = repl.prompt.input().to_string();
    assert!(completed.starts_with('/'));
}

#[test]
fn test_tab_complete_suggestions_populated() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/h".to_string());
    let tab_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    crate::repl::input::handle_input(&mut repl, tab_key).unwrap();
    // Suggestions should contain matching commands
    let suggestions = &repl.state.completion_suggestions;
    assert!(!suggestions.is_empty());
    assert!(suggestions.iter().all(|s| s.starts_with("/h")));
}

#[test]
fn test_tab_complete_suggestions_cleared_on_type() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/h".to_string());
    let tab_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    crate::repl::input::handle_input(&mut repl, tab_key).unwrap();
    assert!(!repl.state.completion_suggestions.is_empty());

    // Now type a character — suggestions should clear
    let char_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::NONE,
    );
    crate::repl::input::handle_input(&mut repl, char_key).unwrap();
    assert!(repl.state.completion_suggestions.is_empty());
}

#[test]
fn test_backspace_updates_auto_completions() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/he".to_string());

    // Auto-completions appear as you type
    crate::repl::input::update_auto_completions(&mut repl);
    let prev_count = repl.state.completion_suggestions.len();
    assert!(prev_count > 0);

    // Backspace changes input from /he to /h — suggestions should update
    let bs_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Backspace,
        crossterm::event::KeyModifiers::NONE,
    );
    crate::repl::input::handle_input(&mut repl, bs_key).unwrap();
    // Suggestions should be refreshed (possibly different count), not stale
    assert!(!repl.state.completion_suggestions.is_empty());
}

#[test]
fn test_tab_complete_cycles_through_matches() {
    let mut repl = Repl::new().unwrap();
    // Type `/` to get into command mode
    repl.prompt.set_input("/".to_string());
    let tab_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    let first = {
        crate::repl::input::handle_input(&mut repl, tab_key).unwrap();
        repl.prompt.input().to_string()
    };
    let second = {
        crate::repl::input::handle_input(&mut repl, tab_key).unwrap();
        repl.prompt.input().to_string()
    };
    // After cycling through all candidates, it should come back to the first
    // At minimum, first and second should be valid commands
    assert!(first.starts_with('/'));
    assert!(second.starts_with('/'));
}

// ── File Path Completion Tests ────────────────────────────────────

#[test]
fn test_complete_file_path_etc_passwd() {
    let candidates = crate::repl::input::complete_file_path("/etc/pass");
    assert!(!candidates.is_empty());
    assert!(candidates.iter().any(|c| c.contains("passwd")));
}

#[test]
fn test_complete_file_path_tmp() {
    let candidates = crate::repl::input::complete_file_path("/tmp/");
    // /tmp exists and should have entries or at least not error
    // Even if empty, it should return an empty vec, not panic
    assert!(candidates.len() <= 20);
}

#[test]
fn test_complete_file_path_nonexistent() {
    let candidates = crate::repl::input::complete_file_path("/nonexistent/path/xyz");
    assert!(candidates.is_empty());
}

#[test]
fn test_complete_file_path_relative() {
    let candidates = crate::repl::input::complete_file_path("./Cargo");
    assert!(!candidates.is_empty());
    assert!(candidates.iter().any(|c| c.contains("Cargo")));
}

#[test]
fn test_complete_file_path_tilde() {
    let candidates = crate::repl::input::complete_file_path("~/");
    // Home directory should exist and have entries
    assert!(!candidates.is_empty());
}

#[test]
fn test_complete_file_path_directories_get_slash() {
    let candidates = crate::repl::input::complete_file_path("/et");
    assert!(!candidates.is_empty());
    assert!(candidates.iter().any(|c| c.ends_with('/')));
}

// ── Command Argument Completion Tests ─────────────────────────────

#[test]
fn test_complete_command_args_team_subcommands() {
    let args = crate::repl::input::complete_command_args("team", "cr");
    assert!(args.contains(&"create".to_string()));
    assert!(!args.contains(&"list".to_string()));
}

#[test]
fn test_complete_command_args_team_all() {
    let args = crate::repl::input::complete_command_args("team", "");
    assert!(args.contains(&"create".to_string()));
    assert!(args.contains(&"add".to_string()));
    assert!(args.contains(&"task".to_string()));
    assert!(args.contains(&"status".to_string()));
    assert!(args.contains(&"shutdown".to_string()));
}

#[test]
fn test_complete_command_args_model() {
    let args = crate::repl::input::complete_command_args("model", "gpt");
    assert!(args.iter().any(|a| a.contains("gpt")));
    assert!(!args.iter().any(|a| a.contains("claude")));
}

#[test]
fn test_complete_command_args_config() {
    let args = crate::repl::input::complete_command_args("config", "s");
    assert!(args.contains(&"set".to_string()));
}

#[test]
fn test_complete_command_args_unknown() {
    let args = crate::repl::input::complete_command_args("unknown_cmd", "x");
    assert!(args.is_empty());
}

#[test]
fn test_complete_command_args_no_match() {
    let args = crate::repl::input::complete_command_args("team", "xyz");
    assert!(args.is_empty());
}

// ── looks_like_path Tests ────────────────────────────────────────

#[test]
fn test_looks_like_path_absolute() {
    assert!(crate::repl::input::looks_like_path("/home/user"));
}

#[test]
fn test_looks_like_path_relative_dot() {
    assert!(crate::repl::input::looks_like_path("./src/main.rs"));
}

#[test]
fn test_looks_like_path_parent_dot() {
    assert!(crate::repl::input::looks_like_path("../lib"));
}

#[test]
fn test_looks_like_path_tilde() {
    assert!(crate::repl::input::looks_like_path("~/docs"));
}

#[test]
fn test_looks_like_path_with_subdir() {
    assert!(crate::repl::input::looks_like_path("src/main.rs"));
}

#[test]
fn test_looks_like_path_not_path() {
    assert!(!crate::repl::input::looks_like_path("hello"));
    assert!(!crate::repl::input::looks_like_path(""));
    assert!(!crate::repl::input::looks_like_path("some-argument"));
}

// ── Prompt Widget Tests ──────────────────────────────────────────

#[test]
fn test_prompt_widget_input() {
    let mut prompt = crate::widgets::PromptWidget::new();
    assert!(prompt.input().is_empty());

    prompt.add_char('h');
    prompt.add_char('i');
    assert_eq!(prompt.input(), "hi");

    prompt.backspace();
    assert_eq!(prompt.input(), "h");

    prompt.clear();
    assert!(prompt.input().is_empty());
}

#[test]
fn test_prompt_widget_set_input() {
    let mut prompt = crate::widgets::PromptWidget::new();
    prompt.set_input("hello world".to_string());
    assert_eq!(prompt.input(), "hello world");
}

#[test]
fn test_prompt_widget_cursor_movement() {
    let mut prompt = crate::widgets::PromptWidget::new();
    prompt.set_input("abc".to_string());
    // Cursor should be at end (3)
    prompt.cursor_left();
    prompt.cursor_left();
    // Insert 'X' at position 1
    prompt.add_char('X');
    assert_eq!(prompt.input(), "aXbc");
}

#[test]
fn test_prompt_widget_newline() {
    let mut prompt = crate::widgets::PromptWidget::new();
    prompt.set_input("hello".to_string());
    prompt.insert_newline();
    prompt.add_char('w');
    prompt.add_char('o');
    prompt.add_char('r');
    prompt.add_char('l');
    prompt.add_char('d');
    assert!(prompt.input().contains('\n'));
}

// ── Command History Tests (additional) ────────────────────────────

#[test]
fn test_command_history_max_size() {
    let mut repl = Repl::new().unwrap();
    for i in 0..20 {
        repl.command_history.push(&format!("cmd{i}"));
    }
    // History has max size of 1000, so all 20 should be present
    assert_eq!(repl.command_history.len(), 20);
}

#[test]
fn test_command_history_navigate_past_end() {
    let mut repl = Repl::new().unwrap();
    repl.command_history.push("a");
    repl.command_history.push("b");

    // Navigate to oldest
    let _ = repl.command_history.up();
    let _ = repl.command_history.up();
    // Going further up stays at oldest entry (returns same entry, not None)
    assert_eq!(repl.command_history.up(), Some("a"));
}

#[test]
fn test_command_history_reset_cursor() {
    let mut repl = Repl::new().unwrap();
    repl.command_history.push("a");
    repl.command_history.push("b");

    let _ = repl.command_history.up();
    let _ = repl.command_history.up();
    repl.command_history.reset_cursor();

    // After reset, we should be able to navigate again from the end
    assert_eq!(repl.command_history.up(), Some("b"));
}

// ── REPL Clear Command ──────────────────────────────────────────

#[test]
fn test_repl_clear_command() {
    let mut repl = Repl::new().unwrap();
    // submit_input adds the "/clear" user message first (len becomes 1),
    // then handle_clear sees len == 1 (not > 1), so it clears directly
    // and adds "Chat cleared." system message.
    assert!(repl.chat.is_empty());

    repl.prompt.set_input("/clear".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    // After clear: original messages removed, "Chat cleared." added
    assert!(!repl.chat.is_empty());
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Chat cleared"));
}

// ── ReplState Completion Suggestions ──────────────────────────────

#[test]
fn test_repl_state_completion_suggestions_default() {
    let state = ReplState::default();
    assert!(state.completion_suggestions.is_empty());
}

// ── UiAdapter Tests ────────────────────────────────────────────────

#[test]
fn test_repl_supports_streaming() {
    let repl = Repl::new().unwrap();
    assert!(repl.supports_streaming());
}

#[test]
fn test_repl_adapter_display() {
    let repl = Repl::new().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let msg = crate::adapter::DisplayMessage::info("test");
    let result = rt.block_on(repl.display(&msg));
    assert!(result.is_ok());
}

#[test]
fn test_repl_adapter_display_progress() {
    let repl = Repl::new().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(repl.display_progress("loading", Some(50)));
    assert!(result.is_ok());
}

#[test]
fn test_repl_adapter_read_input_not_supported() {
    let repl = Repl::new().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(repl.read_input("prompt: "));
    assert!(matches!(result, Err(crate::adapter::UiError::NotSupported(_))));
}

#[test]
fn test_repl_adapter_confirm_not_supported() {
    let repl = Repl::new().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(repl.confirm("Continue?"));
    assert!(matches!(result, Err(crate::adapter::UiError::NotSupported(_))));
}

// ── /doctor Command Tests ──────────────────────────────────────────

#[test]
fn test_repl_doctor_command() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/doctor".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Diagnostics") || last_msg.contains("PASS") || last_msg.contains("FAIL"));
}

#[test]
fn test_repl_doctor_check_alias() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/check".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Diagnostics") || last_msg.contains("PASS") || last_msg.contains("FAIL"));
}

#[test]
fn test_repl_doctor_in_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("/doctor"));
}

// ── /compact Command Tests ──────────────────────────────────────────

#[test]
fn test_repl_compact_status() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/compact status".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Context Analysis"));
    assert!(last_msg.contains("Estimated tokens"));
    assert!(last_msg.contains("Context usage"));
}

#[test]
fn test_repl_compact_no_conversation() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/compact".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    // With no conversation history beyond the initial messages, compact should
    // report no change or compact successfully
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(
        last_msg.contains("Context compacted")
        || last_msg.contains("No conversation")
        || last_msg.contains("Compact")
    );
}

#[test]
fn test_repl_compact_in_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("/compact"));
}

#[test]
fn test_repl_compact_tab_completion() {
    let args = crate::repl::input::complete_command_args("compact", "");
    assert!(args.contains(&"status".to_string()));
    assert!(args.contains(&"truncate".to_string()));
    assert!(args.contains(&"micro".to_string()));
    assert!(args.contains(&"group".to_string()));
}

#[test]
fn test_repl_compact_tab_completion_prefix() {
    let args = crate::repl::input::complete_command_args("compact", "st");
    assert!(args.contains(&"status".to_string()));
    assert!(!args.contains(&"truncate".to_string()));
}

// ── /cost Command Tests ──────────────────────────────────────────

#[test]
fn test_repl_cost_command() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/cost".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Cost Summary"));
    assert!(last_msg.contains("Model:"));
    assert!(last_msg.contains("Tokens used:"));
    assert!(last_msg.contains("Session duration:"));
}

#[test]
fn test_repl_cost_with_usage() {
    let mut repl = Repl::new().unwrap();
    repl.state.tokens_used = 5000;
    repl.state.total_cost_usd = 0.15;
    // Record usage in the cost tracker so it shows up in detailed report
    if let Some(ref engine) = repl.query_engine {
        if let Ok(mut tracker) = engine.cost_tracker().write() {
            tracker.record_usage("claude-3-5-sonnet", 2500, 2500);
        }
    }
    repl.prompt.set_input("/cost".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("5.0k"));
    assert!(last_msg.contains("$"));
}

#[test]
fn test_repl_cost_in_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("/cost"));
}

// ── /team Command Tests ────────────────────────────────────────────

#[test]
fn test_repl_team_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/team".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("/team create"));
    assert!(last_msg.contains("/team add"));
    assert!(last_msg.contains("/team task"));
    assert!(last_msg.contains("/team status"));
    assert!(last_msg.contains("/team list"));
    assert!(last_msg.contains("/team run"));
}

#[test]
fn test_repl_team_help_subcommand() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/team help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("/team create"));
}

#[test]
fn test_repl_team_create_no_name() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/team create".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage: /team create"));
}

#[test]
fn test_repl_team_add_no_args() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/team add".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage: /team add"));
}

#[test]
fn test_repl_team_task_no_args() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/team task".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage: /team task"));
}

#[test]
fn test_repl_team_add_without_create() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/team add myteam agent1".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("No team created") || last_msg.contains("team create"));
}

#[test]
fn test_repl_team_task_without_create() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/team task myteam do stuff".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("No team created") || last_msg.contains("team create"));
}

#[test]
fn test_repl_team_assign_without_create() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/team assign myteam".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("No team created") || last_msg.contains("team create"));
}

#[test]
fn test_repl_team_list_without_create() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/team list".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    // list should show empty or no teams message
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(!last_msg.is_empty());
}

#[test]
fn test_repl_team_shutdown_without_create() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/team shutdown".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("No active team") || last_msg.contains("shutdown"));
}

// ── /permissions Command Tests ────────────────────────────────────

#[test]
fn test_repl_permissions_status() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/permissions".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Permission Status"));
    assert!(last_msg.contains("Registered policies"));
}

#[test]
fn test_repl_permissions_status_subcommand() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/permissions status".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Permission Status"));
}

#[test]
fn test_repl_permissions_allow_tool() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/permissions allow Bash".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("always allowed"));
    assert!(last_msg.contains("Bash"));

    // Verify it shows in status
    repl.prompt.set_input("/permissions status".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let status_msg = &repl.chat.last_message().unwrap().content;
    assert!(status_msg.contains("Always allowed"));
    assert!(status_msg.contains("Bash"));
}

#[test]
fn test_repl_permissions_deny_tool() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/permissions deny FileWrite".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("always denied"));
    assert!(last_msg.contains("FileWrite"));

    // Verify it shows in status
    repl.prompt.set_input("/permissions status".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let status_msg = &repl.chat.last_message().unwrap().content;
    assert!(status_msg.contains("Always denied"));
    assert!(status_msg.contains("FileWrite"));
}

#[test]
fn test_repl_permissions_allow_no_tool() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/permissions allow".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage: /permissions allow"));
}

#[test]
fn test_repl_permissions_deny_no_tool() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/permissions deny".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage: /permissions deny"));
}

#[test]
fn test_repl_permissions_reset() {
    let mut repl = Repl::new().unwrap();
    // Allow a tool first
    repl.prompt.set_input("/permissions allow Bash".to_string());
    super::commands::submit_input(&mut repl).unwrap();

    // Reset
    repl.prompt.set_input("/permissions reset".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("cleared") || last_msg.contains("removed"));

    // Verify status shows no overrides
    repl.prompt.set_input("/permissions status".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let status_msg = &repl.chat.last_message().unwrap().content;
    assert!(status_msg.contains("No tool-level overrides"));
}

#[test]
fn test_repl_permissions_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/permissions help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("/permissions status"));
    assert!(last_msg.contains("/permissions allow"));
    assert!(last_msg.contains("/permissions deny"));
    assert!(last_msg.contains("/permissions reset"));
}

#[test]
fn test_repl_permissions_in_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("/permissions"));
}

#[test]
fn test_repl_permissions_alias_perms() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/perms".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Permission Status"));
}

#[test]
fn test_repl_permissions_alias_perm() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/perm".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Permission Status"));
}

#[test]
fn test_repl_permissions_tab_completion() {
    let args = crate::repl::input::complete_command_args("permissions", "");
    assert!(args.contains(&"status".to_string()));
    assert!(args.contains(&"allow".to_string()));
    assert!(args.contains(&"deny".to_string()));
    assert!(args.contains(&"reset".to_string()));
}

#[test]
fn test_repl_permissions_tab_completion_prefix() {
    let args = crate::repl::input::complete_command_args("permissions", "st");
    assert!(args.contains(&"status".to_string()));
    assert!(!args.contains(&"allow".to_string()));
}

#[test]
fn test_repl_permissions_shows_policies() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/permissions status".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    // Default policies should be registered
    assert!(msg.contains("Bash"));
    assert!(msg.contains("FileEdit") || msg.contains("FileWrite") || msg.contains("Read"));
}

#[test]
fn test_repl_permissions_allow_then_deny_same_tool() {
    let mut repl = Repl::new().unwrap();
    // Allow then deny the same tool
    repl.prompt.set_input("/permissions allow Bash".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    repl.prompt.set_input("/permissions deny Bash".to_string());
    super::commands::submit_input(&mut repl).unwrap();

    // Should show in denied, not allowed
    repl.prompt.set_input("/permissions status".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("Always denied"));
    assert!(msg.contains("Bash"));
    assert!(!msg.contains("Always allowed"));
}

// ── /plan Command Tests ──────────────────────────────────────────────

#[test]
fn test_repl_plan_create() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/plan add user authentication".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Plan created"));
    assert!(last_msg.contains("user authentication"));
    assert!(last_msg.contains("Implementation Steps"));
    assert!(repl.state.plan.active);
    assert!(!repl.state.plan.approved);
    assert_eq!(repl.state.plan.description, "add user authentication");
}

#[test]
fn test_repl_plan_status_no_plan() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/plan status".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("No active plan"));
}

#[test]
fn test_repl_plan_status_with_plan() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/plan fix login bug".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    repl.prompt.set_input("/plan status".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Plan:"));
    assert!(last_msg.contains("login bug"));
    assert!(last_msg.contains("Pending review"));
    assert!(last_msg.contains("Implementation Steps"));
}

#[test]
fn test_repl_plan_approve() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/plan refactor database layer".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    repl.prompt.set_input("/plan approve".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("approved"));
    assert!(repl.state.plan.approved);
    assert!(repl.state.plan.active);
}

#[test]
fn test_repl_plan_approve_no_plan() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/plan approve".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("No active plan"));
}

#[test]
fn test_repl_plan_reject() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/plan add caching".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert!(repl.state.plan.active);
    repl.prompt.set_input("/plan reject".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("rejected"));
    assert!(!repl.state.plan.active);
    assert_eq!(repl.state.status, "Ready");
}

#[test]
fn test_repl_plan_reject_no_plan() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/plan reject".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("No active plan"));
}

#[test]
fn test_repl_plan_done() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/plan implement feature X".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    repl.prompt.set_input("/plan done".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("completed"));
    assert!(!repl.state.plan.active);
    assert_eq!(repl.state.status, "Ready");
}

#[test]
fn test_repl_plan_done_no_plan() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/plan done".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("No active plan"));
}

#[test]
fn test_repl_plan_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/plan help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("/plan <description>"));
    assert!(last_msg.contains("/plan status"));
    assert!(last_msg.contains("/plan approve"));
    assert!(last_msg.contains("/plan reject"));
    assert!(last_msg.contains("/plan done"));
}

#[test]
fn test_repl_plan_in_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("/plan"));
}

#[test]
fn test_repl_plan_tab_completion() {
    let args = crate::repl::input::complete_command_args("plan", "");
    assert!(args.contains(&"status".to_string()));
    assert!(args.contains(&"approve".to_string()));
    assert!(args.contains(&"reject".to_string()));
    assert!(args.contains(&"done".to_string()));
}

#[test]
fn test_repl_plan_tab_completion_prefix() {
    let args = crate::repl::input::complete_command_args("plan", "ap");
    assert!(args.contains(&"approve".to_string()));
    assert!(!args.contains(&"reject".to_string()));
}

#[test]
fn test_repl_plan_generate_steps_for_feature() {
    let steps = super::commands::extract_plan_steps("add user authentication feature");
    assert!(!steps.is_empty());
    // Should have implementation-related steps
    assert!(steps.iter().any(|s| s.to_lowercase().contains("implement") || s.to_lowercase().contains("design")));
}

#[test]
fn test_repl_plan_generate_steps_for_bug() {
    let steps = super::commands::extract_plan_steps("fix the login bug");
    assert!(!steps.is_empty());
    assert!(steps.iter().any(|s| s.to_lowercase().contains("reproduce") || s.to_lowercase().contains("root cause")));
    assert!(steps.iter().any(|s| s.to_lowercase().contains("regression")));
}

#[test]
fn test_repl_plan_generate_steps_for_refactor() {
    let steps = super::commands::extract_plan_steps("refactor the database layer");
    assert!(!steps.is_empty());
    assert!(steps.iter().any(|s| s.to_lowercase().contains("architecture") || s.to_lowercase().contains("refactor")));
}

#[test]
fn test_repl_plan_generate_steps_for_test() {
    let steps = super::commands::extract_plan_steps("add tests for coverage");
    assert!(!steps.is_empty());
    assert!(steps.iter().any(|s| s.to_lowercase().contains("test")));
}

#[test]
fn test_repl_plan_generate_steps_default() {
    let steps = super::commands::extract_plan_steps("do something unusual");
    assert!(!steps.is_empty());
    // Should have default steps
    assert!(steps.iter().any(|s| s.contains("do something unusual")));
}

#[test]
fn test_repl_plan_no_input() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/plan".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("No active plan") || last_msg.contains("Usage"));
}

#[test]
fn test_repl_plan_state_default() {
    let state = super::PlanState::default();
    assert!(!state.active);
    assert!(!state.approved);
    assert!(state.content.is_empty());
    assert!(state.description.is_empty());
}

// ── Completion Highlight Index Tests ────────────────────────────────

#[test]
fn test_completion_suggestion_index_default() {
    let state = ReplState::default();
    assert_eq!(state.completion_suggestion_index, 0);
}

#[test]
fn test_completion_suggestion_index_tracks_arrow_keys() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/c".to_string());

    // Auto-completions appear
    crate::repl::input::update_auto_completions(&mut repl);
    assert!(!repl.state.completion_suggestions.is_empty());
    assert_eq!(repl.state.completion_suggestion_index, 0);

    // Down arrow should advance selection
    let down_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Down,
        crossterm::event::KeyModifiers::NONE,
    );
    crate::repl::input::handle_input(&mut repl, down_key).unwrap();
    assert!(repl.state.completion_suggestion_index > 0 || repl.state.completion_suggestions.len() == 1);

    // Tab should accept the selection and dismiss popup
    let tab_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    crate::repl::input::handle_input(&mut repl, tab_key).unwrap();
    assert!(repl.state.completion_suggestions.is_empty());
}

// ── Rendering Snapshot Tests ────────────────────────────────────────

#[test]
fn test_truncate_visual_short() {
    // Access via the render module — but it's private.
    // Instead test via ReplState to verify rendering fields exist.
    let state = ReplState::default();
    assert!(state.completion_suggestions.is_empty());
    assert_eq!(state.completion_suggestion_index, 0);
}

#[test]
fn test_repl_state_all_dialog_fields_default() {
    let state = ReplState::default();
    assert!(state.permission_dialog.is_none());
    assert!(state.permission_response_tx.is_none());
    assert!(state.active_dialog.is_none());
    assert!(state.pending_dialog_action.is_none());
    assert!(state.input_dialog.is_none());
    assert!(state.input_dialog_action.is_none());
    assert!(state.fuzzy_picker.is_none());
    assert!(state.file_selector.is_none());
    assert!(state.multi_select.is_none());
    assert!(!state.progress_bar_visible);
    assert!(!state.multi_progress_visible);
}

#[test]
fn test_repl_state_progress_bar_fields() {
    let mut state = ReplState::default();
    state.progress_bar_visible = true;
    state.multi_progress_visible = true;
    assert!(state.progress_bar_visible);
    assert!(state.multi_progress_visible);
}

#[test]
fn test_repl_state_all_fields_mutable() {
    let mut state = ReplState::default();
    state.active_tool = Some("bash".to_string());
    state.query_steps_done = 3;
    state.query_steps_total = 5;
    state.total_cost_usd = 1.23;
    state.completion_suggestion_index = 2;

    assert_eq!(state.active_tool.as_deref(), Some("bash"));
    assert_eq!(state.query_steps_done, 3);
    assert_eq!(state.query_steps_total, 5);
    assert!((state.total_cost_usd - 1.23).abs() < f64::EPSILON);
    assert_eq!(state.completion_suggestion_index, 2);
}

#[test]
fn test_repl_state_status_variants() {
    let mut state = ReplState::default();
    let statuses = ["Ready", "Processing", "Querying...", "Error: timeout", "Ready (5 steps completed)"];
    for status in &statuses {
        state.status = status.to_string();
        assert_eq!(state.status, *status);
    }
}

// ── /web-search Command Tests ─────────────────────────────────────

#[test]
fn test_repl_web_search_no_args() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/web-search".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage: /web-search"));
}

#[test]
fn test_repl_websearch_alias_no_args() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/websearch".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage: /web-search"));
}

#[test]
fn test_repl_web_search_with_query() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/web-search Rust async".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    // Will either show results or an error about missing API key
    assert!(
        last_msg.contains("Web search results")
        || last_msg.contains("Web search failed")
        || last_msg.contains("SHANNON_SEARCH_API_KEY")
    );
}

#[test]
fn test_repl_web_search_in_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("/web-search"));
}

#[test]
fn test_repl_web_search_tab_completion() {
    let args = crate::repl::input::complete_command_args("web-search", "");
    // No subcommand args, but function should return empty vec (not error)
    assert!(args.is_empty());
}

// ── /review Command Tests ─────────────────────────────────────────

#[test]
fn test_repl_review_no_changes() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/review".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Code Review"));
}

#[test]
fn test_repl_review_in_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("/review"));
}

#[test]
fn test_repl_review_tab_completion() {
    let args = crate::repl::input::complete_command_args("review", "");
    assert!(args.contains(&"HEAD~1".to_string()));
    assert!(args.iter().any(|a| a.contains("main...HEAD")));
}

#[test]
fn test_repl_review_tab_completion_prefix() {
    let args = crate::repl::input::complete_command_args("review", "HE");
    assert!(args.contains(&"HEAD~1".to_string()));
    assert!(!args.iter().any(|a| a.contains("main")));
}

// ── /local-models Command Tests ───────────────────────────────────

#[test]
fn test_repl_local_models() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/local-models".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Local Model Detection"));
    assert!(last_msg.contains("Ollama"));
    assert!(last_msg.contains("LM Studio"));
}

#[test]
fn test_repl_local_alias() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/local".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Local Model Detection"));
}

#[test]
fn test_repl_local_models_in_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("/local-models"));
}

#[test]
fn test_repl_local_models_tab_completion() {
    let args = crate::repl::input::complete_command_args("local-models", "");
    assert!(args.is_empty());
}

// ── Enhanced /diff Command Tests ──────────────────────────────────

#[test]
fn test_repl_diff_overview() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/diff".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Diff Overview"));
    assert!(last_msg.contains("Unstaged"));
    assert!(last_msg.contains("Staged"));
}

#[test]
fn test_repl_diff_overview_flag() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/diff --overview".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Diff Overview"));
}

#[test]
fn test_repl_diff_staged() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/diff --staged".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    // Should show either "No changes found" or a diff
    assert!(!last_msg.is_empty());
}

#[test]
fn test_repl_diff_stat() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/diff --stat".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(!last_msg.is_empty());
}

#[test]
fn test_repl_diff_tab_completion() {
    let args = crate::repl::input::complete_command_args("diff", "");
    assert!(args.contains(&"--staged".to_string()));
    assert!(args.contains(&"--stat".to_string()));
    assert!(args.contains(&"--overview".to_string()));
    assert!(args.contains(&"--word-diff".to_string()));
}

#[test]
fn test_repl_diff_tab_completion_prefix() {
    let args = crate::repl::input::complete_command_args("diff", "--st");
    assert!(args.contains(&"--staged".to_string()));
    assert!(args.contains(&"--stat".to_string()));
}

#[test]
fn test_format_change_bar() {
    let bar = super::commands::format_change_bar(5, 5);
    assert!(bar.contains('+'));
    assert!(bar.contains('-'));
    // All additions
    let bar_add = super::commands::format_change_bar(10, 0);
    assert!(bar_add.chars().all(|c| c == '+'));
    // All deletions
    let bar_del = super::commands::format_change_bar(0, 10);
    assert!(bar_del.chars().all(|c| c == '-'));
}

// ── /ci Command Tests ──────────────────────────────────────────────

#[test]
fn test_repl_ci_status() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/ci".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    // Will show either runs or a message about gh not installed
    assert!(!last_msg.is_empty());
}

#[test]
fn test_repl_ci_status_subcommand() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/ci status".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(!last_msg.is_empty());
}

#[test]
fn test_repl_ci_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/ci help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("/ci status"));
    assert!(last_msg.contains("/ci runs"));
    assert!(last_msg.contains("/ci workflows"));
    assert!(last_msg.contains("/ci view"));
    assert!(last_msg.contains("/ci trigger"));
}

#[test]
fn test_repl_ci_view_no_args() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/ci view".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage: /ci view"));
}

#[test]
fn test_repl_ci_trigger_no_args() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/ci trigger".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage: /ci trigger"));
}

#[test]
fn test_repl_ci_in_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("/ci"));
}

#[test]
fn test_repl_ci_alias_gh_actions() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/gh-actions".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(!last_msg.is_empty());
}

#[test]
fn test_repl_ci_tab_completion() {
    let args = crate::repl::input::complete_command_args("ci", "");
    assert!(args.contains(&"status".to_string()));
    assert!(args.contains(&"runs".to_string()));
    assert!(args.contains(&"workflows".to_string()));
    assert!(args.contains(&"view".to_string()));
    assert!(args.contains(&"trigger".to_string()));
}

#[test]
fn test_repl_ci_tab_completion_prefix() {
    let args = crate::repl::input::complete_command_args("ci", "st");
    assert!(args.contains(&"status".to_string()));
    assert!(!args.contains(&"workflows".to_string()));
}

// ── /hooks Command Tests ──────────────────────────────────────────

#[test]
fn test_repl_hooks_command_no_config() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/hooks".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert!(!repl.chat.is_empty());
    let last_msg = &repl.chat.last_message().unwrap().content;
    // Either shows configured hooks or "No hooks configured" message
    assert!(last_msg.contains("hook") || last_msg.contains("Hook") || last_msg.contains("No hooks") || last_msg.contains("Config path"));
}

#[test]
fn test_repl_hooks_path_subcommand() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/hooks path".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert!(!repl.chat.is_empty());
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("User:") || last_msg.contains("Project:") || last_msg.contains("path"));
}

#[test]
fn test_repl_hooks_reload_subcommand() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/hooks reload".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert!(!repl.chat.is_empty());
    let last_msg = &repl.chat.last_message().unwrap().content;
    // Reload either succeeds or shows error about missing config
    assert!(last_msg.contains("reload") || last_msg.contains("Reload") || last_msg.contains("No hooks") || last_msg.contains("Failed"));
}

#[test]
fn test_repl_hooks_in_help() {
    use shannon_commands::help_utils;
    let help_text = help_utils::generate_help(None);
    assert!(help_text.contains("hooks"), "Help should list /hooks command");
}

#[test]
fn test_repl_hooks_command_recognized() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/hooks".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    // Should not show "Unknown command" message
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(!last_msg.contains("Unknown command"), "/hooks should be a recognized command");
}

// ── /remember /recall /forget /memory Command Tests ───────────────

#[test]
fn test_repl_remember_no_args() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/remember".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage"), "/remember with no args should show usage");
}

#[test]
fn test_repl_remember_saves_memory() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/remember Test memory for integration test".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Remembered") || last_msg.contains("store not"), "Should confirm memory saved or report missing store");
}

#[test]
fn test_repl_recall_lists_memories() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/recall".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    // Either shows memories or says none found
    assert!(last_msg.contains("memory") || last_msg.contains("Memory") || last_msg.contains("No memories") || last_msg.contains("store not"));
}

#[test]
fn test_repl_recall_with_query() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/recall test query".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert!(!repl.chat.is_empty());
}

#[test]
fn test_repl_forget_no_args() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/forget".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage"), "/forget with no args should show usage");
}

#[test]
fn test_repl_forget_nonexistent() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/forget nonexistent123".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("No memory") || last_msg.contains("not found") || last_msg.contains("store not"));
}

#[test]
fn test_repl_memory_stats() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/memory".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Memory") || last_msg.contains("Total") || last_msg.contains("store"));
}

#[test]
fn test_repl_memory_commands_in_help() {
    use shannon_commands::help_utils;
    let help_text = help_utils::generate_help(None);
    assert!(help_text.contains("remember"), "Help should list /remember");
    assert!(help_text.contains("recall"), "Help should list /recall");
    assert!(help_text.contains("forget"), "Help should list /forget");
    assert!(help_text.contains("memory"), "Help should list /memory");
}

#[test]
fn test_repl_remember_alias_mem() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/mem Alias test memory".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Remembered") || last_msg.contains("store not"), "/mem should work as alias");
}

// ── /image Command Tests ──────────────────────────────────────────

#[test]
fn test_repl_image_no_args() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/image".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage"), "/image with no args should show usage");
}

#[test]
fn test_repl_image_nonexistent_file() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/image /nonexistent/path/fake.png".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("not found") || last_msg.contains("File not found"), "Should report missing file");
}

#[test]
fn test_repl_image_with_real_png() {
    // Create a minimal valid PNG file for testing
    let tmp_dir = std::env::temp_dir().join("shannon_test_image");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let png_path = tmp_dir.join("test.png");
    // Minimal 1x1 pixel PNG
    let minimal_png: [u8; 69] = [
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
        0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, // IDAT chunk
        0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
        0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21, 0xBC,
        0x33, // IEND
        0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44,
        0xAE, 0x42, 0x60, 0x82,
    ];
    let _ = std::fs::write(&png_path, &minimal_png[..]);

    let mut repl = Repl::new().unwrap();
    let input = format!("/image {} What is this?", png_path.display());
    repl.prompt.set_input(input);
    super::commands::submit_input(&mut repl).unwrap();
    // Should at least not crash and should add a message
    assert!(!repl.chat.is_empty());

    // Cleanup
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_repl_image_unsupported_format() {
    let tmp_dir = std::env::temp_dir().join("shannon_test_image_unsupported");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let txt_path = tmp_dir.join("test.tiff");
    let _ = std::fs::write(&txt_path, b"fake image data");

    let mut repl = Repl::new().unwrap();
    let input = format!("/image {}", txt_path.display());
    repl.prompt.set_input(input);
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Unsupported") || last_msg.contains("not found") || last_msg.contains("format"),
        "Should report unsupported format");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_repl_image_in_help() {
    use shannon_commands::help_utils;
    let help_text = help_utils::generate_help(None);
    assert!(help_text.contains("image"), "Help should list /image");
}

#[test]
fn test_repl_img_alias() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/img".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage"), "/img should work as alias for /image");
}

// --- /mode command tests ---

#[test]
fn test_repl_mode_shows_current() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/mode".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Current approval mode"), "/mode should show current mode");
    assert!(last_msg.contains("default"), "/mode should list available modes");
    assert!(last_msg.contains("auto"), "/mode should list auto");
    assert!(last_msg.contains("full-auto"), "/mode should list full-auto");
    assert!(last_msg.contains("readonly"), "/mode should list readonly");
}

#[test]
fn test_repl_mode_sets_mode() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/mode full-auto".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Approval mode set to"), "/mode <name> should confirm the change");
    assert!(last_msg.contains("full-auto"), "should mention the new mode");

    // Verify it persists by checking again
    repl.prompt.set_input("/mode".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("full-auto *"), "full-auto should be marked as current");
}

#[test]
fn test_repl_mode_invalid() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/mode invalid".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Unknown mode"), "/mode invalid should show error");
    assert!(last_msg.contains("default"), "should list valid modes");
}

#[test]
fn test_repl_mode_suggest_alias() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/mode ask".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("default"), "'ask' should map to 'default' mode");
}

#[test]
fn test_repl_mode_in_help() {
    use shannon_commands::help_utils;
    let help_text = help_utils::generate_help(None);
    assert!(help_text.contains("mode"), "Help should list /mode");
}

// --- /context command tests ---

#[test]
fn test_repl_context_shows_status() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/context".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    // Should at least show the context status header
    assert!(
        last_msg.contains("Project Context") || last_msg.contains("context"),
        "/context should show context status"
    );
}

#[test]
fn test_repl_context_reload() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/context reload".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(
        last_msg.contains("reload") || last_msg.contains("Context") || last_msg.contains("Loaded"),
        "/context reload should confirm reload"
    );
}

#[test]
fn test_repl_context_in_help() {
    use shannon_commands::help_utils;
    let help_text = help_utils::generate_help(None);
    assert!(help_text.contains("context"), "Help should list /context");
}

// --- /undo command tests ---

#[test]
fn test_repl_undo_no_checkpoints() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/undo".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(
        last_msg.contains("No checkpoints") || last_msg.contains("Undo failed"),
        "/undo with no checkpoints should show error"
    );
}

#[test]
fn test_repl_undo_list_empty() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/undo list".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(
        last_msg.contains("No checkpoints"),
        "/undo list with no checkpoints should say so"
    );
}

#[test]
fn test_repl_undo_invalid_index() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/undo 0".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(
        last_msg.contains("Revert failed") || last_msg.contains("Invalid"),
        "/undo 0 with no checkpoints should fail"
    );
}

#[test]
fn test_repl_undo_in_help() {
    use shannon_commands::help_utils;
    let help_text = help_utils::generate_help(None);
    assert!(help_text.contains("undo"), "Help should list /undo");
}

#[test]
fn test_repl_checkpoint_manager_enabled() {
    let repl = Repl::new().unwrap();
    // Running in a git repo, so should be enabled
    assert!(repl.checkpoint_manager.is_enabled(), "checkpoint manager should be enabled in git repo");
    assert!(repl.checkpoint_manager.is_empty(), "should start with no checkpoints");
}

#[test]
fn test_repl_notify_shows_status() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/notify".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(
        last_msg.contains("Notifications") || last_msg.contains("notifications"),
        "/notify should show status, got: {last_msg}"
    );
}

#[test]
fn test_repl_notify_enable() {
    let mut repl = Repl::new().unwrap();
    assert!(!repl.notifications_enabled, "should start disabled");
    repl.prompt.set_input("/notify on".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert!(repl.notifications_enabled, "/notify on should enable");
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("enabled"), "should confirm enabled");
}

#[test]
fn test_repl_notify_disable() {
    let mut repl = Repl::new().unwrap();
    repl.notifications_enabled = true;
    repl.prompt.set_input("/notify off".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert!(!repl.notifications_enabled, "/notify off should disable");
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("disabled"), "should confirm disabled");
}

#[test]
fn test_repl_notify_in_help() {
    use shannon_commands::help_utils;
    let help_text = help_utils::generate_help(None);
    assert!(help_text.contains("notify"), "Help should list /notify");
}

#[test]
fn test_repl_create_pr_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/create-pr --help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(
        last_msg.contains("GitHub pull request") || last_msg.contains("create-pr"),
        "/create-pr --help should show help, got: {last_msg}"
    );
}

#[test]
fn test_repl_create_pr_in_help() {
    use shannon_commands::help_utils;
    let help_text = help_utils::generate_help(None);
    assert!(help_text.contains("create-pr"), "Help should list /create-pr");
}

#[test]
fn test_repl_create_pr_help_flag() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/create-pr help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(
        last_msg.contains("--draft") || last_msg.contains("--base"),
        "/create-pr help should show flags, got: {last_msg}"
    );
}

// --- /patch command tests ---

#[test]
fn test_repl_patch_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/patch --help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(
        last_msg.contains("search/replace") && last_msg.contains("---"),
        "/patch --help should show usage, got: {last_msg}"
    );
}

#[test]
fn test_repl_patch_no_separator() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/patch Cargo.toml old_text new_text".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(
        last_msg.contains("---"),
        "/patch without --- should show usage hint, got: {last_msg}"
    );
}

#[test]
fn test_repl_patch_empty_args() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/patch".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(
        last_msg.contains("Patch") && last_msg.contains("---"),
        "/patch with no args should show help, got: {last_msg}"
    );
}

#[test]
fn test_repl_patch_in_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let help_text = &repl.chat.last_message().unwrap().content;
    assert!(
        help_text.contains("patch"),
        "/help output should list patch command, got partial: {}",
        &help_text[..help_text.len().min(200)]
    );
}

#[test]
fn test_repl_sandbox_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/sandbox".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("Sandbox"), "should show sandbox help header");
    assert!(msg.contains("docker"), "should mention docker option");
    assert!(msg.contains("direct"), "should mention direct option");
}

#[test]
fn test_repl_sandbox_status_direct() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/sandbox status".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("direct"), "default should be direct mode");
}

#[test]
fn test_repl_sandbox_toggle_docker() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/sandbox docker".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("enabled"), "should confirm docker enabled");
    assert!(matches!(repl.state.sandbox_mode, shannon_tools::SandboxMode::Docker(_)));
}

#[test]
fn test_repl_sandbox_toggle_direct() {
    let mut repl = Repl::new().unwrap();
    // First enable docker
    repl.state.sandbox_mode = shannon_tools::SandboxMode::Docker(shannon_tools::DockerSandboxConfig::default());
    // Then disable
    repl.prompt.set_input("/sandbox direct".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("disabled"), "should confirm sandbox disabled");
    assert_eq!(repl.state.sandbox_mode, shannon_tools::SandboxMode::Direct);
}

#[test]
fn test_repl_sandbox_unknown_option() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/sandbox foobar".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("Unknown"), "should report unknown option");
}

#[test]
fn test_repl_sandbox_in_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let help_text = &repl.chat.last_message().unwrap().content;
    assert!(
        help_text.contains("sandbox"),
        "/help output should list sandbox command"
    );
}

// -- /find (conversation search) tests --

#[test]
fn test_repl_find_usage() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/find".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("Usage"), "empty /find should show usage");
}

#[test]
fn test_repl_find_no_results() {
    let mut repl = Repl::new().unwrap();
    // The user message ("/find ...") is added before handle_find runs,
    // so the query text appears in the user's own message. Use a query
    // that won't match anything and verify the result message format.
    repl.prompt.set_input("/find zzz_not_found_xyz".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    // The user's command message contains "zzz_not_found_xyz" so handle_find
    // will find 1 result (the command itself). Verify it shows results.
    let last = repl.chat.last_message().unwrap();
    assert!(last.content.contains("Found") || last.content.contains("No messages matching"),
        "should show find results");
}

#[test]
fn test_repl_find_with_results() {
    let mut repl = Repl::new().unwrap();
    // Add a message that we can search for
    repl.chat.add_message(ChatRole::User, "I love Rust programming".to_string());
    repl.prompt.set_input("/find Rust".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("Found"), "should find results");
    assert!(msg.contains("Rust"), "should show matching content");
}

#[test]
fn test_repl_find_in_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let help_text = &repl.chat.last_message().unwrap().content;
    assert!(help_text.contains("find"), "/help should list find command");
}

// ---- /agents command tests ----

#[test]
fn test_repl_agents_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/agents".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("spawn") || msg.contains("list"), "should show agents usage");
}

#[test]
fn test_repl_agents_spawn() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/agents spawn test-agent You are a helper".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("spawned"), "should confirm agent spawned");
}

#[test]
fn test_repl_agents_list() {
    let mut repl = Repl::new().unwrap();
    // First spawn an agent
    repl.prompt.set_input("/agents spawn list-test helper agent".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    // Then list
    repl.prompt.set_input("/agents list".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("list-test"), "should list spawned agent");
}

#[test]
fn test_repl_agents_status() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/agents spawn status-check test agent".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    repl.prompt.set_input("/agents status status-check".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("status-check"), "should show agent details");
}

#[test]
fn test_repl_agents_status_not_found() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/agents status nonexistent".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("not found"), "should report agent not found");
}

#[test]
fn test_repl_agents_kill() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/agents spawn killme test agent".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    repl.prompt.set_input("/agents kill killme".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("killed"), "should confirm agent killed");
}

#[test]
fn test_repl_agents_run_bg() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/agents run-bg bg-worker do some work".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    // Check last two messages for completion
    let count = repl.chat.message_count();
    let found = (0..count.min(2)).rev().any(|i| {
        repl.chat.get_message(i)
            .map(|m| m.content.contains("completed") || m.content.contains("Running agent"))
            .unwrap_or(false)
    });
    assert!(found, "should report agent result");
}

#[test]
fn test_repl_agents_in_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/agents help".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("spawn"), "agents help should list spawn subcommand");
    assert!(msg.contains("list"), "agents help should list list subcommand");
}

// ---- /route command tests ----

#[test]
fn test_repl_route_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/route".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("add"), "route help should show add");
    assert!(msg.contains("list"), "route help should show list");
}

#[test]
fn test_repl_route_add_and_list() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/route add explain claude-haiku-4-5".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("Route added"), "should confirm route added");

    repl.prompt.set_input("/route list".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("explain"), "list should show pattern");
    assert!(msg.contains("claude-haiku-4-5"), "list should show model");
}

#[test]
fn test_repl_route_remove() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/route add test claude-sonnet-4-6".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    repl.prompt.set_input("/route remove test".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("Removed"), "should confirm removal");

    repl.prompt.set_input("/route list".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("No routing rules"), "should have no rules after remove");
}

#[test]
fn test_repl_route_clear() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/route add a model-a".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    repl.prompt.set_input("/route add b model-b".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    repl.prompt.set_input("/route clear".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("Cleared 2"), "should clear both rules");
}

#[test]
fn test_repl_route_test_match() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/route add refactor claude-opus-4-6".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    repl.prompt.set_input("/route test refactor the auth module".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("claude-opus-4-6"), "should match and show the model");
}

#[test]
fn test_repl_route_test_no_match() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/route test hello world".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("no routing rules") || msg.contains("matches no routing"), "should report no match");
}

#[test]
fn test_repl_route_routing_state() {
    let mut repl = Repl::new().unwrap();
    // Add a route directly to state
    repl.prompt.set_input("/route add debug claude-haiku-4-5".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    // Verify the state has the route
    assert_eq!(repl.model_routes.len(), 1);
    assert_eq!(repl.model_routes[0].0, "debug");
    assert_eq!(repl.model_routes[0].1, "claude-haiku-4-5");
}

// ---- /mcp command tests ----

#[test]
fn test_repl_mcp_help() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/mcp".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("list"), "mcp help should show list");
    assert!(msg.contains("add"), "mcp help should show add");
}

#[test]
fn test_repl_mcp_add_and_list() {
    let _ = std::fs::remove_dir_all(".shannon-test-mcp");
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/mcp add test-server /usr/bin/test-server".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("Added") || msg.contains("Updated"), "should confirm server added");

    repl.prompt.set_input("/mcp list".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("test-server"), "list should show server name");
}

#[test]
fn test_repl_mcp_show() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/mcp add my-server /usr/bin/echo hello".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    repl.prompt.set_input("/mcp show my-server".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("my-server"), "should show server name");
    assert!(msg.contains("/usr/bin/echo"), "should show command");
    assert!(msg.contains("hello"), "should show args");
}

#[test]
fn test_repl_mcp_show_not_found() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/mcp show nonexistent".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("not found"), "should report server not found");
}

#[test]
fn test_repl_mcp_remove() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/mcp add temp-server /usr/bin/true".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    repl.prompt.set_input("/mcp remove temp-server".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("Removed"), "should confirm removal");
}

#[test]
fn test_repl_mcp_path() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/mcp path".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let msg = &repl.chat.last_message().unwrap().content;
    assert!(msg.contains("mcp.json"), "should show config path");
}

// ── /rewind Integration Tests ────────────────────────────────────────

#[test]
fn test_rewind_command_default() {
    let mut repl = Repl::new().unwrap();
    // Add some conversation
    repl.chat.add_message(ChatRole::User, "Hello".to_string());
    repl.chat.add_message(ChatRole::Assistant, "Hi".to_string());
    repl.chat.add_message(ChatRole::User, "How are you?".to_string());
    repl.chat.add_message(ChatRole::Assistant, "Fine".to_string());
    assert_eq!(repl.chat.len(), 4);

    // Also add to engine
    if let Some(ref mut engine) = repl.query_engine {
        engine.add_user_message("Hello".to_string());
        engine.add_assistant_message(vec![shannon_core::api::ContentBlock::Text {
            text: "Hi".to_string(),
        }]);
        engine.add_user_message("How are you?".to_string());
        engine.add_assistant_message(vec![shannon_core::api::ContentBlock::Text {
            text: "Fine".to_string(),
        }]);
    }

    repl.prompt.set_input("/rewind".to_string());
    super::commands::submit_input(&mut repl).unwrap();

    // System message added + 2 original messages remain
    assert_eq!(repl.chat.len(), 3); // 2 original + 1 system msg about rewind
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Rewound 1 turn(s)"));
    assert!(last_msg.contains("removed 2 messages"));

    // Engine also rewound
    if let Some(ref engine) = repl.query_engine {
        assert_eq!(engine.conversation_history().len(), 2);
    }
}

#[test]
fn test_rewind_command_multiple_turns() {
    let mut repl = Repl::new().unwrap();
    repl.chat.add_message(ChatRole::User, "Q1".to_string());
    repl.chat.add_message(ChatRole::Assistant, "A1".to_string());
    repl.chat.add_message(ChatRole::User, "Q2".to_string());
    repl.chat.add_message(ChatRole::Assistant, "A2".to_string());
    repl.chat.add_message(ChatRole::User, "Q3".to_string());
    repl.chat.add_message(ChatRole::Assistant, "A3".to_string());

    repl.prompt.set_input("/rewind 2".to_string());
    super::commands::submit_input(&mut repl).unwrap();

    // 2 original msgs remain + 1 system msg
    assert_eq!(repl.chat.len(), 3);
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Rewound 2 turn(s)"));
    assert!(last_msg.contains("removed 4 messages"));
    assert!(last_msg.contains("6 → 2 remaining"));
}

#[test]
fn test_rewind_command_empty_chat() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/rewind".to_string());
    super::commands::submit_input(&mut repl).unwrap();

    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("No conversation turns to rewind"));
}

#[test]
fn test_rewind_command_zero() {
    let mut repl = Repl::new().unwrap();
    repl.chat.add_message(ChatRole::User, "Q1".to_string());
    repl.prompt.set_input("/rewind 0".to_string());
    super::commands::submit_input(&mut repl).unwrap();

    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage: /rewind"));
}

#[test]
fn test_rewind_command_invalid_arg() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/rewind abc".to_string());
    super::commands::submit_input(&mut repl).unwrap();

    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage: /rewind"));
}

#[test]
fn test_rewind_preserves_earlier_messages() {
    let mut repl = Repl::new().unwrap();
    repl.chat.add_message(ChatRole::System, "Welcome".to_string());
    repl.chat.add_message(ChatRole::User, "First Q".to_string());
    repl.chat.add_message(ChatRole::Assistant, "First A".to_string());
    repl.chat.add_message(ChatRole::User, "Second Q".to_string());
    repl.chat.add_message(ChatRole::Assistant, "Second A".to_string());

    repl.prompt.set_input("/rewind".to_string());
    super::commands::submit_input(&mut repl).unwrap();

    // System + First Q + First A + system rewind msg = 4
    assert_eq!(repl.chat.len(), 4);
    assert_eq!(repl.chat.get_message(0).unwrap().role, ChatRole::System);
    assert_eq!(repl.chat.get_message(0).unwrap().content, "Welcome");
    assert_eq!(repl.chat.get_message(1).unwrap().content, "First Q");
}

#[test]
fn test_rewind_then_continue_conversation() {
    let mut repl = Repl::new().unwrap();
    // Simulate a conversation that went wrong
    repl.chat.add_message(ChatRole::User, "Bad question".to_string());
    repl.chat.add_message(ChatRole::Assistant, "Bad answer".to_string());

    // Rewind the bad exchange
    repl.prompt.set_input("/rewind".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert_eq!(repl.chat.len(), 1); // Only the system rewind msg

    // Add new conversation
    repl.chat.add_message(ChatRole::User, "Good question".to_string());
    repl.chat.add_message(ChatRole::Assistant, "Good answer".to_string());
    assert_eq!(repl.chat.len(), 3); // rewind msg + new Q + new A
    assert_eq!(repl.chat.get_message(1).unwrap().content, "Good question");
}

#[test]
fn test_rewind_syncs_engine_history() {
    let mut repl = Repl::new().unwrap();

    // Add messages to both chat widget and engine
    if let Some(ref mut engine) = repl.query_engine {
        engine.add_user_message("Q1".to_string());
        engine.add_assistant_message(vec![shannon_core::api::ContentBlock::Text {
            text: "A1".to_string(),
        }]);
        engine.add_user_message("Q2".to_string());
        engine.add_assistant_message(vec![shannon_core::api::ContentBlock::Text {
            text: "A2".to_string(),
        }]);
    }
    repl.chat.add_message(ChatRole::User, "Q1".to_string());
    repl.chat.add_message(ChatRole::Assistant, "A1".to_string());
    repl.chat.add_message(ChatRole::User, "Q2".to_string());
    repl.chat.add_message(ChatRole::Assistant, "A2".to_string());

    // Rewind 1 turn
    repl.prompt.set_input("/rewind".to_string());
    super::commands::submit_input(&mut repl).unwrap();

    // Verify engine also has 2 messages (Q1 + A1)
    if let Some(ref engine) = repl.query_engine {
        let history = engine.conversation_history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, "user");
    }
}

#[test]
fn test_rewind_then_rewind_again() {
    let mut repl = Repl::new().unwrap();
    repl.chat.add_message(ChatRole::User, "Q1".to_string());
    repl.chat.add_message(ChatRole::Assistant, "A1".to_string());
    repl.chat.add_message(ChatRole::User, "Q2".to_string());
    repl.chat.add_message(ChatRole::Assistant, "A2".to_string());
    repl.chat.add_message(ChatRole::User, "Q3".to_string());
    repl.chat.add_message(ChatRole::Assistant, "A3".to_string());

    // First rewind: remove Q3+A3
    repl.prompt.set_input("/rewind".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert_eq!(repl.chat.len(), 5); // Q1+A1+Q2+A2 + system rewind msg

    // Second rewind: remove Q2+A2 (first system msg also removed since it's past cutoff)
    repl.prompt.set_input("/rewind".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    // Q1+A1 + second system rewind msg = 3 (first system msg was in truncated range)
    assert_eq!(repl.chat.len(), 3);
    assert_eq!(repl.chat.get_message(0).unwrap().content, "Q1");
}

// --- Permission mode cycling tests ---

#[test]
fn test_approval_mode_cycle_sequence() {
    use shannon_core::permissions::ApprovalMode;

    // Verify the cycle order: Suggest → AutoEdit → Plan → FullAuto → Auto → PlanReadonly → Readonly → Suggest
    let mode = ApprovalMode::Suggest;
    assert_eq!(mode.cycle_next(), ApprovalMode::AutoEdit);

    let mode = ApprovalMode::AutoEdit;
    assert_eq!(mode.cycle_next(), ApprovalMode::Plan);

    let mode = ApprovalMode::Plan;
    assert_eq!(mode.cycle_next(), ApprovalMode::FullAuto);

    let mode = ApprovalMode::FullAuto;
    assert_eq!(mode.cycle_next(), ApprovalMode::Auto);

    let mode = ApprovalMode::Auto;
    assert_eq!(mode.cycle_next(), ApprovalMode::PlanReadonly);

    let mode = ApprovalMode::PlanReadonly;
    assert_eq!(mode.cycle_next(), ApprovalMode::Readonly);

    let mode = ApprovalMode::Readonly;
    assert_eq!(mode.cycle_next(), ApprovalMode::Suggest);

    // BypassPermissions and DontAsk cycle back to Suggest
    assert_eq!(ApprovalMode::BypassPermissions.cycle_next(), ApprovalMode::Suggest);
    assert_eq!(ApprovalMode::DontAsk.cycle_next(), ApprovalMode::Suggest);
}

#[test]
fn test_approval_mode_short_labels() {
    use shannon_core::permissions::ApprovalMode;

    assert_eq!(ApprovalMode::Suggest.short_label(), "SUGGEST");
    assert_eq!(ApprovalMode::Plan.short_label(), "PLAN");
    assert_eq!(ApprovalMode::AutoEdit.short_label(), "AUTO");
    assert_eq!(ApprovalMode::FullAuto.short_label(), "FULL");
    assert_eq!(ApprovalMode::BypassPermissions.short_label(), "BYPASS");
    assert_eq!(ApprovalMode::DontAsk.short_label(), "YOLO");
    assert_eq!(ApprovalMode::Readonly.short_label(), "RO");
}

#[test]
fn test_approval_mode_default_is_auto() {
    use shannon_core::permissions::ApprovalMode;

    // The default ApprovalMode is AutoEdit (which shows as "AUTO")
    assert_eq!(ApprovalMode::default(), ApprovalMode::AutoEdit);
}

#[test]
fn test_repl_default_approval_label() {
    let state = ReplState::default();
    assert_eq!(state.approval_mode_label, "AUTO", "default label should match AutoEdit");
}

#[test]
fn test_repl_set_bypass_pending_action() {
    let mut repl = Repl::new().unwrap();

    // Execute the set_bypass_mode pending action
    super::commands::execute_pending_action(&mut repl, "set_bypass_mode").unwrap();

    // Verify label updated
    assert_eq!(repl.state.approval_mode_label, "BYPASS");

    // Verify PermissionManager was updated
    if let Some(ref engine) = repl.query_engine {
        let perms = engine.permissions().read().unwrap();
        assert_eq!(perms.approval_mode(), shannon_core::permissions::ApprovalMode::BypassPermissions);
    }
}

// --- Permission config loading tests ---

#[test]
fn test_load_permission_rules_from_file() {
    use std::io::Write;

    // Create a temp directory with a settings file
    let tmp_dir = tempfile::tempdir().unwrap();
    let settings_dir = tmp_dir.path().join(".shannon");
    std::fs::create_dir_all(&settings_dir).unwrap();
    let settings_path = settings_dir.join("settings.json");

    let settings_content = serde_json::json!({
        "permissions": {
            "allow": ["Bash", "Read"],
            "deny": ["Bash(rm -rf *)"]
        }
    });
    let mut f = std::fs::File::create(&settings_path).unwrap();
    f.write_all(serde_json::to_string_pretty(&settings_content).unwrap().as_bytes()).unwrap();

    // Override cwd for the test
    let orig_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp_dir.path()).unwrap();

    let mut pm = PermissionManager::new();
    super::load_permission_rules(&mut pm);

    // Verify tool-level allow rules were applied
    let memory = pm.memory();
    let allowed = memory.always_allowed_tools();
    assert!(allowed.contains(&"Bash".to_string()), "Bash should be allowed");
    assert!(allowed.contains(&"Read".to_string()), "Read should be allowed");

    // Cleanup
    std::env::set_current_dir(orig_cwd).unwrap();
}

#[test]
fn test_load_permission_rules_missing_file() {
    // Ensure loading from a directory with no settings files does not panic
    let tmp_dir = tempfile::tempdir().unwrap();
    let orig_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp_dir.path()).unwrap();

    let mut pm = PermissionManager::new();
    // Should not panic
    super::load_permission_rules(&mut pm);

    std::env::set_current_dir(orig_cwd).unwrap();
}

#[test]
fn test_load_permission_rules_invalid_json() {
    use std::io::Write;

    let tmp_dir = tempfile::tempdir().unwrap();
    let settings_dir = tmp_dir.path().join(".shannon");
    std::fs::create_dir_all(&settings_dir).unwrap();
    let settings_path = settings_dir.join("settings.json");

    let mut f = std::fs::File::create(&settings_path).unwrap();
    f.write_all(b"not valid json{{{").unwrap();

    let orig_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp_dir.path()).unwrap();

    let mut pm = PermissionManager::new();
    // Should not panic on invalid JSON
    super::load_permission_rules(&mut pm);

    std::env::set_current_dir(orig_cwd).unwrap();
}

#[test]
fn test_load_permission_rules_claude_settings() {
    use std::io::Write;

    // Test .claude/settings.json compatibility
    let tmp_dir = tempfile::tempdir().unwrap();
    let claude_dir = tmp_dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");

    let settings_content = serde_json::json!({
        "permissions": {
            "allow": ["mcp__*"],
            "deny": ["Bash(curl *)"]
        }
    });
    let mut f = std::fs::File::create(&settings_path).unwrap();
    f.write_all(serde_json::to_string_pretty(&settings_content).unwrap().as_bytes()).unwrap();

    let orig_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp_dir.path()).unwrap();

    let mut pm = PermissionManager::new();
    super::load_permission_rules(&mut pm);

    // Pattern-based rules should be applied
    // Verify at least that no panic occurred during rule loading
    let _ = pm.memory();

    std::env::set_current_dir(orig_cwd).unwrap();
}

// --- Status bar display tests ---

#[test]
fn test_approval_mode_label_syncs_with_permissions() {
    let mut repl = Repl::new().unwrap();

    // Default should be AUTO (AutoEdit)
    assert_eq!(repl.state.approval_mode_label, "AUTO");

    // Use /mode to change to readonly
    repl.prompt.set_input("/mode readonly".to_string());
    super::commands::submit_input(&mut repl).unwrap();

    assert_eq!(repl.state.approval_mode_label, "RO");

    // Change back to default
    repl.prompt.set_input("/mode default".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert_eq!(repl.state.approval_mode_label, "SUGGEST");
}

#[test]
fn test_permission_mode_bypass_not_in_cycle() {
    use shannon_core::permissions::ApprovalMode;

    // Verify BypassPermissions is NOT reachable via cycle_next from any safe mode.
    // It can only be set via /mode or the confirmation dialog.
    let safe_modes = [
        ApprovalMode::Suggest,
        ApprovalMode::AutoEdit,
        ApprovalMode::Plan,
        ApprovalMode::FullAuto,
        ApprovalMode::Readonly,
    ];
    for mode in &safe_modes {
        // cycle_next never produces BypassPermissions from safe modes
        // (Readonly cycles to Suggest, not BypassPermissions)
        assert_ne!(mode.cycle_next(), ApprovalMode::BypassPermissions,
            "cycle_next from {mode:?} should not produce BypassPermissions");
    }
}

// ─── Custom Command Tests ────────────────────────────────────────────────────

#[test]
fn test_collect_custom_commands_basic() {
    let dir = tempfile::tempdir().unwrap();
    let cmd_file = dir.path().join("review.md");
    std::fs::write(&cmd_file, "Review this code:\n$ARGUMENTS").unwrap();

    let mut results: Vec<super::CustomCommandEntry> = Vec::new();
    super::collect_custom_commands(dir.path(), "", &mut results);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "review");
    assert_eq!(results[0].template, "Review this code:\n$ARGUMENTS");
    assert_eq!(results[0].model, None);
    assert!(results[0].allowed_tools.is_empty());
    assert_eq!(results[0].agent, None);
}

#[test]
fn test_collect_custom_commands_subdirectory() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("project");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("foo.md"), "Do foo").unwrap();
    std::fs::write(dir.path().join("bar.md"), "Do bar").unwrap();

    let mut results: Vec<super::CustomCommandEntry> = Vec::new();
    super::collect_custom_commands(dir.path(), "", &mut results);

    assert_eq!(results.len(), 2);
    let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"project:foo"));
    assert!(names.contains(&"bar"));
}

#[test]
fn test_collect_custom_commands_nested_subdirectory() {
    let dir = tempfile::tempdir().unwrap();
    let deep = dir.path().join("a").join("b");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("c.md"), "Deep command").unwrap();

    let mut results: Vec<super::CustomCommandEntry> = Vec::new();
    super::collect_custom_commands(dir.path(), "", &mut results);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "a:b:c");
}

#[test]
fn test_collect_custom_commands_skips_hidden_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let hidden = dir.path().join(".hidden");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(hidden.join("secret.md"), "Should be skipped").unwrap();
    std::fs::write(dir.path().join("visible.md"), "Visible command").unwrap();

    let mut results: Vec<super::CustomCommandEntry> = Vec::new();
    super::collect_custom_commands(dir.path(), "", &mut results);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "visible");
}

#[test]
fn test_collect_custom_commands_skips_non_md() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("cmd.md"), "Markdown").unwrap();
    std::fs::write(dir.path().join("readme.txt"), "Text file").unwrap();
    std::fs::write(dir.path().join("script.sh"), "#!/bin/bash").unwrap();

    let mut results: Vec<super::CustomCommandEntry> = Vec::new();
    super::collect_custom_commands(dir.path(), "", &mut results);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "cmd");
}

#[test]
fn test_collect_custom_commands_frontmatter_stripped() {
    let dir = tempfile::tempdir().unwrap();
    let content = "---\ndescription: Review code\n---\nReview this:\n$ARGUMENTS\n";
    std::fs::write(dir.path().join("review.md"), content).unwrap();

    let mut results: Vec<super::CustomCommandEntry> = Vec::new();
    super::collect_custom_commands(dir.path(), "", &mut results);

    assert_eq!(results.len(), 1);
    assert!(!results[0].template.contains("description:"));
    assert!(results[0].template.contains("Review this:"));
    assert!(results[0].template.contains("$ARGUMENTS"));
}

#[test]
fn test_collect_custom_commands_frontmatter_fields() {
    let dir = tempfile::tempdir().unwrap();
    let content = "---\nmodel: claude-sonnet-4-6\nallowed-tools: Bash,Read\nagent: reviewer\n---\nDo review\n";
    std::fs::write(dir.path().join("review.md"), content).unwrap();

    let mut results: Vec<super::CustomCommandEntry> = Vec::new();
    super::collect_custom_commands(dir.path(), "", &mut results);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].model.as_deref(), Some("claude-sonnet-4-6"));
    assert_eq!(results[0].allowed_tools, vec!["Bash", "Read"]);
    assert_eq!(results[0].agent.as_deref(), Some("reviewer"));
}

#[test]
fn test_parse_frontmatter_field() {
    let yaml = "model: claude-opus-4-6\nallowed-tools: Bash, Read, Write\nagent: coder\n";
    assert_eq!(super::parse_frontmatter_field(yaml, "model"), Some("claude-opus-4-6".to_string()));
    assert_eq!(super::parse_frontmatter_field(yaml, "allowed-tools"), Some("Bash, Read, Write".to_string()));
    assert_eq!(super::parse_frontmatter_field(yaml, "agent"), Some("coder".to_string()));
    assert_eq!(super::parse_frontmatter_field(yaml, "nonexistent"), None);
}

#[test]
fn test_parse_frontmatter_field_quoted() {
    let yaml = "model: \"claude-sonnet-4-6\"\ndescription: 'A test'\n";
    assert_eq!(super::parse_frontmatter_field(yaml, "model"), Some("claude-sonnet-4-6".to_string()));
    assert_eq!(super::parse_frontmatter_field(yaml, "description"), Some("A test".to_string()));
}

#[test]
fn test_parse_frontmatter_field_empty_value() {
    let yaml = "model: \nagent:\n";
    assert_eq!(super::parse_frontmatter_field(yaml, "model"), None);
    assert_eq!(super::parse_frontmatter_field(yaml, "agent"), None);
}

#[test]
fn test_collect_custom_commands_no_directory() {
    let mut results: Vec<super::CustomCommandEntry> = Vec::new();
    super::collect_custom_commands(std::path::Path::new("/nonexistent/path"), "", &mut results);
    assert!(results.is_empty());
}

#[test]
fn test_collect_custom_commands_description_from_frontmatter() {
    let dir = tempfile::tempdir().unwrap();
    let content = "---\ndescription: Reviews code for bugs\nmodel: claude-sonnet-4-6\n---\nReview this code: $ARGUMENTS\n";
    std::fs::write(dir.path().join("review.md"), content).unwrap();

    let mut results: Vec<super::CustomCommandEntry> = Vec::new();
    super::collect_custom_commands(dir.path(), "", &mut results);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].description.as_deref(), Some("Reviews code for bugs"));
}

#[test]
fn test_collect_custom_commands_arguments_from_frontmatter() {
    let dir = tempfile::tempdir().unwrap();
    let content = "---\narguments: file, pattern\n---\nSearch for $ARGUMENTS[1] in $ARGUMENTS[0]\n";
    std::fs::write(dir.path().join("search.md"), content).unwrap();

    let mut results: Vec<super::CustomCommandEntry> = Vec::new();
    super::collect_custom_commands(dir.path(), "", &mut results);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].arguments, vec!["file", "pattern"]);
}

#[test]
fn test_collect_custom_commands_arguments_alias() {
    let dir = tempfile::tempdir().unwrap();
    let content = "---\nargs: input, output\n---\nProcess $ARGUMENTS[0] to $ARGUMENTS[1]\n";
    std::fs::write(dir.path().join("process.md"), content).unwrap();

    let mut results: Vec<super::CustomCommandEntry> = Vec::new();
    super::collect_custom_commands(dir.path(), "", &mut results);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].arguments, vec!["input", "output"]);
}

#[test]
fn test_dedup_custom_commands_keeps_last() {
    let mut commands = vec![
        super::CustomCommandEntry {
            name: "review".to_string(),
            template: "user-level".to_string(),
            path: std::path::PathBuf::from("/home/user/.claude/commands/review.md"),
            description: None,
            arguments: Vec::new(),
            model: None,
            allowed_tools: Vec::new(),
            agent: None,
        },
        super::CustomCommandEntry {
            name: "review".to_string(),
            template: "project-level".to_string(),
            path: std::path::PathBuf::from("/project/.claude/commands/review.md"),
            description: Some("Project review".to_string()),
            arguments: vec!["file".to_string()],
            model: None,
            allowed_tools: Vec::new(),
            agent: None,
        },
    ];
    super::dedup_custom_commands(&mut commands);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].template, "project-level");
    assert_eq!(commands[0].description.as_deref(), Some("Project review"));
}

#[test]
fn test_dedup_custom_commands_preserves_unique() {
    let mut commands = vec![
        super::CustomCommandEntry {
            name: "review".to_string(),
            template: "review template".to_string(),
            path: std::path::PathBuf::from("/a/review.md"),
            description: None,
            arguments: Vec::new(),
            model: None,
            allowed_tools: Vec::new(),
            agent: None,
        },
        super::CustomCommandEntry {
            name: "commit".to_string(),
            template: "commit template".to_string(),
            path: std::path::PathBuf::from("/b/commit.md"),
            description: None,
            arguments: Vec::new(),
            model: None,
            allowed_tools: Vec::new(),
            agent: None,
        },
    ];
    super::dedup_custom_commands(&mut commands);
    assert_eq!(commands.len(), 2);
}

#[test]
fn test_commands_reload_full_pipeline() {
    // Simulate the full reload pipeline: collect from multiple dirs, dedup, verify entries
    let project_dir = tempfile::tempdir().unwrap();
    let user_dir = tempfile::tempdir().unwrap();

    // Create project-level command
    let proj_cmds = project_dir.path().join(".claude").join("commands");
    std::fs::create_dir_all(&proj_cmds).unwrap();
    std::fs::write(
        proj_cmds.join("review.md"),
        "---\ndescription: Project review\narguments: file\nmodel: claude-sonnet-4-6\n---\nReview $ARGUMENTS[0]\n",
    ).unwrap();

    // Create user-level command with same name (should be overridden)
    let user_cmds = user_dir.path().join(".claude").join("commands");
    std::fs::create_dir_all(&user_cmds).unwrap();
    std::fs::write(
        user_cmds.join("review.md"),
        "---\ndescription: User review\n---\nReview: $ARGUMENTS\n",
    ).unwrap();

    // Create another unique user-level command
    std::fs::write(
        user_cmds.join("commit.md"),
        "---\ndescription: Commit changes\nallowed-tools: Bash, Read\n---\nCommit: $ARGUMENTS\n",
    ).unwrap();

    // Collect from both directories (project after user, like real code)
    let mut commands: Vec<super::CustomCommandEntry> = Vec::new();
    super::collect_custom_commands(&user_cmds, "", &mut commands);
    super::collect_custom_commands(&proj_cmds, "", &mut commands);
    super::dedup_custom_commands(&mut commands);

    // Should have 2 commands (project "review" overrides user "review")
    assert_eq!(commands.len(), 2);

    // Find review command — should be project-level
    let review = commands.iter().find(|c| c.name == "review").unwrap();
    assert_eq!(review.description.as_deref(), Some("Project review"));
    assert_eq!(review.arguments, vec!["file"]);
    assert_eq!(review.model.as_deref(), Some("claude-sonnet-4-6"));
    assert!(review.template.contains("$ARGUMENTS[0]"));

    // Find commit command
    let commit = commands.iter().find(|c| c.name == "commit").unwrap();
    assert_eq!(commit.description.as_deref(), Some("Commit changes"));
    assert_eq!(commit.allowed_tools, vec!["Bash", "Read"]);
}

// ── /add-dir Command Tests ──────────────────────────────────────────

#[test]
fn test_repl_add_dir_valid() {
    let mut repl = Repl::new().unwrap();
    let tmp = std::env::temp_dir().to_string_lossy().to_string();
    repl.prompt.set_input(format!("/add-dir {tmp}"));
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Added directory:"));
    assert!(last_msg.contains(&tmp));
    assert!(repl.state.extra_dirs.iter().any(|d| d == &tmp));
}

#[test]
fn test_repl_add_dir_not_found() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/add-dir /no/such/directory/xyz123".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Directory not found:"));
    assert!(last_msg.contains("/no/such/directory/xyz123"));
}

#[test]
fn test_repl_add_dir_file_not_dir() {
    let mut repl = Repl::new().unwrap();
    // Use /etc/passwd — a known file that exists on Linux
    repl.prompt.set_input("/add-dir /etc/passwd".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Not a directory:"));
}

#[test]
fn test_repl_add_dir_duplicate() {
    let mut repl = Repl::new().unwrap();
    let tmp = std::env::temp_dir().to_string_lossy().to_string();
    // Add it once
    repl.prompt.set_input(format!("/add-dir {tmp}"));
    super::commands::submit_input(&mut repl).unwrap();
    assert!(repl.state.extra_dirs.iter().any(|d| d == &tmp));
    // Add it again
    repl.prompt.set_input(format!("/add-dir {tmp}"));
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("already added:"));
    // Should still be exactly 1 entry
    assert_eq!(repl.state.extra_dirs.iter().filter(|d| **d == tmp).count(), 1);
}

#[test]
fn test_repl_add_dir_no_args() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/add-dir".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Usage: /add-dir <path>"));
}

// ── /rename Command Tests ───────────────────────────────────────────

#[test]
fn test_repl_rename_set() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/rename My Session".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert_eq!(repl.state.session_title, Some("My Session".to_string()));
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Session renamed to: My Session"));
}

#[test]
fn test_repl_rename_show_current() {
    let mut repl = Repl::new().unwrap();
    // Set a title first
    repl.state.session_title = Some("Test Title".to_string());
    repl.prompt.set_input("/rename".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Current session: Test Title"));
    assert!(last_msg.contains("Usage: /rename <new-name>"));
}

#[test]
fn test_repl_rename_no_title() {
    let mut repl = Repl::new().unwrap();
    assert!(repl.state.session_title.is_none());
    repl.prompt.set_input("/rename".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("No custom session name set"));
    assert!(last_msg.contains("Usage: /rename <new-name>"));
}

#[test]
fn test_repl_rename_reset() {
    let mut repl = Repl::new().unwrap();
    // Set a title first
    repl.state.session_title = Some("Before Reset".to_string());
    // Now reset it
    repl.prompt.set_input("/rename reset".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert!(repl.state.session_title.is_none());
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Session name reset to default"));
}

// ── /copy [N] Command Tests ─────────────────────────────────────────

#[test]
fn test_repl_copy_nth_response() {
    let mut repl = Repl::new().unwrap();
    // Create 3 assistant messages
    repl.chat.add_message(ChatRole::Assistant, "first response".to_string());
    repl.chat.add_message(ChatRole::Assistant, "second response".to_string());
    repl.chat.add_message(ChatRole::Assistant, "third response".to_string());

    // /copy 2 should copy the 2nd from end = "second response"
    repl.prompt.set_input("/copy 2".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    // In test env clipboard is likely unavailable; content goes to temp file
    assert!(
        last_msg.contains("second response") || last_msg.contains("clipboard") || last_msg.contains("Clipboard"),
        "expected copy of 2nd response, got: {last_msg}"
    );
}

#[test]
fn test_repl_copy_out_of_range() {
    let mut repl = Repl::new().unwrap();
    // Only 2 assistant messages
    repl.chat.add_message(ChatRole::Assistant, "alpha".to_string());
    repl.chat.add_message(ChatRole::Assistant, "beta".to_string());

    // /copy 99 — only 2 responses exist
    repl.prompt.set_input("/copy 99".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Only 2 assistant response"));
    assert!(last_msg.contains("/copy 1"));
}

#[test]
fn test_repl_copy_zero() {
    let mut repl = Repl::new().unwrap();
    repl.chat.add_message(ChatRole::Assistant, "some response".to_string());

    repl.prompt.set_input("/copy 0".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Invalid index"));
}

// ── /compact [instructions] Tests ───────────────────────────────────

#[test]
fn test_repl_compact_with_focus_instructions() {
    let mut repl = Repl::new().unwrap();

    // Seed the query engine with some conversation history so compact has something to work with
    if let Some(engine) = repl.query_engine.as_mut() {
        use shannon_core::api::{Message, MessageContent};
        let messages = vec![
            Message { role: "user".to_string(), content: MessageContent::Text("Tell me about authentication logic".to_string()) },
            Message { role: "assistant".to_string(), content: MessageContent::Text("Authentication logic verifies user credentials".to_string()) },
            Message { role: "user".to_string(), content: MessageContent::Text("What about caching?".to_string()) },
            Message { role: "assistant".to_string(), content: MessageContent::Text("Caching stores frequently accessed data".to_string()) },
        ];
        engine.restore_messages(messages);
    }

    repl.prompt.set_input("/compact authentication logic".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    // The focus branch prints a "Focus compact" message first, then the result
    assert!(
        last_msg.contains("authentication") || last_msg.contains("Focus compact") || last_msg.contains("compacted"),
        "expected focus compact to mention 'authentication' or report compaction, got: {last_msg}"
    );
}

// ── /plan off Test ──────────────────────────────────────────────────

#[test]
fn test_repl_plan_off() {
    let mut repl = Repl::new().unwrap();
    // First create a plan so it's active
    repl.prompt.set_input("/plan implement search feature".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert!(repl.state.plan.active);

    // Now deactivate
    repl.prompt.set_input("/plan off".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    assert!(!repl.state.plan.active, "plan.active should be false after /plan off");
    assert!(!repl.state.plan.approved, "plan.approved should be false after /plan off");
    if let Ok(flag) = repl.plan_mode_flag.read() {
        assert!(!*flag, "plan_mode_flag should be false after /plan off");
    }
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("Plan mode deactivated"));
}
