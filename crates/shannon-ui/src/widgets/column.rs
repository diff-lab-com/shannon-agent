//! ColumnRenderable container for exact-height virtual scrolling.
//!
//! Phase 2: Replaces the `List`-based rendering with per-cell exact-height layout.
//! Each cell renders independently into its allocated Rect, using `Clear` before
//! render to prevent stale glyphs.

use super::renderable::{MessageCell, Renderable, SearchParams};
use crate::theme::Theme;

use std::collections::HashMap;
use parking_lot::Mutex;
use ratatui::{
    layout::Rect,
    widgets::{Clear, Widget},
};

// ── ColumnRenderable ────────────────────────────────────────────────────

/// Vertical column of `MessageCell`s with exact-height virtual scrolling.
///
/// Given a viewport area and a scroll offset, computes which cells are visible
/// and their exact positions, then renders each cell into its allocated Rect.
pub struct ColumnRenderable {
    cells: Vec<MessageCell>,
    /// Per-cell vertical scroll offsets (cell_index → scroll_y).
    cell_scrolls: HashMap<usize, u16>,
    /// Per-cell last allocated render height (cell_index → height).
    cell_allocated: Mutex<HashMap<usize, u16>>,
    /// Reusable buffer for height computation (avoids per-frame allocation).
    heights_buf: Mutex<Vec<u16>>,
}

/// Result of the layout computation: which cells to render and where.
struct LayoutResult {
    /// (rect, cell_index) pairs for visible cells.
    visible: Vec<(Rect, usize)>,
}

impl ColumnRenderable {
    pub fn new() -> Self {
        Self {
            cells: Vec::new(),
            cell_scrolls: HashMap::new(),
            cell_allocated: Mutex::new(HashMap::new()),
            heights_buf: Mutex::new(Vec::new()),
        }
    }

    /// Number of cells.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether there are no cells.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Clear all cells.
    pub fn clear(&mut self) {
        self.cells.clear();
        self.cell_scrolls.clear();
        self.cell_allocated.lock().clear();
    }

    /// Push a new cell.
    pub fn push(&mut self, cell: MessageCell) {
        self.cells.push(cell);
    }

    /// Remove and return the last cell.
    pub fn pop(&mut self) -> Option<MessageCell> {
        let cell = self.cells.pop();
        if cell.is_some() {
            let idx = self.cells.len();
            self.cell_scrolls.remove(&idx);
            self.cell_allocated.lock().remove(&idx);
        }
        cell
    }

    /// Remove the first cell and re-key remaining index maps.
    pub fn pop_front(&mut self) -> Option<MessageCell> {
        if self.cells.is_empty() {
            return None;
        }
        let cell = self.cells.remove(0);
        // Decrement all keys by 1 since indices shifted
        let mut new_scrolls = HashMap::new();
        let scrolls = std::mem::take(&mut self.cell_scrolls);
        for (k, v) in scrolls {
            if k > 0 { new_scrolls.insert(k - 1, v); }
        }
        self.cell_scrolls = new_scrolls;
        let mut alloc = self.cell_allocated.lock();
        let mut new_alloc = HashMap::new();
        for (k, v) in alloc.drain() {
            if k > 0 { new_alloc.insert(k - 1, v); }
        }
        *alloc = new_alloc;
        Some(cell)
    }

    /// Truncate to `len` cells, removing any beyond that index.
    pub fn truncate(&mut self, len: usize) {
        self.cells.truncate(len);
        self.cell_scrolls.retain(|&k, _| k < len);
        self.cell_allocated.lock().retain(|&k, _| k < len);
    }

    /// Get immutable access to a cell by index.
    pub fn get(&self, index: usize) -> Option<&MessageCell> {
        self.cells.get(index)
    }

    /// Get mutable access to a cell by index.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut MessageCell> {
        self.cells.get_mut(index)
    }

    /// Get the desired height of a cell at the given width.
    pub fn cell_height(&self, index: usize, width: u16) -> u16 {
        self.cells.get(index).map(|c| c.desired_height(width)).unwrap_or(1)
    }

    /// Set the vertical scroll offset for a specific cell.
    pub fn set_cell_scroll(&mut self, cell_index: usize, scroll_y: u16) {
        if scroll_y == 0 {
            self.cell_scrolls.remove(&cell_index);
        } else {
            self.cell_scrolls.insert(cell_index, scroll_y);
        }
    }

    /// Get the vertical scroll offset for a specific cell.
    pub fn cell_scroll(&self, cell_index: usize) -> u16 {
        self.cell_scrolls.get(&cell_index).copied().unwrap_or(0)
    }

    /// Get the last allocated render height for a specific cell.
    pub fn cell_allocated_height(&self, cell_index: usize) -> u16 {
        self.cell_allocated.lock().get(&cell_index).copied().unwrap_or(0)
    }

    /// Invalidate cached heights for all cells (e.g., after terminal resize).
    pub fn invalidate_all(&self) {
        for cell in &self.cells {
            cell.invalidate_cache();
        }
    }

    /// Compute total height of all cells at the given width.
    #[allow(dead_code)]
    fn total_height(&self, width: u16) -> u16 {
        self.compute_heights(width, 0);
        self.heights_buf.lock().iter().copied().sum()
    }

    /// Fill heights_buf with desired_height for cells from `start` onward.
    /// Heights are stored as `buf[i - start]` to avoid computing uncommitted cells.
    fn compute_heights(&self, width: u16, start: usize) {
        let mut buf = self.heights_buf.lock();
        buf.clear();
        buf.extend(self.cells[start..].iter().map(|c| c.desired_height(width)));
    }

    /// Compute layout: determine which cells are visible and their positions.
    ///
    /// `area` — viewport rect
    /// `scroll_offset` — index of the focused message (absolute index in cells)
    /// `start` — first cell index to consider (e.g., committed_count)
    fn layout(&self, area: Rect, scroll_offset: usize, start: usize, top_align: bool) -> LayoutResult {
        if self.cells.is_empty() || area.height == 0 || start >= self.cells.len() {
            return LayoutResult { visible: Vec::new() };
        }

        let width = area.width;
        let viewport_h = area.height as usize;
        let msg_count = self.cells.len();
        let focused = scroll_offset.clamp(start, msg_count - 1);

        // Compute exact height for each cell from `start` onward (reusing buffer)
        self.compute_heights(width, start);
        let heights = self.heights_buf.lock();

        // Walk backward from focused message accumulating rows to find vis_start.
        // Always include the focused cell, even if it overflows the viewport.
        // Don't go below `start` (committed messages are in terminal scrollback).
        let mut vis_start = focused;
        let mut rows_used: usize = 0;
        for i in (start..=focused).rev() {
            let h = heights[i - start] as usize;
            if rows_used + h > viewport_h && i != focused {
                break;
            }
            rows_used += h;
            vis_start = i;
        }

        // Walk forward from focused to fill remaining rows after the focused message.
        let mut vis_end = focused;
        for i in (focused + 1)..msg_count {
            let h = heights[i - start] as usize;
            if rows_used + h > viewport_h {
                break;
            }
            rows_used += h;
            vis_end = i;
        }

        // Build visible cell rects from vis_start to vis_end.
        // Top-align when committed messages exist (content flows after scrollback);
        // bottom-align otherwise (Codex-style anchoring near input bar).
        let mut visible = Vec::new();
        let pad_top = if top_align {
            0
        } else {
            (viewport_h.saturating_sub(rows_used)) as u16
        };
        let mut y = area.y + pad_top;

        for i in vis_start..=vis_end {
            let h = heights[i - start];
            if y >= area.y + area.height {
                break;
            }
            let available = (area.y + area.height).saturating_sub(y);
            let render_h = h.min(available);
            if render_h > 0 {
                visible.push((
                    Rect::new(area.x, y, area.width, render_h),
                    i,
                ));
            }
            y += h;
        }

        LayoutResult { visible }
    }

    /// Render visible cells into the buffer.
    ///
    /// `area` — the full viewport rect
    /// `scroll_offset` — absolute index of focused message
    /// `start` — first cell index to consider (e.g., committed_count)
    /// `search` — optional search params for match highlighting
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
        theme: &Theme,
        scroll_offset: usize,
        start: usize,
        search: Option<&SearchParams<'_>>,
        streaming: bool,
    ) {
        // Clear the entire viewport first (each cell also clears its own area,
        // but we need this to erase gaps between cells)
        Clear.render(area, buf);

        let layout = self.layout(area, scroll_offset, start, start > 0);

        tracing::debug!(
            "ColumnRenderable::render cells={} start={} offset={} area={}x{} visible={}",
            self.cells.len(), start, scroll_offset, area.width, area.height,
            layout.visible.len()
        );
        for (rect, idx) in &layout.visible {
            let desired = self.cells.get(*idx).map(|c| c.desired_height(rect.width)).unwrap_or(0);
            tracing::debug!(
                "  cell[{}] y={} h={} desired={} alloc_diff={}",
                idx, rect.y, rect.height, desired, desired as i32 - rect.height as i32
            );
        }

        // Record allocated heights for scroll calculations, pruning stale entries
        {
            let mut alloc = self.cell_allocated.lock();
            alloc.retain(|&k, _| layout.visible.iter().any(|&(_, idx)| idx == k));
            for (cell_rect, cell_idx) in &layout.visible {
                alloc.insert(*cell_idx, cell_rect.height);
            }
        }

        for (cell_rect, cell_idx) in &layout.visible {
            if let Some(cell) = self.cells.get(*cell_idx) {
                let desired = cell.desired_height(cell_rect.width);
                let scroll_y = self.cell_scrolls.get(cell_idx).copied().unwrap_or(0);
                if let Some(sp) = search {
                    let cell_sp = SearchParams {
                        query: sp.query,
                        matches: sp.matches,
                        focused_idx: sp.focused_idx,
                        cell_index: *cell_idx,
                    };
                    cell.render_with_search(*cell_rect, buf, theme, &cell_sp, scroll_y);
                } else if desired > cell_rect.height {
                    cell.render_scrolled(*cell_rect, buf, theme, scroll_y);
                } else {
                    cell.render(*cell_rect, buf, theme);
                }
            }
        }

        // Streaming cursor: draw a blinking █ at the end of the last rendered content
        if streaming {
            if let Some((last_rect, _)) = layout.visible.last() {
                let cursor_style = ratatui::style::Style::default()
                    .fg(theme.primary)
                    .add_modifier(ratatui::style::Modifier::REVERSED);
                // Scan from bottom of last cell upward to find last non-empty line
                for y in (last_rect.y..last_rect.bottom()).rev() {
                    let mut last_content_x: Option<u16> = None;
                    for x in (last_rect.x..last_rect.right()).rev() {
                        if let Some(cell) = buf.cell_mut((x, y)) {
                            if cell.symbol() != " " && cell.symbol() != "─" && cell.symbol() != "│" {
                                last_content_x = Some(x);
                                break;
                            }
                        }
                    }
                    if let Some(x) = last_content_x {
                        let cursor_x = (x + 1).min(last_rect.right() - 1);
                        if let Some(cell) = buf.cell_mut((cursor_x, y)) {
                            cell.set_symbol("█");
                            cell.set_style(cursor_style);
                        }
                        break;
                    }
                }
            }
        }
    }
}

impl Default for ColumnRenderable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::chat::{ChatMessage, ChatRole};

    fn test_message(role: ChatRole, content: impl Into<String>) -> ChatMessage {
        ChatMessage {
            role,
            content: content.into(),
            timestamp: chrono::Utc::now(),
            image_lines: None,
            is_error: false,
            tool_name: None,
            start_time: None,
            duration_secs: None,
            spinner_frame: 0,
            folded: true,
            exit_code: None,
        }
    }

    #[test]
    fn test_column_empty() {
        let col = ColumnRenderable::new();
        assert!(col.is_empty());
        assert_eq!(col.len(), 0);
    }

    #[test]
    fn test_column_push_and_len() {
        let mut col = ColumnRenderable::new();
        col.push(MessageCell::new(test_message(ChatRole::User, "Hello"), false));
        col.push(MessageCell::new(test_message(ChatRole::Assistant, "Hi"), false));
        assert_eq!(col.len(), 2);
    }

    #[test]
    fn test_column_clear() {
        let mut col = ColumnRenderable::new();
        col.push(MessageCell::new(test_message(ChatRole::User, "Hello"), false));
        col.clear();
        assert!(col.is_empty());
    }

    #[test]
    fn test_column_layout_single_message() {
        let mut col = ColumnRenderable::new();
        col.push(MessageCell::new(test_message(ChatRole::User, "Hello"), false));

        let area = Rect::new(0, 0, 80, 24);
        let layout = col.layout(area, 0, 0, false);

        assert_eq!(layout.visible.len(), 1, "single message should produce one visible cell");
        assert_eq!(layout.visible[0].1, 0, "cell index should be 0");
        // Content should be bottom-aligned when it fits within viewport
        let content_h = col.cell_height(0, 80);
        assert_eq!(layout.visible[0].0.y, area.y + area.height - content_h,
            "content should be bottom-aligned when shorter than viewport");
    }

    #[test]
    fn test_column_layout_many_messages() {
        let mut col = ColumnRenderable::new();
        for i in 0..50 {
            col.push(MessageCell::new(test_message(ChatRole::User, format!("Message {i}")), false));
        }

        let area = Rect::new(0, 0, 80, 24);
        // scroll_offset = 49 means focused on latest message (index 49)
        let layout = col.layout(area, 49, 0, false);

        assert!(layout.visible.len() < 50, "should not render all 50 messages");
        assert!(layout.visible.len() > 0, "should render some messages");
        let last_idx = layout.visible.last().map(|(_, idx)| *idx).unwrap_or(0);
        assert_eq!(last_idx, 49, "last visible cell should be the latest message");
    }

    #[test]
    fn test_column_layout_scroll_offset() {
        let mut col = ColumnRenderable::new();
        for i in 0..20 {
            col.push(MessageCell::new(test_message(ChatRole::User, format!("Message {i}")), false));
        }

        let area = Rect::new(0, 0, 80, 24);
        let layout = col.layout(area, 10, 0, false);

        assert!(layout.visible.len() > 0);
        let indices: Vec<usize> = layout.visible.iter().map(|(_, i)| *i).collect();
        assert!(indices.contains(&10), "focused message 10 should be visible");
    }

    #[test]
    fn test_column_total_height() {
        let mut col = ColumnRenderable::new();
        col.push(MessageCell::new(test_message(ChatRole::User, "Short"), false));
        let h = col.total_height(80);
        assert!(h > 0, "total height should be > 0");
    }

    #[test]
    fn test_column_default() {
        let col = ColumnRenderable::default();
        assert!(col.is_empty());
    }

    #[test]
    fn test_cell_scroll_get_set() {
        let mut col = ColumnRenderable::new();
        col.push(MessageCell::new(test_message(ChatRole::User, "Hello"), false));
        assert_eq!(col.cell_scroll(0), 0, "default scroll should be 0");

        col.set_cell_scroll(0, 5);
        assert_eq!(col.cell_scroll(0), 5, "scroll should be 5 after set");

        col.set_cell_scroll(0, 0);
        assert_eq!(col.cell_scroll(0), 0, "scroll should be 0 after reset");
    }

    #[test]
    fn test_cell_scroll_clears_with_column() {
        let mut col = ColumnRenderable::new();
        col.push(MessageCell::new(test_message(ChatRole::User, "Hello"), false));
        col.set_cell_scroll(0, 10);
        col.clear();
        assert_eq!(col.cell_scroll(0), 0, "scroll should be cleared after column clear");
    }

    #[test]
    fn test_layout_overflow_cell_clips_height() {
        let mut col = ColumnRenderable::new();
        // Create a message with many lines of content
        let long_content: String = (0..100).map(|i| format!("Line {i}\n")).collect();
        col.push(MessageCell::new(test_message(ChatRole::User, long_content), false));

        // Use a very small viewport so the cell overflows
        let area = Rect::new(0, 0, 80, 5);
        let layout = col.layout(area, 0, 0, false);

        assert_eq!(layout.visible.len(), 1, "should have one visible cell");
        let (cell_rect, _) = layout.visible[0];
        // The allocated height should be clipped to viewport
        assert_eq!(cell_rect.height, 5, "cell should be clipped to viewport height");

        // desired_height should be much larger than allocated
        let desired = col.cells[0].desired_height(80);
        assert!(desired > 5, "desired height ({desired}) should exceed viewport (5)");
    }
}
