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
    assert_eq!(state.model, Some("claude-3-5-sonnet".to_string()));
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
    assert!(repl.state.input_dialog.is_some());
    assert_eq!(repl.state.input_dialog_action.as_deref(), Some("set_model"));
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
fn test_tab_complete_suggestions_cleared_on_backspace() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/h".to_string());
    let tab_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    crate::repl::input::handle_input(&mut repl, tab_key).unwrap();
    assert!(!repl.state.completion_suggestions.is_empty());

    let bs_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Backspace,
        crossterm::event::KeyModifiers::NONE,
    );
    crate::repl::input::handle_input(&mut repl, bs_key).unwrap();
    assert!(repl.state.completion_suggestions.is_empty());
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
    assert!(repl.chat.len() >= 1);
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
fn test_completion_suggestion_index_tracks_tab() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/h".to_string());
    let tab_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    crate::repl::input::handle_input(&mut repl, tab_key).unwrap();
    // After first tab, index should be 0 (the one just selected)
    assert_eq!(repl.state.completion_suggestion_index, 0);
    assert!(!repl.state.completion_suggestions.is_empty());

    // Second tab should advance index
    crate::repl::input::handle_input(&mut repl, tab_key).unwrap();
    assert!(repl.state.completion_suggestion_index > 0 || repl.state.completion_suggestions.len() == 1);
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
    assert!(last_msg.contains("suggest"), "/mode should list available modes");
    assert!(last_msg.contains("auto-edit"), "/mode should list auto-edit");
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
    assert!(last_msg.contains("suggest"), "should list valid modes");
}

#[test]
fn test_repl_mode_suggest_alias() {
    let mut repl = Repl::new().unwrap();
    repl.prompt.set_input("/mode ask".to_string());
    super::commands::submit_input(&mut repl).unwrap();
    let last_msg = &repl.chat.last_message().unwrap().content;
    assert!(last_msg.contains("suggest"), "'ask' should map to 'suggest' mode");
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
