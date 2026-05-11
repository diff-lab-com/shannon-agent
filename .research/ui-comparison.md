# TUI Comparison: Claude Code vs Codex CLI vs OpenCode vs Shannon

## 1. Framework & Language

| Feature | Claude Code | Codex CLI | OpenCode | Shannon |
|---------|------------|-----------|----------|---------|
| Language | TypeScript | Rust | Go | Rust |
| Framework | Ink (React for terminal) | Custom (crossterm-based) | Bubble Tea (Charmbracelet) | ratatui + crossterm |
| Architecture | React components | Elm-like (App/Event/Render) | Elm Architecture (Model/Update/View) | Widget-based render loop |
| Size | 785KB bundled JS | codex-rs/tui/ (large crate) | ~20 files | ~15 files in widgets/ |

## 2. Layout

| Feature | Claude Code | Codex CLI | OpenCode | Shannon |
|---------|------------|-----------|----------|---------|
| Structure | Single column + status | History cells + composer | Split pane (70/30) + bottom editor | Chat + status bar + input |
| Sidebar | None | None | Right panel (files, LSP) | Disabled (planned) |
| Input area | Bottom composer | Bottom pane with modes | Bottom textarea (10%) | Bottom prompt bar |
| Status bar | Inline status + cost | Header bar | Full status (model, cost, LSP, context%) | 2-line status bar |
| Viewport | Alternate screen (fullscreen) or inline | Inline viewport with scrollback insertion | Bubble Tea viewport | Inline viewport (ratatui) |

## 3. Key Differentiating Features

### Claude Code
- **Ink/React**: Component-based rendering with Yoga layout engine
- **/tui fullscreen**: Flicker-free alternate screen mode
- **40+ tools**: Tiered permission system (ask/edit/plan/auto)
- **Vim mode**: Full vim input mode (src/vim/)
- **Hooks system**: Pre/post execution hooks
- **Memory system**: 4-type taxonomy (user/feedback/project/reference)
- **Skills**: Command templates with frontmatter
- **Scrollback issues**: Major pain point — alternate screen buffer prevents terminal scrollback

### Codex CLI
- **Scrollback insertion**: Completed responses inserted into terminal scrollback, inline viewport only for active response
- **History cells**: Each turn is a "cell" with its own scroll state
- **Diff rendering**: Side-by-side diff with syntax highlighting
- **Markdown**: Custom markdown renderer with streaming support
- **Themes**: Theme picker with preview
- **Image support**: Screenshots and image attachments in composer
- **Tab-to-queue**: Queue follow-up during streaming
- **Esc to edit**: Double-Esc to edit previous messages, fork conversation
- **Draft history**: Up/Down navigates prior drafts
- **Ctrl+R**: Search prompt history
- **Frames**: ASCII art animation frames (loading spinners)

### OpenCode
- **Bubble Tea**: Clean Elm Architecture with pubsub events
- **9 themes**: Adaptive light/dark colors (Catppuccin, Dracula, Gruvbox, etc.)
- **Side-by-side diff**: With intra-line character-level highlighting
- **Chroma syntax**: Dynamic theme-driven syntax highlighting
- **Glamour markdown**: Full markdown rendering with custom style config
- **Overlay system**: All modals as centered overlays with drop shadows
- **Session compaction**: Auto-compact at 95% context usage
- **Context usage bar**: Shows percentage of context window used
- **LSP integration**: Shows diagnostics (errors/warnings) in status bar
- **File sidebar**: Modified files with diff stats (+N/-N)
- **Render caching**: Per-message cache keyed by ID+width
- **Tool hierarchy**: Recursive tree rendering for sub-agent tool calls

## 4. Shannon Gaps (vs Competitors)

### Critical (users notice immediately)
1. **No side-by-side diff view** — Codex and OpenCode both render diffs visually
2. **No syntax highlighting** — Codex and OpenCode use Chroma/tree-sitter
3. ~~**No render caching**~~ → **Done** — Per-message cell cache with width-keyed invalidation (renderable.rs)
4. ~~**No context usage indicator**~~ → **Done** — Visual bar in status line 1 (status_bar.rs)
5. ~~**No prompt history search**~~ → **Done** — Ctrl+R incremental search with highlighting

### Important (improves daily workflow)
6. ~~**No theme picker UI**~~ → **Done** — `/theme` fuzzy picker with live preview
7. ~~**No overlay/modal system**~~ → **Done** — Centered dialogs with diff preview for permissions
8. **No file sidebar** — OpenCode shows modified files with diff stats
9. **No image input support** — Codex supports screenshots in composer
10. **No message edit/fork** — Codex has Esc to edit previous messages

### Nice to have (polish)
11. **No ASCII art frames** — Codex has loading animations
12. **No session compaction indicator** — OpenCode shows "Summarizing..."
13. ~~**No draft history navigation**~~ → **Done** — Up/Down in empty input cycles command history
14. ~~**No LSP diagnostics in status**~~ → **Done** — Error/warning counts in status line 2

## 5. Shannon Advantages

- **Scrollback insertion** (Codex-style) — completed responses committed to terminal history
- **Full-history scrolling** — PgUp/PgDn scrolls all messages including committed ones
- **Vim mode** (matches Claude Code)
- **Session state persistence**
- **Desktop notifications**
- **Token rate display** — shows tok/s during streaming
- **Keyboard hints** — context-aware bottom bar + F1 full overlay
- **Toast notifications** — auto-dismiss feedback for queued messages, etc.
- **Render caching** — per-message cell cache avoids re-layout on stable content
- **Context usage bar** — visual progress in status bar
- **Ctrl+R search** — incremental chat search with match highlighting
- **LSP diagnostics** — error/warning counts in status bar
- **Draft history** — Up/Down navigates prior inputs
- **RenderContext struct** — clean parameter passing (refactored from 23-arg function)
- **Git branch in status bar** — auto-detected and refreshed every 10s
- **Scroll-to-bottom indicator** — floating "↓ N new · End = jump" when scrolled up
- **Enter-to-queue** — Enter during streaming auto-queues follow-up message
- **Chat search** — `/` activates search, real-time match highlighting, n/N navigation with auto-scroll
- **Vim word movement** — `w`/`b` with proper vim semantics (word/punctuation classes)
- **Lightweight** — smaller footprint than Codex or OpenCode
