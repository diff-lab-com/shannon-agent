# Shannon Desktop — User Guide

Shannon is your AI workspace. It runs on your computer and helps with
everyday tasks: writing, research, scheduling, automations.

This guide assumes no technical background.

## In this guide

- [Getting Started](getting-started.md) — Install, set up your first AI
  provider, send your first message.
- [Features](features.md) — Walkthrough of the four main screens: Chat,
  Scheduled Tasks, Triage, Settings.

## What Shannon does

- **Chat** — Talk to AI models from multiple providers (Anthropic,
  OpenAI, Ollama, Deepseek). Your conversations stay on your machine.
- **Scheduled Tasks** — Set routines that fire on a schedule or trigger.
- **Triage** — A unified inbox for notifications, errors, and items
  that need your attention.
- **Extensions** — Install additional capabilities (MCP servers,
  skills, agents) from a curated catalog or your own files.

## What Shannon doesn't do

- Doesn't train on your conversations.
- Doesn't sync to a cloud unless you explicitly connect an account.
- Doesn't run code in hidden windows. Every tool call shows in the UI.

## Privacy

Your data lives in `~/.shannon/` on your machine. The app only makes
network calls to:

1. The AI provider you chose (e.g. `api.anthropic.com`).
2. URLs the AI explicitly opens for you, with your approval.

There is no telemetry, no crash reporting, no auto-update in this
release. (See `docs/adr/0001-product-positioning-and-ci.md` for the
positioning decisions behind this.)
