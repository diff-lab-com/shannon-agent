# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Shannon Desktop — Tauri v2 + React 19 + TypeScript desktop app. Cross-platform
"AI workspace" shell: chat with multiple LLM providers, run scheduled tasks,
manage agent teams, install MCP/Skill/Agent extensions, fire native
notifications. The actual reasoning engine lives in
[`shannon-code`](https://github.com/shannon-agent/shannon-code) and is pulled
into this repo as git subpath dependencies.

## Build & dev

```bash
# Prereqs: Rust 1.88+ (edition 2024), Node 20+, pnpm 10+
pnpm install --dir ui          # UI deps (required before cargo run/dev)
cargo run                      # full desktop app (runs tauri::Builder)
cargo build                    # check compile
cargo test                     # Rust integration/unit tests
cargo bench --bench load_tests # criterion benches (OPC metric aggregation)
cargo clippy                   # Rust lints (local gate — not in CI; runner can't reach github.com)

# UI-only (run inside ui/)
pnpm dev                       # vite dev server on :1420 (Tauri devUrl)
pnpm demo                      # same, but VITE_MOCK_MODE=1 — Tauri invoke()
                               #   is swapped for ui/src/lib/mock/coreMock.ts
                               #   so the UI runs without the Rust backend
pnpm build                     # tsc --noEmit then vite build → ui/dist
pnpm lint                      # tsc --noEmit (the only "lint" step)
pnpm test                      # vitest (jsdom)
pnpm test -- path/to/file      # single vitest file
pnpm test -- -t "name"         # single test by name
pnpm test:coverage             # vitest with v8 coverage
pnpm test:e2e                  # playwright (ui/playwright.config.ts)
```

### Local checks (pre-push hook)

`scripts/hooks/pre-push` runs `scripts/local-check.sh` (fmt + clippy +
test + deny + UI lint + vitest) before any push to `main` or `dev`. CI is
UI-only because the Gitea runner can't reach github.com, so the local
hook is the Rust gate.

One-time setup per clone:

```bash
git config core.hooksPath scripts/hooks
```

Bypass for a single push (e.g. WIP branch): `git push --no-verify`.

### Critical: the sibling-checkout patch

`Cargo.toml` has a `[patch."ssh://git@github.com/shannon-agent/shannon-code.git"]`
block that overrides every `shannon-*` git dep with a path dep at
`../shannon-code/crates/*`. **A standalone clone of this repo will not build**
unless a sibling `../shannon-code` checkout exists at the matching rev. This is
intentional — see the comment in `Cargo.toml`. The pinned rev in the git deps
(e.g. `00510a7`) is what `../shannon-code` must be checked out at.

### Tauri feature gate

Everything Tauri-related lives behind the `tauri` feature (default-on).
`src/lib.rs` declares `commands` plus twelve domain-specific
`commands_*` modules, `scheduled_commands`, `lsp_commands`,
`automation_commands`, `extensions_commands`, `notifications`, and
`inbound` under `#[cfg(feature = "tauri")]`. `build.rs` is a no-op
stub because Tauri's own build script replaces it when the feature
is enabled.

## Architecture

### Rust ↔ UI bridge

The app follows the standard Tauri v2 split:

1. **`src/main.rs`** — entry point. Initializes `tracing` (set
   `SHANNON_LOG_FORMAT=json` for newline-delimited JSON to stderr), registers
   all plugins (`shell`, `updater`, `global-shortcut`, `window-state`,
   `dialog`, `notification`), wires the system tray, registers every Tauri
   command in one giant `invoke_handler!`, and runs setup. Setup constructs
   `AppState`, attaches the notification handler, registers global shortcuts,
   starts the auto-updater, and emits `update-available` events to the
   frontend.

2. **`src/commands.rs`** — defines `AppState` (shared mutex-guarded state
   for the whole app) plus the remaining core commands that have not been
   extracted to a domain module: `send_message`, background-task management
   (`start_background_task`, `get_background_tasks`, `cancel_background_task`).
   `AppState` is constructed in `main.rs::setup()` and held via
   `app.manage()`.

3. **Domain-specific command modules** — each is registered alongside
   `commands::*` in `main.rs`. Extraction tracked in `CHANGELOG.md`
   (S2 P1.1). Sort order in `main.rs::invoke_handler!` matches `lib.rs`.
   - `commands_chat.rs` — `get_conversation`, `list_models`, `get_status`,
     `cancel_query`, `list_tools`.
   - `commands_sessions.rs` — 11 session-management commands (new/list/
     search/load/export/switch/set_working_dir/delete/rename/duplicate/
     branch_session) plus session helpers.
   - `commands_mcp.rs` — MCP server lifecycle, skills, addons.
   - `commands_plugins.rs` — plugin marketplace + catalog upstreams.
   - `commands_agents.rs` — agent definitions + inter-agent message history.
   - `commands_billing.rs` — billing demo data (plan, cost history,
     invoices) plus `iso_days_ago` helper.
   - `commands_permissions.rs` — permission request/respond commands.
   - `commands_files.rs` — text save, file diff/apply, file tree,
     working-dir info.
   - `commands_onboarding.rs` — `seed_sample_data` + sample task fixtures.
   - `commands_config.rs` — `configure`, `switch_provider`, `get_config`.
     Both mutation commands emit `CONFIG_UPDATED`, which the tray menu
     listener uses to rebuild the status label (no polling).
   - `commands_tasks.rs` — task board (`list_tasks`, `update_task`) plus
     `.claude/tasks/` walker helpers.
   - `commands_notifications.rs` — native OS notifications + webhook/inbound
     notification config + inbound listener supervisor. Owns
     `fire_query_notification`, `NotificationKind`, and
     `load_desktop_webhook_config` (all `pub(crate)`) used by
     `commands.rs::send_message` and `AppState::new`.
   - `scheduled_commands.rs` — Tasks board, Triage, History, Triggered
     Routines, worktree management, OPC metric aggregation. Backed by
     `~/.shannon/scheduled-tasks/`, `~/.shannon/scheduled-runs/`,
     `~/.shannon/triage.jsonl`, `~/.shannon/routine-overrides.json`.
     Field names mirror `shannon_core::scheduled_routines::ScheduledRoutine`
     verbatim — no rename to "ScheduledTask" — so the frontend can pass
     structs through unchanged.
   - `extensions_commands.rs` — unified Extensions Hub: MCP registry
     installers (`.mcpb` / stdio / OAuth), skills catalog + installer, agent
     catalog + installer, native data sources (Obsidian, IMAP), prompt
     injection scanner, signature verifier.
   - `lsp_commands.rs` — code actions, diagnostics, source-file reads.
   - `automation_commands.rs` — hook event catalog and custom permission
     profiles.
   - `notifications.rs` — `TauriNotificationHandler` implementation of the
     engine's `NotificationHandler` trait; wired into `AppState.notifier`.
   - `inbound.rs` — Slack + Telegram inbound listener used by
     `commands_notifications::restart_inbound_listener`.

4. **`src/events.rs`** — typed payloads for every frontend-bound Tauri event
   (`QueryTextPayload`, `ToolStartPayload`, `UsagePayload`, etc.) plus
   `event_names()` constants. Frontend listens via `@tauri-apps/api/event`.
   The bridge from `QueryEngine::QueryEvent` to JSON happens here.

5. **`src/config.rs`** — `DesktopConfig` loaded from
   `~/.shannon/desktop/config.json` (separate from the engine's
   `~/.shannon/.shannon.toml` / `~/.shannon/config.toml`). MCP servers are
   persisted separately at `~/.shannon/desktop/mcp-servers.json`.

6. **`src/notifications.rs`** — `TauriNotificationHandler` implements the
   engine's `NotificationHandler` trait and drives
   `tauri-plugin-notification`. Registered once on `AppState.notifier` during
   setup via `attach_notification_handler(app_handle)`; all background tasks
   share the same cooldown + dedup state.

### Shannon engine

All `shannon_*` crates come from the sibling repo: `shannon-core`,
`shannon-types`, `shannon-tools`, `shannon-mcp`, `shannon-skills`,
`shannon-agents`. The desktop shell only adds IPC plumbing, persistence, and
UI — every reasoning/tool/MCP/skill behavior is delegated. When the pinned
rev bumps, expect wire-compatible schema changes (see `CHANGELOG.md`).

### Frontend (`ui/src/`)

- **React 19 + Vite 6 + Tailwind CSS 4 + react-router-dom 7**. Strict TS
  (`tsconfig.json` enables `noUnusedLocals`, `noUnusedParameters`,
  `noFallthroughCasesInSwitch`). `@/*` alias maps to `./src/*`.
- **`App.tsx`** — every route is `React.lazy`-loaded under a single
  `<Suspense>`. Layout wraps a sidebar + header shell; `/welcome` renders
  standalone. Legacy routes (`/strategic-focus`, `/agent-swarm`,
  `/quick-inject`, `/background-tasks`) redirect to current pages.
- **`context/AppContext.tsx`** — central state + actions. Every page reads
  chat, sessions, config, models, tasks, agents, MCP servers, background
  tasks through `useApp()`. Refresh functions (`refreshSessions`,
  `refreshConfig`, etc.) are the canonical way to re-pull after mutations.
- **`hooks/`** — `useTauriEvent` (typed event listener bridge),
  `useNotification`, `useKeyboardShortcuts`, `useTheme`,
  `scheduled-tasks.ts` (Tasks board data layer).
- **`lib/tauri-api.ts`** — one typed wrapper per Tauri command. Always go
  through this module rather than calling `invoke()` directly so types stay
  centralized.
- **`lib/mock/`** — `setupMockMode()` in `main.tsx` plus a vite alias swap
  of `@tauri-apps/api/core` → `coreMock.ts` when `VITE_MOCK_MODE=1`. The
  demo build runs without the Rust backend.
- **`i18n/`** — react-intl v7. Two locales: `en`, `zh-CN` (curated, not
  machine-translated). `useI18n()` returns `{ locale, setLocale }`. Read
  `ui/src/i18n/MIGRATION.md` before adding user-visible strings — the
  convention is `{feature}.{subsection}.{key}`, alphabetical within each
  block, and **both** `en.json` and `zh-CN.json` must be updated in the same
  change. Test setup auto-wraps every `render()` in `I18nProvider`.
- **`__tests__/setup.ts`** — globally mocks `@tauri-apps/api/core`,
  `@tauri-apps/api/event`, `@tauri-apps/plugin-dialog`, and the entire
  `@/lib/tauri-api` module with sensible defaults. Individual tests override
  per-case via `vi.mocked(...)`. `matchMedia`, `ResizeObserver`,
  `IntersectionObserver`, `scrollIntoView`, `getAnimations` are stubbed for
  jsdom.

## Conventions

- **Tauri command naming**: snake_case in Rust; the frontend invokes exactly
  the same name (`invoke('send_message', { message, filePaths })`). Argument
  names are camelCased by Tauri's auto-conversion on the JS side,
  snake_cased in the Rust signature.
- **Adding a new Tauri command**: define the `#[tauri::command]` fn in the
  appropriate module, add it to the `invoke_handler!` list in `main.rs`,
  add a typed wrapper in `ui/src/lib/tauri-api.ts`, add the matching type in
  `ui/src/types/index.ts`, and if it fires events, register payloads in
  `src/events.rs`.
- **Storage**: anything user-data lives under `~/.shannon/`. The desktop
  shell adds `~/.shannon/desktop/` for its own config and uses the engine's
  stores for everything else (`scheduled-tasks/`, `scheduled-runs/`,
  `plugins/`, `agent-messages/`, etc.). Never write outside `~/.shannon/`
  without an explicit reason.
- **Coverage**: `vitest.config.ts` enforces 80% lines / 60% functions / 75%
  branches / 80% statements. Files explicitly excluded are listed there.
- **CHANGELOG.md** is per-sprint, grouped by category (Features, Fixes,
  Accessibility, i18n, Dependencies). When bumping the engine pin, record
  what changed in the engine and why.
