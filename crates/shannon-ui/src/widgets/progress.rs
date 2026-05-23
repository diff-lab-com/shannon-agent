//! Progress display widgets for Shannon UI
//!
//! Provides progress bars with various styles and animations.
//!
//! # Widgets
//!
//! - **ProgressBarWidget**: Single progress bar with fill/empty styles, optional title,
//!   percentage display, and animation support.
//!
//! - **SpinnerWidget**: Animated braille-dot spinner for indeterminate progress
//!   (tool execution, loading states). Supports static mode (●) for paused states.
//!
//! - **MultiProgressWidget**: Shows multiple labeled progress bars simultaneously.
//!   Used for parallel tool execution — each running tool gets its own bar with
//!   label, fill percentage, and color. Bars can be added/updated dynamically
//!   as tools start and complete.

use crate::theme::Theme;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

/// Unicode block characters for smooth progress display
const BLOCKS: [&str; 9] = [" ", "▏", "▎", "█", "▌", "▋", "▊", "▉", "█"];

/// Progress bar widget
#[derive(Debug, Clone)]
pub struct ProgressBarWidget {
    title: Option<String>,
    progress: f64,
    width: Option<u16>,
    fill_style: Style,
    empty_style: Style,
    show_percentage: bool,
    animated: bool,
    animation_frame: usize,
}

impl ProgressBarWidget {
    /// Create a new progress bar
    pub fn new() -> Self {
        Self {
            title: None,
            progress: 0.0,
            width: None,
            fill_style: Style::default(),
            empty_style: Style::default(),
            show_percentage: true,
            animated: false,
            animation_frame: 0,
        }
    }

    /// Set the title
    pub fn with_title(mut self, title: String) -> Self {
        self.title = Some(title);
        self
    }

    /// Set the progress (0.0 to 1.0)
    pub fn with_progress(mut self, progress: f64) -> Self {
        self.progress = progress.clamp(0.0, 1.0);
        self
    }

    /// Set the width in characters
    pub fn with_width(mut self, width: u16) -> Self {
        self.width = Some(width);
        self
    }

    /// Set the fill style
    pub fn with_fill_style(mut self, style: Style) -> Self {
        self.fill_style = style;
        self
    }

    /// Set the empty style
    pub fn with_empty_style(mut self, style: Style) -> Self {
        self.empty_style = style;
        self
    }

    /// Show or hide percentage
    pub fn with_percentage(mut self, show: bool) -> Self {
        self.show_percentage = show;
        self
    }

    /// Enable animation
    pub fn with_animation(mut self, animated: bool) -> Self {
        self.animated = animated;
        self
    }

    /// Advance animation frame
    pub fn tick(&mut self) {
        if self.animated {
            self.animation_frame = (self.animation_frame + 1) % 4;
        }
    }

    /// Get progress as percentage
    pub fn percentage(&self) -> f64 {
        self.progress * 100.0
    }

    /// Get current progress value (0.0 to 1.0)
    pub fn progress(&self) -> f64 {
        self.progress
    }

    /// Set progress value (0.0 to 1.0)
    pub fn set_progress(&mut self, progress: f64) {
        self.progress = progress.clamp(0.0, 1.0);
    }

    /// Set the title
    pub fn set_title(&mut self, title: String) {
        self.title = Some(title);
    }

    /// Render the progress bar
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let width = self.width.unwrap_or(area.width.saturating_sub(4)) as usize;
        let filled = (self.progress * width as f64) as usize;
        let _empty = width.saturating_sub(filled);

        let mut content = Vec::new();

        // Title line
        if let Some(ref title) = self.title {
            content.push(Line::from(vec![
                Span::styled(title, Style::default().fg(theme.text)),
                Span::raw(" "),
                Span::styled(
                    format!("{:.1}%", self.percentage()),
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            content.push(Line::from(""));
        }

        // Progress bar line
        let mut bar_spans = Vec::new();

        // Animated shimmer effect
        let shimmer_offset = if self.animated {
            (self.animation_frame as f64 * width as f64 * 0.1) as usize
        } else {
            0
        };

        for i in 0..width {
            let is_filled = i < filled;
            let relative_pos = if i >= shimmer_offset {
                i - shimmer_offset
            } else {
                width - (shimmer_offset - i)
            };

            let block_char = if self.animated && is_filled {
                // Shimmer effect with different block characters
                BLOCKS[(relative_pos % 4 + 4) % 9]
            } else if is_filled {
                "█"
            } else {
                "░"
            };

            let style = if is_filled {
                Style::default().fg(theme.success)
            } else {
                Style::default().fg(theme.text_dim)
            };

            bar_spans.push(Span::styled(block_char, style));
        }

        content.push(Line::from(bar_spans));

        // Percentage line (if no title)
        if self.show_percentage && self.title.is_none() {
            content.push(Line::from(vec![Span::styled(
                format!("{:.1}%", self.percentage()),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )]));
        }

        let paragraph = Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.accent)),
            )
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }
}

impl Default for ProgressBarWidget {
    fn default() -> Self {
        Self::new()
    }
}

/// Animation phase for the spinner — each phase uses distinct frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpinnerPhase {
    /// Model is thinking (no tokens received yet)
    Thinking,
    /// Tokens are streaming in
    Streaming,
    /// A tool is executing
    Tool,
    /// Default/idle processing
    Default,
}

/// Spinner widget for indeterminate progress
#[derive(Debug, Clone)]
pub struct SpinnerWidget {
    frames: Vec<&'static str>,
    current_frame: usize,
    message: Option<String>,
    phase: SpinnerPhase,
    /// When true, skip animation and use static indicator
    static_mode: bool,
}

/// Braille dots — smooth rotation (default)
const FRAMES_DEFAULT: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Full braille circle — mesmerizing spin for thinking
const FRAMES_THINKING: &[&str] = &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"];

/// Quarter-circle rotation for tool execution
const FRAMES_TOOL: &[&str] = &["◐", "◓", "◑", "◒"];

/// Arrow wave for streaming
const FRAMES_STREAMING: &[&str] = &["⠁⠉⠙", "⠉⠙⠹", "⠙⠹⠸", "⠹⠸⠼", "⠸⠼⠴", "⠼⠴⠦"];

impl SpinnerWidget {
    /// Create a new spinner
    pub fn new() -> Self {
        Self {
            frames: FRAMES_DEFAULT.to_vec(),
            current_frame: 0,
            message: None,
            phase: SpinnerPhase::Default,
            static_mode: false,
        }
    }

    /// Enable static mode (no animation, use fixed indicator)
    pub fn set_static_mode(&mut self, enabled: bool) {
        self.static_mode = enabled;
    }

    /// Set a custom message
    pub fn with_message(mut self, message: String) -> Self {
        self.message = Some(message);
        self
    }

    /// Set custom frames
    pub fn with_frames(mut self, frames: Vec<&'static str>) -> Self {
        self.frames = frames;
        self
    }

    /// Set the animation phase, switching to the appropriate frame set.
    /// Resets frame index if the phase changes.
    pub fn set_phase(&mut self, phase: SpinnerPhase) {
        if self.phase != phase {
            self.phase = phase;
            self.frames = match phase {
                SpinnerPhase::Thinking => FRAMES_THINKING.to_vec(),
                SpinnerPhase::Streaming => FRAMES_STREAMING.to_vec(),
                SpinnerPhase::Tool => FRAMES_TOOL.to_vec(),
                SpinnerPhase::Default => FRAMES_DEFAULT.to_vec(),
            };
            self.current_frame = 0;
        }
    }

    /// Get the current phase
    pub fn phase(&self) -> SpinnerPhase {
        self.phase
    }

    /// Advance to next frame (no-op in static mode)
    pub fn tick(&mut self) {
        if !self.static_mode {
            self.current_frame = (self.current_frame + 1) % self.frames.len();
        }
    }

    /// Get current frame index
    pub fn current_frame(&self) -> usize {
        self.current_frame
    }

    /// Get the current spinner character (static "●" in static mode)
    pub fn current_char(&self) -> &'static str {
        if self.static_mode {
            "●"
        } else {
            self.frames[self.current_frame]
        }
    }

    /// Get the message text (if any)
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Render the spinner
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let spinner = self.frames[self.current_frame];

        let content = if let Some(ref msg) = self.message {
            Line::from(vec![
                Span::styled(
                    spinner,
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(msg, Style::default().fg(theme.text)),
            ])
        } else {
            Line::from(vec![Span::styled(
                spinner,
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )])
        };

        let paragraph = Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.accent)),
            )
            .alignment(Alignment::Center);

        frame.render_widget(paragraph, area);
    }
}

impl Default for SpinnerWidget {
    fn default() -> Self {
        Self::new()
    }
}

/// Multi-progress widget for showing multiple progress bars simultaneously.
///
/// Designed for future parallel tool execution where multiple tools may run
/// concurrently. Each bar has a label, progress value, and color.
///
/// # Example
/// ```ignore
/// let mut widget = MultiProgressWidget::new()
///     .add_bar("Build".to_string(), 0.3, Color::Green)
///     .add_bar("Test".to_string(), 0.7, Color::Cyan);
/// widget.update("Build", 0.9);
/// widget.render(frame, area);
/// ```
#[derive(Debug, Clone)]
pub struct MultiProgressWidget {
    bars: Vec<(String, f64, Color)>,
    show_labels: bool,
}

impl MultiProgressWidget {
    /// Create a new multi-progress widget
    pub fn new() -> Self {
        Self {
            bars: Vec::new(),
            show_labels: true,
        }
    }

    /// Add a progress bar
    pub fn add_bar(mut self, label: String, progress: f64, color: Color) -> Self {
        self.bars.push((label, progress, color));
        self
    }

    /// Show or hide labels
    pub fn with_labels(mut self, show: bool) -> Self {
        self.show_labels = show;
        self
    }

    /// Clear all bars
    pub fn clear(&mut self) {
        self.bars.clear();
    }

    /// Check if a bar with the given label exists
    pub fn has_bar(&self, label: &str) -> bool {
        self.bars.iter().any(|(l, _, _)| l == label)
    }

    /// Add a bar or update it if it already exists
    pub fn add_or_update(&mut self, label: &str, progress: f64, color: Color) {
        if let Some(bar) = self.bars.iter_mut().find(|(l, _, _)| l == label) {
            bar.1 = progress.clamp(0.0, 1.0);
        } else {
            self.bars
                .push((label.to_string(), progress.clamp(0.0, 1.0), color));
        }
    }

    /// Update a bar's progress
    pub fn update(&mut self, label: &str, progress: f64) {
        if let Some(bar) = self.bars.iter_mut().find(|(l, _, _)| l == label) {
            bar.1 = progress.clamp(0.0, 1.0);
        }
    }

    /// Render the multi-progress widget
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let mut content = Vec::new();

        let bar_width = 30_usize; // Fixed width for individual bars

        for (label, progress, color) in &self.bars {
            let filled = (*progress * bar_width as f64) as usize;
            let _empty = bar_width.saturating_sub(filled);

            let mut line = Vec::new();

            if self.show_labels {
                line.push(Span::styled(
                    format!("{label:20} "),
                    Style::default().fg(theme.text),
                ));
            }

            // Progress bar
            for i in 0..bar_width {
                let char = if i < filled { "█" } else { "░" };
                line.push(Span::styled(
                    char,
                    Style::default().fg(if i < filled { *color } else { theme.text_dim }),
                ));
            }

            line.push(Span::styled(
                format!(" {:5.1}%", progress * 100.0),
                Style::default().fg(theme.accent),
            ));

            content.push(Line::from(line));
        }

        let paragraph = Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.accent))
                    .title(" Progress "),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
    }
}

impl Default for MultiProgressWidget {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_bar_creation() {
        let bar = ProgressBarWidget::new();
        assert_eq!(bar.progress, 0.0);
        assert_eq!(bar.percentage(), 0.0);
    }

    #[test]
    fn test_progress_bar_with_values() {
        let bar = ProgressBarWidget::new()
            .with_progress(0.5)
            .with_title("Loading".to_string());

        assert_eq!(bar.progress, 0.5);
        assert_eq!(bar.percentage(), 50.0);
        assert!(bar.title.is_some());
    }

    #[test]
    fn test_progress_bar_clamping() {
        let bar = ProgressBarWidget::new().with_progress(1.5);
        assert_eq!(bar.progress, 1.0);

        let bar = ProgressBarWidget::new().with_progress(-0.5);
        assert_eq!(bar.progress, 0.0);
    }

    #[test]
    fn test_spinner_creation() {
        let spinner = SpinnerWidget::new();
        assert_eq!(spinner.current_frame, 0);
        assert_eq!(spinner.frames.len(), 10);
    }

    #[test]
    fn test_spinner_tick() {
        let mut spinner = SpinnerWidget::new();
        spinner.tick();
        assert_eq!(spinner.current_frame, 1);

        spinner.tick();
        assert_eq!(spinner.current_frame, 2);
    }

    #[test]
    fn test_spinner_phase_switching() {
        let mut spinner = SpinnerWidget::new();
        assert_eq!(spinner.frames.len(), 10); // default braille

        spinner.set_phase(SpinnerPhase::Thinking);
        assert_eq!(spinner.frames.len(), 8); // full braille circle
        assert_eq!(spinner.current_frame, 0); // reset on phase change

        spinner.set_phase(SpinnerPhase::Thinking); // same phase — no reset
        assert_eq!(spinner.current_frame, 0);

        spinner.tick();
        assert_eq!(spinner.current_frame, 1);

        spinner.set_phase(SpinnerPhase::Tool);
        assert_eq!(spinner.frames.len(), 4); // quarter circle
        assert_eq!(spinner.current_frame, 0); // reset on phase change

        spinner.set_phase(SpinnerPhase::Default);
        assert_eq!(spinner.frames.len(), 10); // back to default
    }

    #[test]
    fn test_multi_progress_add_bar() {
        let widget = MultiProgressWidget::new()
            .add_bar("Task 1".to_string(), 0.3, Color::Green)
            .add_bar("Task 2".to_string(), 0.7, Color::Blue);

        assert_eq!(widget.bars.len(), 2);
    }

    #[test]
    fn test_multi_progress_update() {
        let mut widget =
            MultiProgressWidget::new().add_bar("Task 1".to_string(), 0.3, Color::Green);

        widget.update("Task 1", 0.8);
        assert_eq!(widget.bars[0].1, 0.8);
    }
}
