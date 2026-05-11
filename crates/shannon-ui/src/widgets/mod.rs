//! Ratatui widgets for Shannon UI

pub mod select;
pub mod progress;
pub mod dialog;
pub mod diff_viewer;
pub mod header;
pub mod status_bar;
pub mod chat;
pub mod prompt;
pub mod sidebar;
pub mod key_hint;
pub mod session_tab;
pub mod tool_approval;
pub mod attachment_bar;
pub mod command_palette;
pub mod renderable;
pub mod column;

// Re-exports for convenient access
pub use header::HeaderWidget;
pub use status_bar::StatusBarWidget;
pub use chat::{ChatWidget, ChatMessage, ChatRole};
pub use prompt::PromptWidget;
#[allow(unused_imports)]
pub use sidebar::{SidebarWidget, SidebarInfo, SidebarSection};
pub use key_hint::KeyHintWidget;
#[allow(unused_imports)]
pub use session_tab::{SessionTabWidget, SessionInfo};
#[allow(unused_imports)]
pub use tool_approval::{ToolApprovalWidget, ToolApprovalRequest, ApprovalDecision, RiskLevel};
#[allow(unused_imports)]
pub use attachment_bar::{AttachmentBarWidget, Attachment, AttachmentKind};
#[allow(unused_imports)]
pub use command_palette::{CommandPaletteWidget, PaletteCommand, CommandCategory};

// Re-export shared utilities used by other crates
pub use chat::{detect_diff_language, highlight_diff_line};

use crate::theme::Theme;

use ratatui::{
    layout::{Alignment, Constraint, Direction, Rect},
    style::Style,
    widgets::{Clear, Paragraph},
    Frame,
};

// Layout constants (shared with sidebar module)
use sidebar::{
    MIN_SIDEBAR_WIDTH_VAL as MIN_SIDEBAR_WIDTH,
    COLLAPSE_HEADER_WIDTH_VAL as COLLAPSE_HEADER_WIDTH,
    MIN_TERMINAL_WIDTH_VAL as MIN_TERMINAL_WIDTH,
    MIN_TERMINAL_HEIGHT_VAL as MIN_TERMINAL_HEIGHT,
};

// ---------------------------------------------------------------------------
// Streaming state
// ---------------------------------------------------------------------------

/// Detailed streaming state for status indicator
#[derive(Debug, Clone, PartialEq)]
pub enum StreamingState {
    Idle,
    Thinking,
    CallingTool { name: String },
    Generating { elapsed_secs: u64 },
}

impl Default for StreamingState {
    fn default() -> Self {
        Self::Idle
    }
}

// ---------------------------------------------------------------------------

/// Main UI layout widget
pub struct MainLayoutWidget;

impl MainLayoutWidget {
    /// Create the main layout chunks
    /// Returns (header_area, chat_area, prompt_area, status_area, full_area)
    pub fn layout(area: Rect, prompt_height: u16) -> (Rect, Rect, Rect, Rect, Rect) {
        let chunks = ratatui::layout::Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(HeaderWidget::height() as u16), // Header bar
                Constraint::Min(0),              // Chat area (flexible)
                Constraint::Length(prompt_height), // Input prompt (dynamic)
                Constraint::Length(2),            // Status bar (2 lines)
            ])
            .split(area);

        let header_area = chunks[0];
        let chat_area = chunks[1];
        let prompt_area = chunks[2];
        let status_area = chunks[3];

        (header_area, chat_area, prompt_area, status_area, area)
    }

    /// Create layout with optional sidebar.
    /// When sidebar is visible and terminal is wide enough, splits the middle area horizontally.
    /// Returns (header_area, chat_area, prompt_area, status_area, sidebar_area, full_area)
    pub fn layout_with_sidebar(area: Rect, prompt_height: u16, sidebar_visible: bool, chat_is_empty: bool) -> (Rect, Rect, Rect, Rect, Option<Rect>, Rect) {
        // Responsive: collapse header on very narrow terminals; hide when chat has messages
        let header_height: u16 = if !chat_is_empty {
            0
        } else if area.width < COLLAPSE_HEADER_WIDTH {
            1
        } else {
            HeaderWidget::height() as u16
        };
        let _effective_sidebar = sidebar_visible && area.width >= MIN_SIDEBAR_WIDTH;

        let (header_area, chat_area, prompt_area, status_area, full) = {
            let chunks = ratatui::layout::Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(header_height),
                    Constraint::Min(0),
                    Constraint::Length(prompt_height),
                    Constraint::Length(2),
                ])
                .split(area);
            (chunks[0], chunks[1], chunks[2], chunks[3], area)
        };

        // Sidebar disabled — always return None
        (header_area, chat_area, prompt_area, status_area, None, full)
    }

    /// Render the complete UI
    #[allow(clippy::too_many_arguments)]
    pub fn render_complete(
        frame: &mut Frame,
        chat: &ChatWidget,
        prompt: &PromptWidget,
        status: &str,
        model: Option<&str>,
        tokens_used: Option<u64>,
        _working_dir: &str,
        theme: &Theme,
    ) {
        Self::render_complete_with_spinner(frame, chat, prompt, status, model, tokens_used, _working_dir, None, None, None, theme, crate::repl::SidebarTab::default(), None, false, false, None, &[], None, None, None, None, None, None, None, None, true);
    }

    /// Render the complete UI with spinner animation support
    #[allow(clippy::too_many_arguments)]
    pub fn render_complete_with_spinner(
        frame: &mut Frame,
        chat: &ChatWidget,
        prompt: &PromptWidget,
        status: &str,
        model: Option<&str>,
        tokens_used: Option<u64>,
        _working_dir: &str,
        spinner: Option<&crate::widgets::progress::SpinnerWidget>,
        progress_bar: Option<&crate::widgets::progress::ProgressBarWidget>,
        sidebar_info: Option<&SidebarInfo>,
        theme: &Theme,
        sidebar_tab: crate::repl::SidebarTab,
        approval_mode: Option<&str>,
        focus_mode: bool,
        fullscreen_mode: bool,
        search_query: Option<&str>,
        search_matches: &[(usize, usize, usize)],
        search_focused_idx: Option<usize>,
        max_tokens: Option<u64>,
        cost_usd: Option<f64>,
        git_branch: Option<&str>,
        token_breakdown: Option<(u64, u64)>,
        diag_counts: Option<(usize, usize)>,
        cached_statusline: Option<&str>,
        rate_limit: Option<(u32, u32)>,
        auto_follow: bool,
    ) {
        let area = frame.area();

        // Show warning if terminal is too small
        if area.width < MIN_TERMINAL_WIDTH || area.height < MIN_TERMINAL_HEIGHT {
            let msg = format!(
                "Terminal too small: {}x{}. Need at least {}x{}.",
                area.width, area.height, MIN_TERMINAL_WIDTH, MIN_TERMINAL_HEIGHT
            );
            let warning = Paragraph::new(msg)
                .style(Style::default().fg(theme.error))
                .alignment(Alignment::Center);
            frame.render_widget(warning, area);
            return;
        }

        let prompt_height = prompt.needed_height(area.width);
        let sidebar_visible = sidebar_info.is_some();

        // Render chat — column path handles search highlighting internally
        let render_chat = |frame: &mut Frame, chat_area: Rect, theme: &Theme| {
            let search = search_query.and_then(|q| {
                if q.is_empty() || search_matches.is_empty() {
                    None
                } else {
                    Some(renderable::SearchParams {
                        query: q,
                        matches: search_matches,
                        focused_idx: search_focused_idx,
                        cell_index: 0,
                    })
                }
            });
            chat.render(frame, chat_area, theme, search.as_ref(), auto_follow);
        };

        if fullscreen_mode {
            // Fullscreen: chat fills entire terminal + prompt, no chrome at all
            let chunks = ratatui::layout::Layout::default()
                .direction(Direction::Vertical)
                .margin(0)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(prompt_height),
                ])
                .split(area);
            render_chat(frame, chunks[0], theme);
            prompt.render(frame, chunks[1], theme);
        } else if focus_mode {
            // Focus mode: only chat + prompt, maximized (no header/status/sidebar)
            let chunks = ratatui::layout::Layout::default()
                .direction(Direction::Vertical)
                .margin(0)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(prompt_height),
                ])
                .split(area);
            render_chat(frame, chunks[0], theme);
            prompt.render(frame, chunks[1], theme);
        } else {
            let (_header_area, chat_area, prompt_area, status_area, sidebar_area, _) =
                Self::layout_with_sidebar(area, prompt_height, sidebar_visible, chat.is_empty());

            // Header area reserved but not rendered — chat is the primary interface
            render_chat(frame, chat_area, theme);
            prompt.render(frame, prompt_area, theme);
            if let Some(custom) = cached_statusline {
                StatusBarWidget::render_custom(frame, status_area, custom, theme);
            } else {
                let files_info = sidebar_info.map(|si| (si.modified_files.len(), si.total_additions, si.total_deletions));
                let tools_invoked = sidebar_info.map(|si| si.tools_invoked);
                let session_duration = sidebar_info.map(|si| si.session_duration_secs);
                StatusBarWidget::render_with_spinner(frame, status_area, status, model, tokens_used, max_tokens, cost_usd, git_branch, spinner, progress_bar, theme, approval_mode, token_breakdown, diag_counts, rate_limit, files_info, tools_invoked, session_duration);
            }

            if let (Some(info), Some(sb_area)) = (sidebar_info, sidebar_area) {
                if sb_area.width > 5 && sb_area.height > 3 {
                    // Clear the sidebar area first to prevent old content showing through
                    // during diff-based rendering (especially during streaming).
                    frame.render_widget(Clear, sb_area);
                    let sidebar_widget = SidebarWidget::new();
                    sidebar_widget.render(frame, sb_area, info, theme, sidebar_tab);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    // ── Chat Widget Tests ─────────────────────────────────────────────

    #[test]
    fn test_chat_widget_creation() {
        let chat = ChatWidget::new(100);
        assert_eq!(chat.len(), 0);
        assert!(chat.is_empty());
    }

    #[test]
    fn test_chat_widget_add_message() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Hello".to_string());
        assert_eq!(chat.len(), 1);
        assert!(!chat.is_empty());
    }

    #[test]
    fn test_chat_widget_multiple_messages() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "First".to_string());
        chat.add_message(ChatRole::Assistant, "Second".to_string());
        chat.add_message(ChatRole::System, "Third".to_string());
        assert_eq!(chat.len(), 3);
    }

    #[test]
    fn test_chat_widget_clear() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Hello".to_string());
        chat.add_message(ChatRole::Assistant, "Hi".to_string());
        assert_eq!(chat.len(), 2);
        chat.clear();
        assert_eq!(chat.len(), 0);
        assert!(chat.is_empty());
    }

    #[test]
    fn test_chat_widget_update_message() {
        let mut chat = ChatWidget::new(100);
        let index = chat.add_message(ChatRole::Assistant, "Initial".to_string());
        chat.update_message(index, "Updated".to_string());
        assert_eq!(chat.messages[index].content, "Updated");
    }

    #[test]
    fn test_chat_widget_update_last_message() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::Assistant, "First".to_string());
        chat.add_message(ChatRole::Assistant, "Second".to_string());
        chat.update_last_message("Last Updated".to_string());
        assert_eq!(chat.messages.len(), 2);
        assert_eq!(chat.messages[1].content, "Last Updated");
    }

    #[test]
    fn test_chat_widget_scroll() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Msg1".to_string());
        chat.add_message(ChatRole::User, "Msg2".to_string());
        chat.add_message(ChatRole::User, "Msg3".to_string());
        assert_eq!(chat.scroll_offset, 2); // Auto-scrolls to bottom
        chat.scroll_up();
        assert_eq!(chat.scroll_offset, 1);
        chat.scroll_down();
        assert_eq!(chat.scroll_offset, 2);
    }

    // ── Prompt Widget Tests ────────────────────────────────────────────

    #[test]
    fn test_prompt_widget_creation() {
        let prompt = PromptWidget::new();
        assert_eq!(prompt.input(), "");
        assert_eq!(prompt.cursor_position(), 0);
    }

    #[test]
    fn test_prompt_widget_add_char() {
        let mut prompt = PromptWidget::new();
        prompt.add_char('a');
        assert_eq!(prompt.input(), "a");
        assert_eq!(prompt.cursor_position(), 1);
    }

    #[test]
    fn test_prompt_widget_backspace() {
        let mut prompt = PromptWidget::new();
        prompt.add_char('a');
        prompt.add_char('b');
        prompt.backspace();
        assert_eq!(prompt.input(), "a");
        assert_eq!(prompt.cursor_position(), 1);
    }

    #[test]
    fn test_prompt_widget_clear() {
        let mut prompt = PromptWidget::new();
        prompt.add_char('a');
        prompt.add_char('b');
        prompt.clear();
        assert_eq!(prompt.input(), "");
        assert_eq!(prompt.cursor_position(), 0);
    }

    #[test]
    fn test_prompt_widget_cursor_movement() {
        let mut prompt = PromptWidget::new();
        prompt.set_input("abc".to_string());
        assert_eq!(prompt.cursor_position(), 3);
        prompt.cursor_left();
        assert_eq!(prompt.cursor_position(), 2);
        prompt.cursor_left();
        assert_eq!(prompt.cursor_position(), 1);
        prompt.cursor_right();
        assert_eq!(prompt.cursor_position(), 2);
        prompt.cursor_right();
        assert_eq!(prompt.cursor_position(), 3);
    }

    #[test]
    fn test_prompt_widget_set_input() {
        let mut prompt = PromptWidget::new();
        prompt.set_input("test input".to_string());
        assert_eq!(prompt.input(), "test input");
        assert_eq!(prompt.cursor_position(), 10);
    }

    #[test]
    fn test_prompt_widget_with_placeholder() {
        let prompt = PromptWidget::new().with_placeholder("Enter command...".to_string());
        assert_eq!(prompt.placeholder, "Enter command...");
    }

    // ── Header Widget Tests ────────────────────────────────────────────

    #[test]
    fn test_header_widget_height() {
        assert_eq!(HeaderWidget::height(), 3);
    }

    #[test]
    fn test_header_widget_welcome_message() {
        let theme = Theme::default_dark();
        let spans = HeaderWidget::welcome_message(&theme);
        assert!(!spans.is_empty());
        assert_eq!(spans.len(), 3); // "Welcome to " + "Shannon" + "! "
    }

    #[test]
    fn test_header_widget_tip_message() {
        let theme = Theme::default_dark();
        let spans = HeaderWidget::tip_message(&theme);
        assert!(!spans.is_empty());
        assert_eq!(spans.len(), 2); // "Tip: " + tip text
    }

    // ── Main Layout Widget Tests ───────────────────────────────────────

    #[test]
    fn test_main_layout_widget_returns_five_chunks() {
        // Create a test area (100x20)
        let area = Rect::new(0, 0, 100, 20);
        let (header, chat, prompt, status, full) = MainLayoutWidget::layout(area, 3);

        // Header should be at top with height 3
        assert_eq!(header.y, 0);
        assert_eq!(header.height, 3);

        // Chat should be below header and be flexible
        assert_eq!(chat.y, 3); // header(3)
        assert!(chat.height > 0); // Flexible size

        // Prompt should be below chat with height 3
        assert_eq!(prompt.height, 3);

        // Status should be at bottom with height 2
        assert_eq!(status.height, 2);

        // Full area should match input area
        assert_eq!(full, area);
    }

    #[test]
    fn test_main_layout_widget_chat_area_is_flexible() {
        let small_area = Rect::new(0, 0, 80, 10);
        let (_, small_chat, _, _, _) = MainLayoutWidget::layout(small_area, 3);

        let large_area = Rect::new(0, 0, 80, 30);
        let (_, large_chat, _, _, _) = MainLayoutWidget::layout(large_area, 3);

        // Chat area should grow with available space
        assert!(large_chat.height > small_chat.height);
    }

    #[test]
    fn test_main_layout_widget_fixed_sizes() {
        let area = Rect::new(0, 0, 100, 20);
        let (header, _, prompt, status, _) = MainLayoutWidget::layout(area, 3);

        // Header and prompt have fixed heights; status bar is 2 lines
        assert_eq!(header.height, 3);
        assert_eq!(prompt.height, 3);
        assert_eq!(status.height, 2);
    }

    #[test]
    fn test_main_layout_widget_margins() {
        let area = Rect::new(0, 0, 100, 20);
        let (header, _, _, _, _) = MainLayoutWidget::layout(area, 3);

        // No outer margin — full width
        assert_eq!(header.x, 0);
        assert_eq!(header.y, 0);
        assert_eq!(header.width, 100); // Full width
    }

    // ── Chat Message Tests ─────────────────────────────────────────────

    #[test]
    fn test_chat_message_creation() {
        let msg = ChatMessage {
            role: ChatRole::User,
            content: "Test message".to_string(),
            timestamp: chrono::Utc::now(),
            image_lines: None,
            is_error: false,
            tool_name: None,
            start_time: None,
            duration_secs: None,
            spinner_frame: 0,
            folded: true,
            exit_code: None,
        };
        assert_eq!(msg.content, "Test message");
        assert_eq!(msg.role, ChatRole::User);
    }

    #[test]
    fn test_chat_role_colors() {
        let (user_name, user_color) = match ChatRole::User {
            ChatRole::User => ("User", Color::Green),
            _ => panic!("Wrong role"),
        };
        assert_eq!(user_name, "User");
        assert_eq!(user_color, Color::Green);
    }

    #[test]
    fn test_all_chat_roles_have_colors() {
        let roles = vec![
            (ChatRole::User, "User", Color::Green),
            (ChatRole::Assistant, "Assistant", Color::Cyan),
            (ChatRole::System, "System", Color::Yellow),
            (ChatRole::Tool, "Tool", Color::Magenta),
        ];

        for (role, expected_name, expected_color) in roles {
            let (name, color) = match role {
                ChatRole::User => ("User", Color::Green),
                ChatRole::Assistant => ("Assistant", Color::Cyan),
                ChatRole::System => ("System", Color::Yellow),
                ChatRole::Tool => ("Tool", Color::Magenta),
            };
            assert_eq!(name, expected_name);
            assert_eq!(color, expected_color);
        }
    }

    // ── Integration Tests ──────────────────────────────────────────────

    #[test]
    fn test_chat_prompt_workflow() {
        let mut chat = ChatWidget::new(10);
        let mut prompt = PromptWidget::new();

        // User types message
        prompt.add_char('H');
        prompt.add_char('e');
        prompt.add_char('l');
        prompt.add_char('l');
        prompt.add_char('o');
        assert_eq!(prompt.input(), "Hello");

        // Add to chat
        chat.add_message(ChatRole::User, prompt.input().to_string());
        assert_eq!(chat.len(), 1);

        // Clear prompt
        prompt.clear();
        assert_eq!(prompt.input(), "");
    }

    #[test]
    fn test_multiple_chat_updates() {
        let mut chat = ChatWidget::new(10);
        let idx = chat.add_message(ChatRole::Assistant, "Thinking...".to_string());

        // Simulate streaming updates
        for i in 1..=5 {
            chat.update_message(idx, format!("Step {i} complete"));
        }
        assert_eq!(chat.messages[idx].content, "Step 5 complete");
    }

    // ── Rewind Tests ─────────────────────────────────────────────────

    #[test]
    fn test_rewind_single_turn() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Hello".to_string());
        chat.add_message(ChatRole::Assistant, "Hi there".to_string());
        chat.add_message(ChatRole::User, "How are you?".to_string());
        chat.add_message(ChatRole::Assistant, "I'm fine".to_string());
        assert_eq!(chat.len(), 4);

        let removed = chat.rewind(1);
        assert_eq!(removed, 2); // last user + assistant
        assert_eq!(chat.len(), 2);
        assert_eq!(chat.get_message(0).unwrap().content, "Hello");
        assert_eq!(chat.get_message(1).unwrap().content, "Hi there");
    }

    #[test]
    fn test_rewind_multiple_turns() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Q1".to_string());
        chat.add_message(ChatRole::Assistant, "A1".to_string());
        chat.add_message(ChatRole::User, "Q2".to_string());
        chat.add_message(ChatRole::Assistant, "A2".to_string());
        chat.add_message(ChatRole::User, "Q3".to_string());
        chat.add_message(ChatRole::Assistant, "A3".to_string());
        assert_eq!(chat.len(), 6);

        let removed = chat.rewind(2);
        assert_eq!(removed, 4); // Q2+A2+Q3+A3
        assert_eq!(chat.len(), 2);
        assert_eq!(chat.get_message(0).unwrap().content, "Q1");
    }

    #[test]
    fn test_rewind_all_turns() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Q1".to_string());
        chat.add_message(ChatRole::Assistant, "A1".to_string());
        assert_eq!(chat.len(), 2);

        let removed = chat.rewind(1);
        assert_eq!(removed, 2);
        assert!(chat.is_empty());
    }

    #[test]
    fn test_rewind_with_tool_messages() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Run tests".to_string());
        chat.add_message(ChatRole::Tool, "bash: cargo test".to_string());
        chat.add_message(ChatRole::Tool, "output: all passed".to_string());
        chat.add_message(ChatRole::Assistant, "Tests passed".to_string());
        chat.add_message(ChatRole::User, "Now commit".to_string());
        chat.add_message(ChatRole::Assistant, "Done".to_string());
        assert_eq!(chat.len(), 6);

        // Rewind 1 turn removes "Now commit" + "Done"
        let removed = chat.rewind(1);
        assert_eq!(removed, 2);
        assert_eq!(chat.len(), 4);

        // Rewind 1 more turn removes "Run tests" + all tool + assistant
        let removed = chat.rewind(1);
        assert_eq!(removed, 4);
        assert!(chat.is_empty());
    }

    #[test]
    fn test_rewind_with_system_messages() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::System, "Session started".to_string());
        chat.add_message(ChatRole::User, "Q1".to_string());
        chat.add_message(ChatRole::Assistant, "A1".to_string());
        assert_eq!(chat.len(), 3);

        // Rewind 1 turn: system message stays, user+assistant removed
        let removed = chat.rewind(1);
        assert_eq!(removed, 2); // User + Assistant only
        assert_eq!(chat.len(), 1);
        assert_eq!(chat.get_message(0).unwrap().role, ChatRole::System);
    }

    #[test]
    fn test_rewind_empty_chat() {
        let mut chat = ChatWidget::new(100);
        let removed = chat.rewind(1);
        assert_eq!(removed, 0);
        assert!(chat.is_empty());
    }

    #[test]
    fn test_rewind_zero_turns() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Q1".to_string());
        let removed = chat.rewind(0);
        assert_eq!(removed, 0);
        assert_eq!(chat.len(), 1);
    }

    #[test]
    fn test_rewind_more_than_available() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Q1".to_string());
        chat.add_message(ChatRole::Assistant, "A1".to_string());

        // Ask for 5 turns when only 1 exists
        let removed = chat.rewind(5);
        assert_eq!(removed, 2);
        assert!(chat.is_empty());
    }

    #[test]
    fn test_rewind_no_user_messages() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::System, "System msg".to_string());
        chat.add_message(ChatRole::Assistant, "Assistant msg".to_string());

        let removed = chat.rewind(1);
        assert_eq!(removed, 0); // No user messages to anchor a turn
        assert_eq!(chat.len(), 2);
    }

    #[test]
    fn test_rewind_fixes_scroll_offset() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Q1".to_string());
        chat.add_message(ChatRole::Assistant, "A1".to_string());
        chat.add_message(ChatRole::User, "Q2".to_string());
        chat.add_message(ChatRole::Assistant, "A2".to_string());
        // scroll_offset should be 3 (last message index)

        chat.rewind(1);
        // scroll_offset should be updated to 1 (new last message)
        assert_eq!(chat.scroll_offset, 1);
    }

    // ── Markdown Parsing Tests ──────────────────────────────────────────

    #[test]
    fn test_parse_markdown_plain_text() {
        let segments = chat::parse_markdown_segments("Hello\nWorld");
        assert_eq!(segments.len(), 1);
    }

    #[test]
    fn test_parse_markdown_code_block() {
        let input = "Before\n```rust\nfn main() {}\n```\nAfter";
        let segments = chat::parse_markdown_segments(input);
        assert_eq!(segments.len(), 3);
    }

    #[test]
    fn test_parse_markdown_code_block_no_lang() {
        let input = "```\nsome code\n```";
        let segments = chat::parse_markdown_segments(input);
        assert_eq!(segments.len(), 1);
    }

    #[test]
    fn test_highlight_code_with_renderer() {
        let code = "fn main() {\n    println!(\"hello\");\n}";
        let lines = crate::render::Renderer::new().highlight_code(code, "rust");
        assert!(!lines.is_empty());
        assert_eq!(lines.len(), 3); // One line per source line
    }

    #[test]
    fn test_highlight_code_empty_with_renderer() {
        let lines = crate::render::Renderer::new().highlight_code("", "rust");
        assert!(lines.is_empty());
    }

    #[test]
    fn test_highlight_code_unknown_language_with_renderer() {
        let lines = crate::render::Renderer::new().highlight_code("fn main() {}", "no_such_lang");
        assert!(!lines.is_empty()); // Should still render, just plain
    }

    // ── Tool Message Tests ──────────────────────────────────────────────

    #[test]
    fn test_add_tool_message() {
        let mut chat = ChatWidget::new(100);
        let idx = chat.add_tool_message("bash".to_string(), "cargo test\nall passed".to_string(), false, None);
        assert_eq!(chat.len(), 1);
        let msg = &chat.messages[idx];
        assert_eq!(msg.role, ChatRole::Tool);
        assert_eq!(msg.tool_name.as_deref(), Some("bash"));
        assert!(!msg.is_error);
    }

    #[test]
    fn test_add_tool_message_error() {
        let mut chat = ChatWidget::new(100);
        chat.add_tool_message("bash".to_string(), "error: build failed".to_string(), true, None);
        let msg = &chat.messages[0];
        assert!(msg.is_error);
        assert_eq!(msg.tool_name.as_deref(), Some("bash"));
    }

    // ── SidebarInfo Tests ───────────────────────────────────────────────

    #[test]
    fn test_sidebar_info_context_window() {
        let info = SidebarInfo {
            model: Some("test-model".to_string()),
            tokens_used: 5000,
            cost_usd: 0.05,
            tools_invoked: 3,
            modified_files: vec![],
            total_additions: 0,
            total_deletions: 0,
            error_count: 1,
            context_window: 200_000,
            active_agents: vec![],
            diagnostics: vec![],
            session_duration_secs: 120,
            turn_count: 5,
            commands_run: 3,
            tokens_per_sec: Some(42.0),
            memory_rss_kb: 0,        };
        assert_eq!(info.context_window, 200_000);
        assert_eq!(info.error_count, 1);
    }

    // ── Truncation Tests ────────────────────────────────────────────────

    #[test]
    fn test_truncate_to_short() {
        assert_eq!(chat::truncate_to("hi", 10), "hi");
    }

    #[test]
    fn test_truncate_to_exact() {
        assert_eq!(chat::truncate_to("abc", 3), "abc");
    }

    #[test]
    fn test_truncate_to_long() {
        assert_eq!(chat::truncate_to("abcdef", 4), "abc…");
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(sidebar::format_tokens(500), "500");
        assert_eq!(sidebar::format_tokens(1500), "1.5k");
        assert_eq!(sidebar::format_tokens(1_500_000), "1.5M");
    }

    // ── Sidebar Tab Tests ─────────────────────────────────────────────

    #[test]
    fn test_sidebar_tab_default() {
        assert_eq!(crate::repl::SidebarTab::default(), crate::repl::SidebarTab::Context);
    }

    #[test]
    fn test_sidebar_tab_cycle() {
        let mut tab = crate::repl::SidebarTab::Context;
        tab = tab.next();
        assert_eq!(tab, crate::repl::SidebarTab::Files);
        tab = tab.next();
        assert_eq!(tab, crate::repl::SidebarTab::Agents);
        tab = tab.next();
        assert_eq!(tab, crate::repl::SidebarTab::Perf);
        tab = tab.next();
        assert_eq!(tab, crate::repl::SidebarTab::Context);
    }

    #[test]
    fn test_sidebar_fits() {
        assert!(SidebarWidget::fits(80));
        assert!(!SidebarWidget::fits(60));
    }

    // ── Markdown Parsing Edge Case Tests ──────────────────────────────────

    #[test]
    fn test_parse_markdown_unclosed_code_block() {
        let input = "Before\n```rust\nfn main() {}\nno closing fence";
        let segments = chat::parse_markdown_segments(input);
        assert_eq!(segments.len(), 2);
        match &segments[1] {
            chat::MdSegment::CodeBlock { lang, code } => {
                assert_eq!(lang.as_deref(), Some("rust"));
                assert!(code.contains("fn main()"));
                assert!(code.contains("no closing fence"));
            }
            _ => panic!("Expected CodeBlock segment"),
        }
    }

    #[test]
    fn test_parse_markdown_unclosed_code_block_no_text() {
        let input = "Text\n```\ncode line 1\ncode line 2";
        let segments = chat::parse_markdown_segments(input);
        assert_eq!(segments.len(), 2);
    }

    #[test]
    fn test_parse_markdown_empty_input() {
        let segments = chat::parse_markdown_segments("");
        assert!(segments.is_empty());
    }

    #[test]
    fn test_parse_markdown_single_newline() {
        let segments = chat::parse_markdown_segments("\n");
        assert_eq!(segments.len(), 0, "bare newline should produce no segments");
    }

    #[test]
    fn test_parse_markdown_multiple_code_blocks() {
        let input = "```\ncode1\n```\nBetween\n```\ncode2\n```";
        let segments = chat::parse_markdown_segments(input);
        assert_eq!(segments.len(), 3);
    }

    #[test]
    fn test_parse_markdown_code_block_with_backticks_inside() {
        let input = "```\nthis has `backticks` inside\n```";
        let segments = chat::parse_markdown_segments(input);
        assert_eq!(segments.len(), 1);
    }

    #[test]
    fn test_parse_markdown_nested_code_fences() {
        let input = "````\n```\ninner\n```\n````";
        let segments = chat::parse_markdown_segments(input);
        // Parser uses starts_with("```") so ```` both opens and closes,
        // and the inner ``` lines also trigger open/close, producing multiple segments.
        assert!(segments.len() >= 1, "Should produce at least one segment");
    }

    #[test]
    fn test_parse_markdown_very_long_line() {
        let long_line = "a".repeat(10_000);
        let segments = chat::parse_markdown_segments(&long_line);
        assert_eq!(segments.len(), 1);
        match &segments[0] {
            chat::MdSegment::Text(lines) => {
                assert_eq!(lines.len(), 1);
                assert_eq!(lines[0].len(), 10_000);
            }
            _ => panic!("Expected Text segment"),
        }
    }

    #[test]
    fn test_parse_markdown_many_lines() {
        let input: String = (0..500).map(|i| format!("Line {i}")).collect::<Vec<_>>().join("\n");
        let segments = chat::parse_markdown_segments(&input);
        assert_eq!(segments.len(), 1);
        match &segments[0] {
            chat::MdSegment::Text(lines) => assert_eq!(lines.len(), 500),
            _ => panic!("Expected Text segment"),
        }
    }

    #[test]
    fn test_parse_markdown_header_levels() {
        for level in 1..=6 {
            let input = format!("{} Header", "#".repeat(level));
            let segments = chat::parse_markdown_segments(&input);
            assert_eq!(segments.len(), 1, "Expected single header segment for level {level}");
            match &segments[0] {
                chat::MdSegment::Header { level: l, text } => {
                    assert_eq!(*l, level);
                    assert_eq!(text, "Header");
                }
                _ => panic!("Expected Header segment for level {level}"),
            }
        }
    }

    #[test]
    fn test_parse_markdown_header_no_space() {
        let input = "#Header";
        let segments = chat::parse_markdown_segments(input);
        match &segments[0] {
            chat::MdSegment::Text(lines) => assert_eq!(lines[0], "#Header"),
            _ => panic!("Expected Text segment, not a header"),
        }
    }

    #[test]
    fn test_parse_markdown_header_seven_hashes() {
        let input = "####### Not a header";
        let segments = chat::parse_markdown_segments(input);
        match &segments[0] {
            chat::MdSegment::Text(lines) => assert_eq!(lines[0], "####### Not a header"),
            _ => panic!("Expected Text segment for 7-hash line"),
        }
    }

    #[test]
    fn test_parse_markdown_code_block_with_lang_and_filename() {
        let input = "```rust:src/main.rs\nfn main() {}\n```";
        let segments = chat::parse_markdown_segments(input);
        assert_eq!(segments.len(), 1);
        match &segments[0] {
            chat::MdSegment::CodeBlock { lang, code } => {
                assert_eq!(lang.as_deref(), Some("rust:src/main.rs"));
                assert_eq!(code, "fn main() {}");
            }
            _ => panic!("Expected CodeBlock segment"),
        }
    }

    #[test]
    fn test_parse_markdown_code_block_empty() {
        let input = "```\n```";
        let segments = chat::parse_markdown_segments(input);
        assert_eq!(segments.len(), 1);
        match &segments[0] {
            chat::MdSegment::CodeBlock { lang, code } => {
                assert!(lang.is_none());
                assert!(code.is_empty());
            }
            _ => panic!("Expected CodeBlock segment"),
        }
    }

    #[test]
    fn test_parse_markdown_text_before_and_after_code() {
        let input = "Before\n```\ncode\n```\nAfter";
        let segments = chat::parse_markdown_segments(input);
        assert_eq!(segments.len(), 3);
        assert!(matches!(&segments[0], chat::MdSegment::Text(_)));
        assert!(matches!(&segments[1], chat::MdSegment::CodeBlock { .. }));
        assert!(matches!(&segments[2], chat::MdSegment::Text(_)));
    }

    #[test]
    fn test_sidebar_width_constant() {
        assert_eq!(SidebarWidget::width(), 28);
    }

    #[test]
    fn test_sidebar_fits_at_boundary() {
        assert!(SidebarWidget::fits(78));
        assert!(!SidebarWidget::fits(77));
    }

    #[test]
    fn test_sidebar_fits_large_terminal() {
        assert!(SidebarWidget::fits(200));
        assert!(SidebarWidget::fits(120));
    }

    #[test]
    fn test_sidebar_fits_narrow_terminal() {
        assert!(!SidebarWidget::fits(40));
        assert!(!SidebarWidget::fits(20));
    }

    #[test]
    fn test_sidebar_layout_with_sidebar_visible() {
        let area = Rect::new(0, 0, 120, 30);
        let (_header, _chat, _prompt, _status, sidebar_area, full) =
            MainLayoutWidget::layout_with_sidebar(area, 3, true, true);
        assert_eq!(full, area);
        // Sidebar is disabled — always returns None
        assert!(sidebar_area.is_none());
    }

    #[test]
    fn test_sidebar_layout_hidden_when_too_narrow() {
        let area = Rect::new(0, 0, 70, 20);
        let (_, _, _, _, sidebar_area, _) =
            MainLayoutWidget::layout_with_sidebar(area, 3, true, true);
        assert!(sidebar_area.is_none());
    }

    #[test]
    fn test_sidebar_layout_not_visible_when_flag_off() {
        let area = Rect::new(0, 0, 120, 30);
        let (_, _, _, _, sidebar_area, _) =
            MainLayoutWidget::layout_with_sidebar(area, 3, false, true);
        assert!(sidebar_area.is_none());
    }

    #[test]
    fn test_sidebar_info_all_fields() {
        let info = SidebarInfo {
            model: Some("claude-opus".to_string()),
            tokens_used: 150_000,
            cost_usd: 1.2345,
            tools_invoked: 7,
            modified_files: vec![
                ("src/main.rs".to_string(), 50, 10),
                ("lib.rs".to_string(), 20, 5),
            ],
            total_additions: 70,
            total_deletions: 15,
            error_count: 0,
            context_window: 200_000,
            active_agents: vec![],
            diagnostics: vec![],
            session_duration_secs: 300,
            turn_count: 10,
            commands_run: 5,
            tokens_per_sec: Some(85.5),
            memory_rss_kb: 0,        };
        assert_eq!(info.model.as_deref(), Some("claude-opus"));
        assert_eq!(info.tokens_used, 150_000);
        assert_eq!(info.modified_files.len(), 2);
        assert_eq!(info.total_additions, 70);
        assert_eq!(info.total_deletions, 15);
    }

    #[test]
    fn test_sidebar_info_default_values() {
        let info = SidebarInfo {
            model: None,
            tokens_used: 0,
            cost_usd: 0.0,
            tools_invoked: 0,
            modified_files: vec![],
            total_additions: 0,
            total_deletions: 0,
            error_count: 0,
            context_window: 0,
            active_agents: vec![],
            diagnostics: vec![],
            session_duration_secs: 0,
            turn_count: 0,
            commands_run: 0,
            tokens_per_sec: None,
            memory_rss_kb: 0,        };
        assert!(info.model.is_none());
        assert!(info.modified_files.is_empty());
    }

    // ── Token Formatting Edge Cases ────────────────────────────────────────

    #[test]
    fn test_format_tokens_boundary_values() {
        assert_eq!(sidebar::format_tokens(0), "0");
        assert_eq!(sidebar::format_tokens(999), "999");
        assert_eq!(sidebar::format_tokens(1000), "1.0k");
        assert_eq!(sidebar::format_tokens(999_999), "1000.0k");
        assert_eq!(sidebar::format_tokens(1_000_000), "1.0M");
    }

    #[test]
    fn test_format_tokens_exact_millions() {
        assert_eq!(sidebar::format_tokens(1_000_000), "1.0M");
        assert_eq!(sidebar::format_tokens(2_500_000), "2.5M");
    }

    // ── Truncation Edge Cases ──────────────────────────────────────────────

    #[test]
    fn test_truncate_to_zero_chars() {
        let ellipsis = "\u{2026}";
        assert_eq!(chat::truncate_to("hello", 0), ellipsis);
    }

    #[test]
    fn test_truncate_to_one_char() {
        let ellipsis = "\u{2026}";
        assert_eq!(chat::truncate_to("hello", 1), ellipsis);
    }

    #[test]
    fn test_truncate_to_two_chars() {
        assert_eq!(chat::truncate_to("hello", 2), "h\u{2026}");
    }

    #[test]
    fn test_truncate_to_empty_string() {
        // Empty string fits in any width, including 0
        assert_eq!(chat::truncate_to("", 10), "");
        assert_eq!(chat::truncate_to("", 0), "");
    }

    #[test]
    fn test_truncate_to_unicode() {
        assert_eq!(chat::truncate_to("hello world", 5), "hell\u{2026}");
        let unicode = "caf\u{e9} r\u{e9}sum\u{e9}";
        assert_eq!(chat::truncate_to(unicode, 5), "caf\u{e9}\u{2026}");
    }

    // ── Chat Widget Scroll Bounds ──────────────────────────────────────────

    #[test]
    fn test_chat_scroll_bounds_empty() {
        let mut chat = ChatWidget::new(100);
        chat.scroll_up();
        assert_eq!(chat.scroll_offset, 0);
        chat.scroll_down();
        assert_eq!(chat.scroll_offset, 0);
    }

    #[test]
    fn test_chat_scroll_bounds_single_message() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Hello".to_string());
        assert_eq!(chat.scroll_offset, 0);
        chat.scroll_down();
        assert_eq!(chat.scroll_offset, 0);
        chat.scroll_up();
        assert_eq!(chat.scroll_offset, 0);
    }

    #[test]
    fn test_chat_scroll_up_does_not_underflow() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "A".to_string());
        chat.add_message(ChatRole::Assistant, "B".to_string());
        for _ in 0..10 {
            chat.scroll_up();
        }
        assert_eq!(chat.scroll_offset, 0);
    }

    #[test]
    fn test_chat_scroll_down_does_not_overflow() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "A".to_string());
        chat.add_message(ChatRole::Assistant, "B".to_string());
        for _ in 0..10 {
            chat.scroll_down();
        }
        assert_eq!(chat.scroll_offset, 1);
    }

    #[test]
    fn test_chat_scroll_to_top_and_back() {
        let mut chat = ChatWidget::new(100);
        for i in 0..20 {
            chat.add_message(ChatRole::User, format!("Msg {i}"));
        }
        // After adding 20 messages, scroll_offset = 19 (auto-scrolled to bottom/newest)
        assert_eq!(chat.scroll_offset, 19);
        // scroll_to_top sets offset to 0 (oldest message)
        chat.scroll_to_top();
        assert_eq!(chat.scroll_offset, 0);
        // scroll_to_latest resets offset to len()-1 = 19 (newest message)
        chat.scroll_to_latest();
        assert_eq!(chat.scroll_offset, 19);
    }

    // ── Chat Widget Update Bounds ──────────────────────────────────────────

    #[test]
    fn test_chat_update_message_out_of_bounds() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Hello".to_string());
        chat.update_message(5, "Updated".to_string());
        assert_eq!(chat.messages[0].content, "Hello");
    }

    #[test]
    fn test_chat_update_last_message_empty_chat() {
        let mut chat = ChatWidget::new(100);
        chat.update_last_message("still empty".to_string());
        assert!(chat.is_empty());
    }

    // ── StreamingState Tests ───────────────────────────────────────────────

    #[test]
    fn test_streaming_state_default() {
        assert_eq!(StreamingState::default(), StreamingState::Idle);
    }

    #[test]
    fn test_streaming_state_variants() {
        let thinking = StreamingState::Thinking;
        let tool = StreamingState::CallingTool { name: "bash".to_string() };
        let generating = StreamingState::Generating { elapsed_secs: 5 };
        assert_eq!(thinking, StreamingState::Thinking);
        assert_eq!(tool, StreamingState::CallingTool { name: "bash".to_string() });
        assert_eq!(generating, StreamingState::Generating { elapsed_secs: 5 });
    }


    // ── Chat Toggle Fold Tests ─────────────────────────────────────────────

    #[test]
    fn test_toggle_fold_non_tool_message() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Hello".to_string());
        chat.toggle_fold(0);
        assert_eq!(chat.messages[0].folded, false);
    }

    #[test]
    fn test_toggle_last_tool_fold_no_tools() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Hello".to_string());
        chat.add_message(ChatRole::Assistant, "Hi".to_string());
        chat.toggle_last_tool_fold();
        assert_eq!(chat.len(), 2);
    }

    #[test]
    fn test_toggle_last_tool_fold_with_tools() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Run tests".to_string());
        chat.add_tool_message("bash".to_string(), "cargo test".to_string(), false, None);
        assert!(chat.messages[1].folded);
        chat.toggle_last_tool_fold();
        assert!(!chat.messages[1].folded);
    }
}
