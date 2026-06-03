# Shannon Code

AI-powered code assistant for VS Code with multi-provider LLM support, MCP extensions, and intelligent tool use.

## Features

- **Multi-provider LLM** — Anthropic, OpenAI, Ollama, DeepSeek out of the box
- **Chat interface** — Interactive conversation panel with Markdown rendering and syntax highlighting
- **Tool use** — File read/write/edit, bash execution, search, and more
- **MCP extensions** — Model Context Protocol for adding custom tools and servers
- **Diff review** — Review, accept, or reject AI-generated file changes
- **Keyboard shortcuts** — `Ctrl+Shift+S` to open chat, `Ctrl+Shift+Enter` to send, `Escape` to stop

## Getting Started

1. Install the [Shannon Code CLI](https://github.com/shannon-agent/shannon-code)
2. Install this extension
3. Set your API key: `SHANNON_API_KEY` env var or in extension settings
4. Open the chat with `Ctrl+Shift+S` or the status bar icon

## Configuration

| Setting | Description | Default |
|---------|-------------|---------|
| `shannon.cliPath` | Path to Shannon CLI binary | `shannon` |
| `shannon.provider` | LLM provider | `anthropic` |
| `shannon.model` | Model override (empty = default) | `""` |
| `shannon.apiKey` | API key (or use env var) | `""` |

## Commands

| Command | Shortcut | Description |
|---------|----------|-------------|
| `Shannon: Open Chat` | `Ctrl+Shift+S` | Open the chat panel |
| `Shannon: Send Prompt` | `Ctrl+Shift+Enter` | Send a prompt |
| `Shannon: Stop Generation` | `Escape` | Stop current generation |
| `Shannon: Show Pending Changes` | — | Review file changes |
| `Shannon: Accept All Changes` | — | Accept all pending changes |
| `Shannon: Reject All Changes` | — | Reject all pending changes |

## Requirements

- VS Code 1.85+
- Shannon Code CLI installed and in PATH

## License

Apache-2.0
