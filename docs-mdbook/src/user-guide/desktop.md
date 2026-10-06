# Desktop App

Shannon Desktop is the GUI surface of the same engine that powers the terminal — not a separate product. Sessions, memory, and configuration are shared: start a task in the terminal, continue it on the desktop.

## Launch

```bash
shannon desktop            # launch the app
shannon desktop --install  # first time: download the platform bundle
```

The desktop installer bundles the `shannon` CLI. The app is built on **Tauri 2 + React 19 — not Electron** — with a system tray, global shortcuts, auto-update, and 8 themes.

## Simple mode (for everyone)

Four surfaces cover the daily loop:

- **Chat** — talk to any model. Tool calls are shown inline and can be approved or revoked one by one. Drag-and-drop images and files, or dictate with voice input.
- **Scheduled Tasks** — your automations, with a calendar view, a dependency (DAG) view, and Active / History / Worktrees tabs.
- **Triage** — a single inbox for everything your agents did while you were away: completions, failures, webhook events. Continue in the original session, rerun, or snooze.
- **Settings** — six sub-pages including providers, voice, and connections.

No technical background is assumed. If you can use a chat app, you can use Simple mode.

## Advanced mode (for developers)

- **Extensions** — one-click install directory for MCP servers, skills, agents, data sources, and plugins.
- **OPC (One Person Company)** — multi-agent orchestration: mission focus, agent swarm, Kanban.
- **Multi-panel workspace** — chat, diff, preview, and terminal panels with per-project layouts.
- **Integrated terminal** — shares the agent's environment.
- **Editor & timeline** — CodeMirror-based editor with LSP symbols; per-turn timeline of what happened.
- **Memory** — persistent memory with provenance (jump from a memory entry back to the session that produced it).

## Voice input

Three providers, configured in Settings:

- **Cloud STT** — Groq (`whisper-large-v3`) or OpenAI (`whisper-1`), or any custom endpoint.
- **Local STT** — whisper.rs runs entirely on your machine; audio never leaves it. Models are managed and downloaded from the voice settings page.

## Attachments

Drag and drop, or pick files: images (with inline preview), PDFs (text extraction), and more. Attachments ride along when you dispatch a session from IM or mobile.
