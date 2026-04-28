# UI Visual Experience Upgrade Design

**Date**: 2026-04-28
**Status**: Implemented
**Scope**: Theme system, Diff rendering, Markdown/Code blocks, Tool call visuals, Advanced features

## Summary

Comprehensive visual experience upgrade comparing Shannon Code against Claude Code, Codex CLI, and OpenCode. Four phases implemented across all UI rendering surfaces.

## Phase 1: Theme System + Diff Rendering

### Theme System (28 → 50+ color tokens)
- Added diff extended tokens: `diff_added_bg`, `diff_removed_bg`, `diff_context`, `diff_context_bg`, `diff_added_word`, `diff_removed_word`, `diff_line_number`, `diff_line_number_bg`
- Added syntax highlighting tokens: `syntax_keyword`, `syntax_function`, `syntax_string`, `syntax_number`, `syntax_comment`, `syntax_type`, `syntax_variable`, `syntax_operator`
- Added tool category tokens: `tool_read`, `tool_write`, `tool_search`, `tool_bash`
- Added fullscreen tokens: `fullscreen_bg`, `fullscreen_border`
- Added subagent colors: `subagent_1` through `subagent_8`
- Added misc tokens: `selection_bg`, `link`

### New Themes (3 → 15)
Built-in: dark, light, dracula, tokyonight, catppuccin_mocha, gruvbox_dark, nord, kanagawa, monokai, onedark, everforest, ayu, flexoki, dark_daltonized, light_daltonized

### Enhanced color parsing
- `#RGB` 3-digit hex
- `rgb(r,g,b)` function format
- `ansi256(n)` terminal index format

### Diff Rendering Upgrade
- Background color fills (dark/light terminal adaptive)
- Word-level highlighting via LCS algorithm
- Line number gutters with `│` separator
- Hunk-aware parsing from `@@` headers
- Collapse/expand for large diffs (>100 lines)
- DiffStats summary (files changed, additions, deletions)
- Adaptive color palette for dark/light terminals

## Phase 2: Markdown + Code Block Rendering

### Markdown
- Table alignment with box-drawing borders (`┌─┬─┐`, `├─┼─┤`, `└─┴─┘`)
- Left/right/center column alignment from separator row
- Nested lists (ordered/unordered, mixed)
- Task list checkboxes (`☑`/`☐`)
- OSC 8 hyperlinks
- Blockquote styling with `│` bars and nesting support
- Heading decorations (█ H1, ▌ H2, ▎ H3)

### Code Blocks
- Title bar with language tag and filename hint
- Line numbers (dim, right-aligned)
- Long block folding (>20 lines, shows first 10 + last 5)

## Phase 3: Tool Call Visual Improvements

- 5-category tool classification: Read, Write, Search, Bash, Agent
- Category-specific icons: `▸` Read, `✎` Write, `⊛` Search, `$` Bash, `◆` Agent
- Category-specific colors from theme tokens
- Tool execution duration display (auto-formatted: ms/s/m)
- Enhanced status indicators with category-aware styling

## Phase 4: Advanced Visual Features

### Message Bubble Styling
- Role prefix indicators: `You >`, `AI >`, `SYS >`
- Separator lines between messages of different roles

### Fullscreen Mode
- F11 toggle (configurable in keybindings.json)
- Hides ALL chrome (header, status, sidebar, borders)
- `[FS]` indicator in top-right corner
- Uses `fullscreen_bg`/`fullscreen_border` theme tokens

### Search Highlighting in Chat
- Ctrl+H activates chat search
- Highlights matches with `selection_bg`/`primary` colors
- Focused match has bold highlighting
- Navigation overlay shows "match N of M"
- N/P keys for next/previous match

## Files Modified

| File | Changes |
|------|---------|
| `theme.rs` | Expanded to 50+ tokens, 15 themes, enhanced color parsing |
| `tool_format.rs` | Diff rendering engine with background fills, word-level highlighting, line numbers, collapse, stats |
| `render.rs` | Markdown table alignment, nested lists, task lists, blockquotes, heading decorations |
| `widgets/mod.rs` | Code block title bar/line numbers/folding, tool category icons, duration display, message bubbles, search highlighting, fullscreen mode |
| `keybindings.rs` | Added `fullscreen` (F11) and `chat_search` (Ctrl+H) keybindings |
| `repl/mod.rs` | Fullscreen mode state, chat search state and methods |
| `repl/input.rs` | Fullscreen toggle handler, chat search input handler |
| `repl/render.rs` | Fullscreen rendering, search overlay rendering |
| `repl/query.rs` | Updated render call with new parameters |
| `screenshot.rs` | Updated render call with new parameters |
