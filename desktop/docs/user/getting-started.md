# Getting Started

This walks you through installing Shannon Desktop and sending your first
message. Expect about 5 minutes.

## 1. Install

Download the installer for your platform from the Releases page (link
will appear here once the public release pipeline is wired — see
`docs/adr/0001-product-positioning-and-ci.md` §0.1 for status).

| Platform | Format |
|---|---|
| macOS | `.dmg` — drag to Applications |
| Windows | `.msi` — double-click to install |
| Linux | `.AppImage` (portable) or `.deb` |

First launch will be slow (~3 seconds) as the app initializes its data
folder at `~/.shannon/`.

## 2. Pick an AI provider

The Welcome wizard walks you through this. You have four options:

| Provider | Best for | Needs API key? |
|---|---|---|
| Anthropic (Claude) | Coding, writing | Yes |
| OpenAI | Reasoning, vision | Yes |
| Ollama | Local, private | No (runs locally) |
| Deepseek | Long documents, cost-effective | Yes |

If you don't have an API key yet, pick **Ollama** to try without one.
You can switch providers later in **Settings → Models**.

API keys are stored locally in `~/.shannon/.shannon.toml`. They never
leave your machine except to the provider's API.

## 3. Send your first message

1. Click **New Chat** (top-left, or press `Ctrl+Shift+N` /
   `Cmd+Shift+N`).
2. Type in the bottom input box.
3. Press **Enter** to send. `Shift+Enter` for a new line.

That's it. The AI's response streams into the chat window.

Each AI turn can use tools — file reads, web search, scheduled task
creation — and every tool call shows up inline so you can see what's
happening. You can cancel a running turn with the stop button or
`Ctrl+Shift+C`.

## 4. Where things live

| Path | Contents |
|---|---|
| `~/.shannon/` | All app data (config, sessions, tasks) |
| `~/.shannon/desktop/config.json` | Desktop UI preferences |
| `~/.shannon/.shannon.toml` | Engine config (provider, API key, model) |
| `~/.shannon/sessions/` | Your chat history |
| `~/.shannon/scheduled-tasks/` | Scheduled routines |

Uninstalling Shannon removes the app but leaves `~/.shannon/` in place.
To wipe everything: delete that folder.

## Next steps

- **[Features](features.md)** — Tour of the four main screens.
- Switch to **Advanced mode** (sidebar toggle) when you want Extensions
  and the experimental One-Person-Company surface.

## Troubleshooting

**"API key missing" banner appears in chat.** Go to
**Settings → Models**, choose your provider, paste the key.

**App launches to a blank window.** Most often caused by a stale lock
file. Quit, delete `~/.shannon/desktop/.lock`, relaunch.

**Can't find a feature you read about.** It's probably behind the
Advanced mode toggle at the bottom of the sidebar.
