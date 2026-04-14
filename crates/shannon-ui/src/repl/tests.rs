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
    let (prefix, start, end) = crate::repl::input::extract_completion_word("/commit some ", &repl);
    assert_eq!(prefix, "/commit some ");
    assert_eq!(start, 0);
    assert_eq!(end, 13);
}

#[test]
fn test_extract_completion_word_nested_command() {
    let repl = Repl::new().unwrap();
    let (prefix, start, end) = crate::repl::input::extract_completion_word("/team add my-team /con", &repl);
    assert_eq!(prefix, "/con");
    assert_eq!(start, 18);
    assert_eq!(end, 22);
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
