# Shannon Desktop

**Status: Phase 5 complete — production-ready desktop app with full feature set.**

Tauri v2 desktop app wrapping Shannon's core query engine for a native desktop
chat experience with React 18 frontend and comprehensive tool integration.

## Current Features

### Core LLM
- Multi-provider LLM support (Anthropic, OpenAI, DeepSeek, Ollama)
- Streaming responses via Tauri event system
- Markdown rendering (marked.js + highlight.js)
- Tool call display with collapsible input/output
- Provider/model switching at runtime
- Config persistence (`~/.shannon/desktop.json`)

### Session Management
- Create, switch, delete, rename, duplicate sessions
- Session search by title substring
- Session export to Markdown and JSON
- Tab bar with multi-session switching

### Tool Integration
- File read/write/edit with diff view and hunk-level apply
- Bash command execution with output streaming
- MCP server lifecycle management (add, remove, restart, list)
- MCP tool discovery with actual tool counts
- Permission prompts for tool execution (allow/deny)
- Approval mode selector (auto-ask, auto-allow, etc.)

### Agent & Task Dashboard
- Background agent task management (start, cancel, list)
- Agent dashboard with real-time status and progress
- Task board reading from `.claude/tasks/*.json`
- File tree browser with git status integration

### Desktop Integration
- System tray icon with menu (Show, New Session, Check Updates, Status, Quit)
- Auto-updater with startup check and download progress banner
- Window state persistence (position and size restored on restart)
- Global keyboard shortcuts (show/hide, new session, focus input)
- Skill browser for available commands

### UI
- React 18 frontend with shadcn/ui components
- Theme support (Tokyo Night, Catppuccin, Nord)
- Command palette (Ctrl+Shift+P)
- Settings panel with provider/model configuration
- Terminal pane for command output
- Toast notifications
- Keyboard navigation support

## Building

Requires Tauri system deps (GTK3, webkit2gtk on Linux). Then:

```bash
just desktop            # dev build
just desktop-release    # release build
```

## Test Coverage

- ~100 Rust backend tests (commands, config, events, mcp)
- ~324 frontend tests (components, context, hooks)
- Run with: `cargo test -p shannon-desktop` and `cd crates/shannon-desktop/ui && npx vitest run`
