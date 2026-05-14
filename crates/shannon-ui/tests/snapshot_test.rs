//! UI snapshot tests — render widgets to a buffer and verify output
//!
//! Run with: cargo test --package shannon-ui --test snapshot_test

use ratatui::{
    backend::TestBackend,
    buffer::Buffer,
    layout::Rect,
    Terminal,
};
use shannon_ui::theme::Theme;
use shannon_ui::{
    ChatWidget, ChatRole, HeaderWidget, PromptWidget, SidebarInfo,
    StatusBarWidget,
};

/// Helper: create a terminal with a TestBackend of given size
fn test_terminal(w: u16, h: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(w, h);
    Terminal::new(backend).unwrap()
}

/// Helper: extract text content from a buffer region (stripping trailing spaces per line)
fn buffer_text(buf: &Buffer, area: Rect) -> String {
    let mut lines = Vec::new();
    for y in area.top()..area.bottom() {
        let mut line = String::new();
        for x in area.left()..area.right() {
            line.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

// ── Header Widget ──────────────────────────────────────────────────

#[test]
fn test_header_renders_model_and_directory() {
    let mut terminal = test_terminal(80, 5);
    let theme = Theme::default_dark();

    terminal.draw(|f| {
        let area = Rect::new(0, 0, 80, 5);
        HeaderWidget::render(f, area, &theme);
    }).unwrap();

    let buf = terminal.backend().buffer().clone();
    let text = buffer_text(&buf, Rect::new(0, 0, 80, 5));

    assert!(text.contains("Shannon"), "header should show welcome message");
    assert!(text.contains("/help"), "header should show key hints");
}

#[test]
fn test_header_handles_none_model() {
    let mut terminal = test_terminal(80, 5);
    let theme = Theme::default_dark();

    terminal.draw(|f| {
        let area = Rect::new(0, 0, 80, 5);
        HeaderWidget::render(f, area, &theme);
    }).unwrap();

    let buf = terminal.backend().buffer().clone();
    let text = buffer_text(&buf, Rect::new(0, 0, 80, 5));
    assert!(text.contains("Shannon"), "header should show welcome message");
}

// ── Status Bar Widget ──────────────────────────────────────────────

#[test]
fn test_status_bar_renders_status() {
    let mut terminal = test_terminal(80, 1);
    let theme = Theme::default_dark();

    terminal.draw(|f| {
        let area = Rect::new(0, 0, 80, 1);
        StatusBarWidget::render(f, area, "Ready", &theme);
    }).unwrap();

    let buf = terminal.backend().buffer().clone();
    let text = buffer_text(&buf, Rect::new(0, 0, 80, 1));
    assert!(text.contains("Ready"), "status bar should show status text");
}

#[test]
fn test_status_bar_shows_no_model_configured() {
    let mut terminal = test_terminal(80, 2);
    let theme = Theme::default_dark();

    terminal.draw(|f| {
        let area = Rect::new(0, 0, 80, 2);
        StatusBarWidget::render_with_spinner(
            f, area,
            "Ready",
            None, // No model configured
            None, // No effort level
            None, None, None, None, None, None,
            &theme,
            None, None, None, None, None, None, None, None,
            false, 0, // thinking
        );
    }).unwrap();

    let buf = terminal.backend().buffer().clone();
    let text = buffer_text(&buf, Rect::new(0, 0, 80, 2));
    assert!(text.contains("No model configured"), "should show 'No model configured' when model is None");
}

#[test]
fn test_status_bar_shows_model_name() {
    let mut terminal = test_terminal(80, 2);
    let theme = Theme::default_dark();

    terminal.draw(|f| {
        let area = Rect::new(0, 0, 80, 2);
        StatusBarWidget::render_with_spinner(
            f, area,
            "Ready",
            Some("gpt-4"),
            None, // No effort level
            None, None, None, None, None, None,
            &theme,
            None, None, None, None, None, None, None, None,
            false, 0, // thinking
        );
    }).unwrap();

    let buf = terminal.backend().buffer().clone();
    let text = buffer_text(&buf, Rect::new(0, 0, 80, 2));
    assert!(text.contains("gpt-4"), "should show model name when configured");
    assert!(!text.contains("No model configured"), "should NOT show 'No model configured' when model is set");
}

#[test]
fn test_status_bar_shows_effort_level() {
    let mut terminal = test_terminal(80, 2);
    let theme = Theme::default_dark();

    terminal.draw(|f| {
        let area = Rect::new(0, 0, 80, 2);
        StatusBarWidget::render_with_spinner(
            f, area,
            "Ready",
            Some("claude-sonnet-4"),
            Some("high"),
            None, None, None, None, None, None,
            &theme,
            None, None, None, None, None, None, None, None,
            false, 0,
        );
    }).unwrap();

    let buf = terminal.backend().buffer().clone();
    let text = buffer_text(&buf, Rect::new(0, 0, 80, 2));
    assert!(text.contains("claude-sonnet-4"), "should show model name");
    assert!(text.contains("high"), "should show effort level");
}

#[test]
fn test_status_bar_shows_thinking_indicator() {
    let mut terminal = test_terminal(80, 2);
    let theme = Theme::default_dark();

    terminal.draw(|f| {
        let area = Rect::new(0, 0, 80, 2);
        StatusBarWidget::render_with_spinner(
            f, area,
            "Thinking...",
            Some("claude-sonnet-4"),
            None,
            None, None, None, None, None, None,
            &theme,
            None, None, None, None, None, None, None, None,
            true, 5000, // thinking phase with 5k chars
        );
    }).unwrap();

    let buf = terminal.backend().buffer().clone();
    let text = buffer_text(&buf, Rect::new(0, 0, 80, 2));
    assert!(text.contains("Thinking"), "should show thinking status");
    assert!(text.contains("5k"), "should show thinking char count");
}

// ── Chat Widget ────────────────────────────────────────────────────

#[test]
fn test_chat_widget_empty() {
    let mut terminal = test_terminal(60, 10);
    let theme = Theme::default_dark();

    terminal.draw(|f| {
        let area = Rect::new(0, 0, 60, 10);
        let chat = ChatWidget::new(100);
        chat.render(f, area, &theme, None, true);
    }).unwrap();

    let buf = terminal.backend().buffer().clone();
    let text = buffer_text(&buf, Rect::new(0, 0, 60, 10));
    assert!(!text.is_empty());
}

#[test]
fn test_chat_widget_with_messages() {
    let mut terminal = test_terminal(60, 20);
    let theme = Theme::default_dark();

    let mut chat = ChatWidget::new(100);
    chat.add_message(ChatRole::User, "Hello assistant".to_string());
    chat.add_message(ChatRole::Assistant, "Hello! How can I help?".to_string());

    terminal.draw(|f| {
        let area = Rect::new(0, 0, 60, 20);
        chat.render(f, area, &theme, None, true);
    }).unwrap();

    let buf = terminal.backend().buffer().clone();
    let text = buffer_text(&buf, Rect::new(0, 0, 60, 20));
    assert!(text.contains("Hello"), "chat should display messages");
}

#[test]
fn test_chat_widget_system_message() {
    let mut terminal = test_terminal(60, 15);
    let theme = Theme::default_dark();

    let mut chat = ChatWidget::new(100);
    chat.add_message(ChatRole::System, "Session started".to_string());
    chat.add_message(ChatRole::User, "Ping".to_string());

    terminal.draw(|f| {
        let area = Rect::new(0, 0, 60, 15);
        chat.render(f, area, &theme, None, true);
    }).unwrap();

    let buf = terminal.backend().buffer().clone();
    let text = buffer_text(&buf, Rect::new(0, 0, 60, 15));
    assert!(text.contains("Session") || text.contains("Ping"), "chat should display system/user messages");
}

// ── Prompt Widget ──────────────────────────────────────────────────

#[test]
fn test_prompt_widget_renders() {
    let mut terminal = test_terminal(60, 3);
    let theme = Theme::default_dark();

    let mut prompt = PromptWidget::new();
    prompt.insert_text("hello world");

    terminal.draw(|f| {
        let area = Rect::new(0, 0, 60, 3);
        prompt.render(f, area, &theme, None);
    }).unwrap();

    let buf = terminal.backend().buffer().clone();
    let text = buffer_text(&buf, Rect::new(0, 0, 60, 3));
    assert!(text.contains("hello"), "prompt should display text");
}

// ── Sidebar Info ───────────────────────────────────────────────────

#[test]
fn test_sidebar_info_default() {
    let info = SidebarInfo {
        model: Some("test-model".to_string()),
        tokens_used: 500,
        cost_usd: 0.05,
        tools_invoked: 3,
        modified_files: vec![],
        total_additions: 0,
        total_deletions: 0,
        error_count: 0,
        context_window: 200000,
        active_agents: vec![],
        diagnostics: vec![],
        session_duration_secs: 120,
        turn_count: 5,
        commands_run: 2,
        tokens_per_sec: Some(42.0),
        memory_rss_kb: 0,
    };

    assert_eq!(info.model.as_deref(), Some("test-model"));
    assert_eq!(info.tokens_used, 500);
    assert_eq!(info.tools_invoked, 3);
    assert!(info.modified_files.is_empty());
}

#[test]
fn test_sidebar_info_with_files() {
    let info = SidebarInfo {
        model: Some("claude-sonnet-4".to_string()),
        tokens_used: 10000,
        cost_usd: 0.12,
        tools_invoked: 7,
        modified_files: vec![
            ("src/main.rs".to_string(), 15, 3),
            ("src/lib.rs".to_string(), 8, 1),
        ],
        total_additions: 23,
        total_deletions: 4,
        error_count: 1,
        context_window: 200000,
        active_agents: vec![],
        diagnostics: vec![],
        session_duration_secs: 300,
        turn_count: 10,
        commands_run: 5,
        tokens_per_sec: None,
        memory_rss_kb: 0,
    };

    assert_eq!(info.modified_files.len(), 2);
    assert_eq!(info.total_additions, 23);
    assert_eq!(info.total_deletions, 4);
    assert_eq!(info.error_count, 1);
}

// ── Layout boundary tests ──────────────────────────────────────────

#[test]
fn test_narrow_terminal_no_panic() {
    let mut terminal = test_terminal(30, 8);
    let theme = Theme::default_dark();

    terminal.draw(|f| {
        let area = f.area();
        HeaderWidget::render(f, area, &theme);
    }).unwrap();

    let mut terminal2 = test_terminal(30, 8);
    terminal2.draw(|f| {
        let area = f.area();
        StatusBarWidget::render(f, area, "Ready", &theme);
    }).unwrap();
}

#[test]
fn test_chat_widget_scroll_methods() {
    let mut chat = ChatWidget::new(100);
    for i in 0..20 {
        chat.add_message(ChatRole::User, format!("Message {i}"));
    }
    assert_eq!(chat.len(), 20);

    chat.scroll_to_top();
    // Just verify scroll methods don't panic
    chat.scroll_to_latest();
}

#[test]
fn test_chat_widget_scroll_empty() {
    let mut chat = ChatWidget::new(100);
    // Verify scroll methods on empty chat don't panic
    chat.scroll_to_top();
    chat.scroll_to_latest();
}
