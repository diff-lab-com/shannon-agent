//! UI snapshot tests — render widgets to a buffer and verify output with insta.
//!
//! Run with: cargo test --package shannon-ui --test ui_snapshot_tests -- --test-threads=1

use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect};
use shannon_ui::theme::Theme;
use shannon_ui::{ChatMessage, ChatRole, StatusBarWidget};

/// Helper: create a terminal with a TestBackend of given size.
fn test_terminal(w: u16, h: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(w, h);
    Terminal::new(backend).unwrap()
}

/// Helper: extract text content from a buffer region (stripping trailing spaces per line).
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

/// Fixed timestamp for deterministic snapshots.
fn fixed_timestamp() -> chrono::DateTime<chrono::Utc> {
    "2026-01-15T12:00:00Z".parse().unwrap()
}

/// Helper: build a ChatMessage with sensible defaults.
fn make_message(role: ChatRole, content: &str) -> ChatMessage {
    ChatMessage {
        role,
        content: content.to_string(),
        timestamp: fixed_timestamp(),
        image_lines: None,
        is_error: false,
        tool_name: None,
        start_time: None,
        duration_secs: None,
        spinner_frame: 0,
        folded: true,
        exit_code: None,
        thinking_content: None,
        thinking_expanded: false,
        thinking_duration_secs: None,
        diff_stats: None,
        stats_line: None,
    }
}

/// Helper: build a tool ChatMessage.
fn make_tool_message(tool_name: &str, content: &str, is_error: bool) -> ChatMessage {
    ChatMessage {
        role: ChatRole::Tool,
        content: content.to_string(),
        timestamp: fixed_timestamp(),
        image_lines: None,
        is_error,
        tool_name: Some(tool_name.to_string()),
        start_time: None,
        duration_secs: Some(1.5),
        spinner_frame: 0,
        folded: true,
        exit_code: if is_error { Some(1) } else { Some(0) },
        thinking_content: None,
        thinking_expanded: false,
        thinking_duration_secs: None,
        diff_stats: None,
        stats_line: None,
    }
}

// ── Chat Message Snapshots ───────────────────────────────────────────

#[test]
fn test_chat_message_text_snapshot() {
    let msg = make_message(ChatRole::User, "Hello, can you help me with Rust?");
    insta::assert_snapshot!("chat_message_text", format!("{:#?}", msg));
}

#[test]
fn test_chat_message_tool_call_snapshot() {
    let msg = make_tool_message("bash", "cargo test\nrunning 42 tests\nall passed", false);
    insta::assert_snapshot!("chat_message_tool_call", format!("{:#?}", msg));
}

#[test]
fn test_chat_message_error_snapshot() {
    let msg = make_tool_message("write", "Permission denied: /etc/hosts", true);
    insta::assert_snapshot!("chat_message_error", format!("{:#?}", msg));
}

#[test]
fn test_chat_message_thinking_snapshot() {
    let mut msg = make_message(ChatRole::Assistant, "Here is the answer:");
    msg.thinking_content = Some("Let me analyze the code step by step...".to_string());
    msg.thinking_duration_secs = Some(3.2);
    msg.thinking_expanded = false;
    insta::assert_snapshot!("chat_message_thinking", format!("{:#?}", msg));
}

#[test]
fn test_tool_status_snapshot() {
    let mut msg = make_tool_message("bash", "running...", false);
    msg.spinner_frame = 3;
    msg.duration_secs = None; // still running
    insta::assert_snapshot!("tool_status_running", format!("{:#?}", msg));
}

#[test]
fn test_diff_stats_snapshot() {
    let mut msg = make_tool_message("write", "src/main.rs written", false);
    msg.diff_stats = Some((42, 7));
    insta::assert_snapshot!("diff_stats", format!("{:#?}", msg));
}

#[test]
fn test_progress_indicator_snapshot() {
    // Snapshot a status bar rendering in "Thinking..." state with a model configured.
    let mut terminal = test_terminal(80, 2);
    let theme = Theme::default_dark();

    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 80, 2);
            StatusBarWidget::render_with_spinner(
                f,
                area,
                "Thinking...",
                Some("claude-sonnet-4"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                &theme,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                true,
                5000,
                None,
                None,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer().clone();
    let text = buffer_text(&buf, Rect::new(0, 0, 80, 2));
    insta::assert_snapshot!("progress_indicator", text);
}

#[test]
fn test_stats_line_snapshot() {
    // Snapshot a message that has a stats_line field set.
    let mut msg = make_message(ChatRole::Assistant, "Done!");
    msg.stats_line = Some("960 tokens · $0.0142".to_string());
    msg.diff_stats = Some((15, 3));
    insta::assert_snapshot!("stats_line", format!("{:#?}", msg));
}

#[test]
fn test_cache_hit_rate_snapshot() {
    // Snapshot the status bar when cache tokens are present (hit rate should display).
    let mut terminal = test_terminal(120, 2);
    let theme = Theme::default_dark();

    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 120, 2);
            StatusBarWidget::render_with_spinner(
                f,
                area,
                "Responding",
                Some("claude-sonnet-4"),
                None,
                Some(5000),   // tokens_used
                Some(200000), // max_tokens (200k context)
                Some(0.0234), // cost_usd
                None,         // git_branch
                None,         // spinner
                None,         // progress_bar
                &theme,
                None,              // approval_mode
                Some((5000, 800)), // token_breakdown (input, output)
                Some(4500),        // cache_read_tokens — 4500 read from cache
                Some(500),         // cache_creation_tokens — 500 written to cache
                None,              // diag_counts
                None,              // rate_limit
                None,              // files_info
                None,              // tools_invoked
                None,              // session_duration
                false,             // thinking_phase
                0,                 // thinking_chars
                None,              // turn_count
                None,              // memory_rss_kb
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer().clone();
    let text = buffer_text(&buf, Rect::new(0, 0, 120, 2));
    insta::assert_snapshot!("cache_hit_rate", text);

    // Verify the rendered output contains a cache hit rate indicator
    assert!(
        text.to_lowercase().contains("cache"),
        "Status bar should show cache hit rate, got: {text}"
    );
}

#[test]
fn test_cache_zero_tokens_snapshot() {
    // Snapshot the status bar when cache tokens are both zero (no cache indicator).
    let mut terminal = test_terminal(120, 2);
    let theme = Theme::default_dark();

    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 120, 2);
            StatusBarWidget::render_with_spinner(
                f,
                area,
                "Responding",
                Some("claude-sonnet-4"),
                None,
                Some(5000),
                Some(200000),
                Some(0.0234),
                None, // git_branch
                None, // spinner
                None, // progress_bar
                &theme,
                None,
                Some((5000, 800)),
                Some(0), // cache_read_tokens = 0
                Some(0), // cache_creation_tokens = 0
                None,
                None,
                None,
                None,
                None,
                false,
                0,
                None,
                None,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer().clone();
    let text = buffer_text(&buf, Rect::new(0, 0, 120, 2));
    insta::assert_snapshot!("cache_zero_tokens", text);
}
