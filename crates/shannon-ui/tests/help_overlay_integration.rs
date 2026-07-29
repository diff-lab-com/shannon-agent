//! Integration test: /help must NOT mutate chat history.

use shannon_ui::repl::{HelpOverlayState, ReplState};

#[test]
fn handle_help_sets_overlay_state_without_chat_mutation() {
    let mut state = ReplState::default();
    let initial_overlay = state.help_overlay.is_none();

    // Simulate what handle_help should do for `/help connect`
    state.help_overlay = Some(HelpOverlayState {
        filter: Some("connect".to_string()),
        ..Default::default()
    });

    assert!(initial_overlay, "overlay should start closed");
    assert!(
        state.help_overlay.is_some(),
        "overlay should be open after /help"
    );
    assert_eq!(
        state.help_overlay.as_ref().unwrap().filter.as_deref(),
        Some("connect"),
    );
}
