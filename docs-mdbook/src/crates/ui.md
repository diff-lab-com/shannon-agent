# shannon-ui

Terminal UI built with ratatui + crossterm. Provides the interactive REPL experience.

## Key Components

### REPL (`repl/`)
- Main event loop (user input → LLM → rendering)
- Vim mode (normal/insert/visual)
- Slash command handling
- Settings watcher for live config reload

### Widgets
- `ChatMessage` — Renders assistant/user/tool messages with markdown
- `DiffViewer` — Full-screen diff viewer (unified format)
- `MultiProgressWidget` — Parallel tool execution progress
- `AgentBarWidget` — Agent status dashboard (3 views: compact/expanded/detailed)
- `DialogWidget` — Modal dialogs for confirmations
- `StatusBar` — Model, context usage, session info

### Rendering
- Markdown rendering with syntax highlighting
- Streaming diff tracking with configurable threshold
- Markdown table renderer with box-drawing borders
- Tool output formatting with color-coded status

### Input
- Vim-style modal editing
- Image paste from clipboard (macOS pngpaste / Linux xclip)
- URL image fetching

### File Watching
- `SourceWatcher` — Detects project file changes
- `DiagnosticWatcher` — Auto-runs `cargo check` on changes
- `SettingsWatcher` — Watches config files
- `CustomCommandWatcher` — Watches command directories
