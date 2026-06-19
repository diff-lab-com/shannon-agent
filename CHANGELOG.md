# Changelog

All notable changes to Shannon Desktop are documented here. Entries are grouped by sprint and category.

## v0.3.5 (2026-06-19) — S2 P0–P2 landing

Six PRs merged into `dev` covering the S2 P0–P2 sprint scope. CI
slimmed to UI-only after the Gitea runner could not reliably reach
github.com for action checkout / sibling fetch; Rust checks moved to
`scripts/local-check.sh` as a local pre-merge gate.

### Tooling

- **P0.3–P0.5: Gitea CI slim-down + `rust-toolchain.toml` + ADR-0001 (PR #1 + #6).**
  `rust-toolchain.toml` (channel `1.88`, profile `minimal`, components
  `rustfmt`/`clippy`) is now the single source of truth for the toolchain.
  CI workflow `.gitea/workflows/ci.yml` runs only the `ui` job (pnpm install
  + lint + vitest) on the `ubuntu-22.04` self-hosted runner — Rust jobs
  (`rust-test`, `rust-clippy`, `rust-fmt`, `cargo-deny`) were removed because
  the runner cannot reliably reach github.com for `actions/checkout` and the
  shannon-code sibling fetch. ADR-0001 lands the positioning + CI + engine
  distribution decision record.
- **Local cargo gate.** `scripts/local-check.sh` runs `cargo fmt --check`,
  `cargo clippy -D warnings`, `cargo test`, `cargo deny check`, then UI
  lint + vitest. Run before merge to `main`/`dev`.
- **i18n key-parity CI (P1.3, PR #2).** `scripts/check-i18n-parity.mjs`
  diffs keys between `en.json` and `zh-CN.json` and fails on mismatch. Runs
  as a separate `i18n-parity` CI job.

### Features

- **Featured install flow i18n + granular progress (P1.4 phase 1, PR #4).**
  Extensions Hub Featured surface now ships full en + zh-CN strings for
  install progress (cloning → building → registering), error toasts, and
  the trust-badge tooltip. Phase 2 (OAuth loopback for MCP-remote servers)
  deferred to a follow-up session.

### Security

- **CSP hardening + Markdown sanitize (P2.4, PR #3).** Production CSP
  drops `'unsafe-inline'` from `script-src`. `Markdown.tsx` now runs
  `rehype-sanitize` on rendered LLM output, closing an XSS vector where a
  malicious model response could inject arbitrary HTML into the chat view.
  External links continue to use `tauri-plugin-shell` → system browser
  (unaffected by CSP). `style-src 'unsafe-inline'` retained (Tailwind 4 +
  pervasive inline styles — separate cleanup).

### Docs

- **User-facing docs (P2.3, PR #5).** `docs/user/{README,getting-started,features}.md`
  written for a general (non-coder) audience. `README.md` gains a "For
  users" pointer block at the top so the project's primary README opens
  with consumer-facing context, not contributor context.

### Version sync

- `Cargo.toml`, `tauri.conf.json`, `ui/package.json` bumped 0.3.2 → 0.3.5
  to match this entry (prior releases shipped 0.3.2 binaries under a 0.3.4
  changelog header — drift now closed).

### Known issues

- 6 merged remote branches (`s2/p0-cleanup`, `s2/p1.3-i18n-parity`,
  `s2/p2.4-csp-sanitize`, `s2/p1.4-mcp-1click`, `s2/p2.3-user-docs`,
  `s2/ci-fixes`) pending deletion — `git push origin --delete` hangs on the
  SSH path from the developer machine; clean up via Gitea web UI at
  convenience.

## v0.3.4 (2026-06-19) — R2 scope reduction (partial)

R2 of the four-sprint plan, partial delivery. The two remaining R2 items
(commands.rs split, full unwrap cleanup) deferred to follow-up sessions
because each is a multi-hour focused refactor with regression risk.

### Tests

- **R2-A1: Flaky HOME-env tests fixed.** `extensions::security::tests::remove_report_drops_matching_entries` and `extensions::skill_installers::tests::list_installed_skills_returns_plugin_subdirs` (plus 6 sibling tests in the same files) no longer mutate `std::env::HOME` via `unsafe`. New pattern: thread-local override + RAII guard (`set_test_reports_home` / `set_test_skills_root`). `reports_path()` and `shannon_skills_root()` check the thread-local first, fall back to `dirs::home_dir()` in production. Eliminates the cross-file race that forced `--test-threads=1`. Verified stable across 3 consecutive `cargo test --lib` runs at default parallelism (296/296 each).

### Security

- **R2-A2: `withGlobalTauri: false`.** Tauri v2 exposes `window.__TAURI__` to all JS (including any XSS payload) when this flag is `true`. With CSP also tightened in R1, this closes another vector. Audit: grep across `ui/src/` found only a TS declaration in `vite-env.d.ts` (no runtime usage). All actual IPC already goes through typed `@tauri-apps/api` imports. UI tests: 884/884 still pass.

### Deferred to follow-up sessions

- **R2-A3 commands.rs split** — 5527-line god-file needs per-domain extraction (chat / sessions / config / mcp / agents / files). Multi-hour focused work; start with one cohesive domain as template.
- **R2-A4 unwrap cleanup** — 317 `.unwrap()` calls across the tree, concentrated in `mcpb.rs` (41), `lsp_commands.rs` (47), `mcp_installers.rs` (37), `commands.rs` (32). Priority: unwraps on user-input paths in Tauri command handlers (can panic in production).

## v0.3.3 (2026-06-19) — R1 industrialization baseline (CI-only scope)

Sprint R1 scoped down to CI-only after the Gitea pivot and "先解决自动化CI"
decision. Updater wiring, release pipeline, and Dependabot were drafted and
then reverted — see "Deferred" below.

### Tooling

- **R1-A1: CI workflow (Gitea Actions).** Added `.gitea/workflows/ci.yml` gating push/PR on `main` and `dev`. Jobs: `rust-test` (cargo nextest), `rust-clippy` (`-D warnings` with shannon-code's allow list), `rust-fmt` (`cargo fmt --check`), `cargo-deny` (advisories + licenses + bans), and `ui` (pnpm install + lint + vitest). Runs on the `ubuntu-22.04` self-hosted runner label. CI failure blocks merge. Migrated off GitHub Actions syntax (original `.github/workflows/` draft) after the project standardized on `gitea.diff-lab.com`.
- **R1-A2: cargo-deny policy.** Added `deny.toml` mirroring `shannon-code/deny.toml`: same advisory ignore list, same license allowlist (MIT/Apache/BSD/ISC/MPL/Unicode-3.0/CDLA-Permissive-2.0), same source allowlist plus `ssh://git@github.com/shannon-agent/shannon-code.git` for the engine subpath dep.
- **R1-A3: Version sync.** Bumped `Cargo.toml`, `ui/package.json`, `tauri.conf.json` from 0.3.1 → 0.3.2 to match the v0.3.2 changelog entry. Previous releases shipped a 0.3.1 binary labeled "v0.3.2" in notes.
- **R1-A8: CODEOWNERS.** Added `.gitea/CODEOWNERS` marking `.gitea/workflows/`, `tauri.conf.json`, `Cargo.toml`, and `docs/RELEASING.md` as `@ericdong`-owned. First line of defense against workflow tampering once signer secrets land. Wire up via Gitea branch protection → "Require code owner review" on `main` and `dev`.

### Deferred to a future sprint

The following R1 candidates were drafted then scoped out — preserved in
`docs/RELEASING.md` as the agreed design:

- **R1-A4 (Tauri updater wiring) — deferred.** `endpoints: []` and `pubkey: ""` stay empty. Plan: wire to an S3 + CDN distribution channel (token-free public read for the updater manifest, preferred over Gitea Releases for a private repo) once the signing keypair + bucket exist.
- **R1-A5 (Release pipeline) — deferred.** Drafted `.github/workflows/release.yml` using `tauri-apps/tauri-action@v0`, then removed it along with the rest of `.github/`. Release remains a manual `cargo tauri build`.
- **R1-A7 (Dependabot) — deferred.** Removed `.github/dependabot.yml`. Renovate (Gitea-compatible) is the planned replacement for cargo/npm/action-version drift PRs.

### Tests

- **R1-A6: Pure-function coverage gaps closed.** Added targeted unit tests for seven high-value helpers that previously lacked direct coverage:
  - `parse_approval_mode` (11 documented aliases + case-insensitivity + safe fallback)
  - `detect_media_type` (5 image MIMEs + case-insensitivity + non-image rejection)
  - `provider_from_str` (9 LLM providers)
  - `iso_days_ago` (format check + zero/negative edge cases)
  - `template_to_str` / `template_from_str` (8 known variants + custom roundtrip + unknown fallback)
  - `parse_trigger_type` (4 trigger kinds + case-insensitivity + rejection)
  - `is_completed_status` / `is_in_progress_status` (positive/negative matrices, with documented whitespace non-handling)
- 22 new test fns. Total Rust lib tests: **296** (295 passing in parallel; 1 pre-existing HOME-env-race flake in `skill_installers::list_installed_skills_returns_plugin_subdirs` passes under `--test-threads=1`).

### Docs

- **`docs/RELEASING.md`.** Prefixed with a "Deferred" status banner. The signing-keypair, secret-configuration, S3+CDN distribution, tag-push trigger, rollback, and troubleshooting sections are preserved as the design reference for when release automation is prioritized.

### Known issues (pre-existing, surfaced by R1)

- Two unit tests (`extensions::security::tests::remove_report_drops_matching_entries`, `extensions::skill_installers::tests::list_installed_skills_returns_plugin_subdirs`) flake under parallel test execution because they mutate `std::env::HOME` via `unsafe`. They pass under `--test-threads=1`. Fixing requires moving the helpers off `HOME` env var to an explicit path parameter — tracked as Sprint R2 followup.

## v0.3.2 (2026-06-18) — plugin marketplace browser + data source fetchers + triage + branching sessions

### Features

- **D1 — Plugin marketplace browser.** Replaced the placeholder Plugins tab with a real catalog browser backed by the existing `list_plugin_marketplace` command. Cards render per entry (name, author, description, trust badge, stars, license, version, source). Filter chips group by kind (MCP / Skills / Agents / Data Sources / Plugins), the kind picker narrows the list, and the layout search input drives client-side text search across name/description/tags/source. Typed `CatalogEntry` / `CatalogSource` / `TrustLevel` shared between Rust and TS via the existing extensions type surface. Full `en` + `zh-CN` parity for 22 new i18n keys.
- **D1.2 — Real install wiring + sort + auto-refresh.** Install button now actually calls `install_skill_from_repo` / `install_agent_from_repo` for skills/agents with `git_hub_repo` source (MCP/data_source still route to their dedicated tabs since their installers need form input). New sort dropdown: trust (default) / stars / name / recently updated — applied within each kind group. On successful install, a `shannon:extension-installed` window event fires and the Installed tab auto-refreshes.
- **D2 — 4 more data source adapters.** Added Notion, Linear, GitHub Issues, and Jira to the native data source catalog. Each ships as a declarative `DataSourceAdapter` with install-form fields (token + optional default scope), surfaced automatically in the Extensions → Data Sources tab. Catalog now exposes 6 adapters total (Obsidian, Email IMAP, Notion, Linear, GitHub Issues, Jira).
- **D3 — Real HTTP fetchers for the 4 new data sources.** Each adapter now has a fetcher implementation: Notion (POST `/v1/databases/{id}/query`), Linear (GraphQL `/graphql`), GitHub Issues (REST `/repos/{owner}/{repo}/issues`), Jira (REST `/rest/api/3/search`). New `query_data_source(slug, query)` Tauri command dispatches to the right fetcher based on the `kind` field stored in `~/.shannon/data-sources/<slug>.toml`. Normalized `DataSourceResult` shape shared across all sources. Per-source error mapping (AuthError / RateLimited / UpstreamError).
- **P6 — Triage queue + branching sessions.** Sidebar Tasks entry now shows an unread triage count badge (30s polling via `list_triage_stats`). New `TriageDrawer` popover with filters (All/Unread/by kind) and per-row actions (mark read, archive, open linked). Branching sessions: new `branch_session(parent_id, branch_point)` Tauri command clones the first N messages from a parent into a new session, with `parent_id` and `branch_point` fields exposed on `SessionInfo`. Engine already supported these fields — this commit surfaces them end-to-end.

### Tests

- 9 UI tests for the marketplace browser (`Plugins.marketplace.test.tsx`): loading, empty, error, kind filtering, kind grouping, trust badges, license/version/stars rendering, homepage link, search-via-outlet-context.
- 3 UI tests for sort/install/auto-refresh: sort-by-stars ordering, install function call, window event dispatch.
- 4 Rust tests for the data source catalog: notion token field, jira required fields, snake_case serialization across all kinds, expanded adapter list assertion.
- 16 Rust tests for D3 fetchers (4 per source: deserialization, auth requirement, mapping, edge cases).
- 2 Rust tests for `branch_session` (basic branch with N messages, parent_id stored correctly).
- 2 UI tests for TriageDrawer + 1 for sidebar badge.

### Notes

- E2E Playwright coverage for the marketplace tab is deferred — the existing smoke test suite has stale assertions from the P1 navigation restructure that need fixing first.
- Engine pin remains at `00510a7` (already at remote latest).

## v0.3.1 (2026-06-18) — full-text session search + keyboard shortcuts + test coverage

### Features

- **C1 — Full-text session search.** `search_sessions` now also scans session messages for matches when the title doesn't match. Title hits rank first; content matches fill the rest (capped at 200 sessions/keystroke to bound cost). Chat sidebar debounces the backend call (250 ms) once the query is ≥ 3 chars; shorter queries stay on the client-side title filter for instant feedback. Backend errors fall back to the client filter so the UI never dead-ends.
- **C2 — Cmd/Ctrl+K quick session switcher.** CommandPalette session items now call `switchSession(id)` instead of just navigating to `/chat`, so the palette works as a real session switcher (already had keyboard arrow nav + Enter).
- **C2 — Cmd/Ctrl+D change working directory.** New `mod+d` shortcut dispatches a window-level `shannon:change-wd` event. The Chat page listens via a ref and opens the same native folder picker the WD chip button uses — so users can repoint the WD without leaving the keyboard.

### Fixes

- **KeyboardShortcutsHelp missing translations.** The dialog was rendering raw key strings (`shortcuts.help.goChat`, etc.) for 7 of its 10 rows because those keys didn't exist in `en.json` / `zh-CN.json`. Added all 11 `shortcuts.help.*` keys (including the new `changeWorkingDir`) at full parity.

### Tests

- **B2 — coverage for new Tauri commands.** 12 new tests: 7 for the per-session WD chip in `Chat.test.tsx` (placeholder, breadcrumb, config fallback, folder picker, cancel-noop, session-list hint, provider pill), 5 for the `InboundSection` of `NotificationsSettings.test.tsx` (render, prefill, save wiring, empty-token omission, clear). Mocks for `setSessionWorkingDir` + `get/save/clear_inbound_config` added to `setup.ts`. Total: 819 tests across 76 files, all passing.

## v0.3.0 (2026-06-18) — navigation restructure + per-session working dir + Extensions Hub + i18n audit

### Features

- **P1 — Sidebar navigation restructure.** Sidebar now exposes only Chat + Scheduled as top-level entries. Automation, Goals, Triage, and Mission Control removed from primary nav; Tasks moved to internal tabs within Scheduled. Cuts the surface area to what the redesigned chat-first flow actually uses.
- **P2 — Chat right context panel + inline QuickFix/Editor.** Chat page gains a right-side context panel (token usage with context-window bar, active tools with live status, file context chips). QuickFix and Editor — previously top-level routes — are now inline modals launched from the chat input toolbar. Lazy-loaded so the main chat bundle stays small.
- **P3 — Scheduled Sprint 2 form.** Dual-mode schedule input at the top of `ScheduleForm`: natural-language ("every weekday at 9am") parses to a cron preview, plus the raw cron editor underneath for power users. Picks up shannon-code engine B6 (event-triggered routines) + B9 (per-task worktrees) wire-compatible schemas.
- **P4 — Extensions catalog expansion + Models quick-setup presets.** MCP registry installer surfaces a `tool_count` badge on installed servers. New Models quick-setup cards for Deepseek / GLM / MiniMax / OpenAI / Kimi — each card takes an inline API key and calls `switchProvider` with the right `base_url` + `model`.
- **P5 Phase 1 — Slack + Telegram inbound config storage.** `NotificationsSettings` gains an Inbound section for Slack (bot_token + trigger_word + allowed_channels) and Telegram (bot_token + trigger_word + allowed_chats). Persisted to `~/.shannon/desktop/config.json` under `[notifications.inbound]`. Listener (Phase 2) not yet wired — this commit only lands storage + UI.
- **Per-session working directory.** `SessionMeta` gains an optional `working_dir`. New `set_session_working_dir(id, path)` Tauri command canonicalizes the path, updates session metadata, syncs the process cwd when the session is active, and emits `CONFIG_UPDATED`. `switch_session` restores the process cwd from session metadata so each conversation remembers its own project root. Chat page exposes this via a header strip breadcrumb chip with native folder picker.

### Fixes

- **P0 — timestamp millis.** Engine contract uses Unix epoch milliseconds for `chrono_timestamp()`. Desktop was passing through seconds, which broke every "x minutes ago" computation in the UI. Now serializes millis consistently across `QueryEvent` payloads and session metadata.
- **P0 — MCP registry wrapped shape.** `list_mcp_registry_servers` was unwrapping a paginated envelope. Now returns the inner `servers[]` array the frontend expects.
- **P0 — billing demo data.** `get_billing_plan` / `get_cost_history` / `get_billing_history` were stubs returning empty; now return demo data so the Usage & Billing page renders meaningfully out of the box.
- **P0 — Advanced rename.** Renamed the Advanced Settings subpage (was mislabeled "Performance" in nav) so labels match across sidebar, page header, and route.
- **P0 — `/perf` route removal.** Legacy performance route still linked from a stale nav entry. Removed the route and all inbound links.
- **Chat page visual refresh.** New header strip shows session title + working directory breadcrumb + provider/model pill. Subtle ambient backdrop for depth. Session list items show working-directory hint when set. Refined spacing and typography on message bubbles.

### Accessibility

- **T9 — WCAG AA on NotificationsSettings** (carried over from v0.2.9). Loading spinners have `role="status"` + `aria-live="polite"`. Form fields use associated `<label htmlFor>` elements. Show/hide secret button exposes `aria-label`. Save/Clear states communicated via disabled + label change.
- **Per-session WD chip** keyboard-accessible: focus-visible ring, aria-label, native folder picker.

### i18n

- **P7 — McpServers + Installed bilingual coverage.** Both Extensions Hub tabs now have every user-visible string wrapped with `intl.formatMessage`. ICU message fix: `extensions.installed.count` was using `{filtered.length}` (invalid — ICU doesn't allow dotted variable names); restructured to use plain `entries` / `categories` variables.
- **GeneralSettings audit.** Header, subheader, all 5 approval modes (label + description), session info labels (Provider / Active Provider / Model / Working Directory), toast messages ("Approval mode: X", "Failed to update approval mode") — all bilingual.
- **Plugins tab audit.** Previously fully English. Now wraps title, description (with rich-text `<code>` via FormattedMessage), "Coming in P3" preview, "Soon" badge, entries count, and all 4 placeholder repo descriptions.
- **OPCKanbanBoard variant cards.** Critical / Review / Done / Archived / In Progress / "Proposed by {name}" / "Assigned to {name}" — all localized in both blocked, active, done, failed, and default card variants.
- **TaskDAGView legend.** Completed / Running / Pending legend + "Click any task to view details" hint localized.
- **ExtensionsHub detail pane.** Close button, "No description available." fallback, "Trigger: X" label localized.
- **Chat beautification i18n.** 12 new `chat.header.*` and `chat.session.workingDirHint` keys for the per-session working directory UI.
- All keys at full parity between `en.json` and `zh-CN.json` (zero missing on either side).

### Dependencies

- shannon-code engine remains at v0.5.5 (no rev bump this sprint — see Cargo.toml `[patch."ssh://..."]` block for the path-dep override).

## v0.2.9 (2026-06-17) — webhook config UI + load fix + i18n

### Features

- **Webhook config UI (C7).** New `NotificationsSettings` page in Settings lets users configure webhook delivery from the desktop UI instead of editing `.shannon.toml` by hand. Form fields: URL, template dropdown (Slack / Discord / Feishu / WeChat Work / Microsoft Teams / Telegram / DingTalk / Raw / Custom JSON), optional shared secret (show/hide toggle), `timeout_ms`, and `include_body` switch. Save writes the merged config back to `.shannon.toml` via `toml::Value` read-modify-write so unrelated tables are preserved. Clear removes the `[notifications.webhook]` section. Sidebar entry added under Settings.
  - 3 new Tauri commands: `get_webhook_config`, `save_webhook_config`, `clear_webhook_config`. Config path resolution prefers `.shannon.toml` then falls back to `~/.shannon/config.toml`.
  - 6 vitest tests covering loading state, empty defaults, prefill from saved config, save blocked on empty URL, save dto shape, and clear behavior.

### Fixes

- **Desktop webhook config actually loads.** Reused the shannon-code v0.5.4 fix: replaces `ConfigBuilder::load_local_toml()` (skipped nested `[notifications.webhook]` table) with `toml::from_str::<ShannonConfig>` direct parse. `.shannon.toml` first, `~/.shannon/config.toml` fallback.

### Accessibility

- **WCAG AA on NotificationsSettings (T9).** Loading spinner has `role="status"` + `aria-live="polite"` + sr-only text. All form fields have associated `<label htmlFor>` elements. Show/hide secret button has `aria-label`. Save/Clear button states (saving/clearing) communicated via disabled state + label change.

### i18n

- **26 new translation keys** added to both `en.json` and `zh-CN.json`: `nav.notifications`, `settings.notifications.{title, subtitle, loading, url, template, templateCustom, customBody, customBodyHint, secret, secretPlaceholder, secretHint, timeoutMs, includeBody, save, saving, clear, clearing, saved, cleared, toggleSecret, restartHint, error.urlRequired, error.saveFailed, error.clearFailed}`.

### Dependencies

- **shannon-code engine bumped to v0.5.5.** Picks up C9 (Teams/Telegram/DingTalk templates), T7 (retry/backoff + 5s default timeout), T5 (Quiet/Balanced/Verbose presets), T2 (permission prompt notification), T3 (agent exit notification). The desktop webhook config UI reads/writes the same `[notifications.webhook]` schema so future core changes stay wire-compatible.

## v0.2.8 (2026-06-17) — notifications next phase (Bundle A + Bundle B)

### Features

- **Webhook sink wiring.** `AppState::attach_notification_handler` now conditionally attaches a `shannon_core::notifier::WebhookHandler` when `[notifications.webhook]` is configured in `.shannon.toml`. Config is loaded best-effort via `ConfigBuilder`; init failures are logged at `warn` level and never panic the app. Surfaces six templates (Slack / Discord / Feishu / WeChat Work / custom / raw) with optional HMAC-SHA256 signing. Fires are async and fire-and-forget so a slow endpoint can't block the UI.
- **Click-to-foreground.** `main.rs` listens for `notification-clicked` Tauri events and calls `unminimize + show + set_focus` on the main window. macOS and Windows already focus the app natively via bundle-id behavior; this listener is a defensive fallback for Linux DEs and any future Tauri plugin versions that route desktop clicks here.

### Dependencies

- **shannon-code engine bumped to v0.5.3** (`a19a15d` → `c5a107e`). Picks up the `WebhookHandler` core types (six templates + HMAC signing) used by the new desktop wiring.

## v0.2.7 (2026-06-17) — notifications feature complete

### Features

- **Query lifecycle → notification wiring.** Desktop now fires OS notifications when a query completes (`source="query_complete"`, 0ms window — always fires) or fails (`source="query_error"`, 5000ms window — coalesces cascading errors). Routes through the shared `Notifier` on `AppState` so cooldown + level filtering apply, instead of bypassing them.
  - `src/notifications.rs`: `TauriNotificationHandler` bridges `shannon_core::notifier::NotificationHandler` to `tauri-plugin-notification` via a cloned `AppHandle`. Single handler instance serves all background tasks (AppHandle is Send+Sync).
  - `AppState.notifier: Arc<Notifier>` — populated once in `main.rs` setup via `attach_notification_handler`. Empty by default in tests.
  - `fire_query_notification(notifier, kind)` — takes `Completed | Failed(String)`, builds a `Notification` with the right source/level, calls `notify_dedup(window_ms)`. Body truncated to 200 chars to avoid notify-send / Windows toast overflows.
  - 3 unit tests: completed always fires (0ms window), failed coalesces within 5s window, long body truncation safe.
- **"Send test notification" button in General Settings.** Frontend `useNotification()` hook drives the `send_notification` Tauri command directly (bypasses the orchestrator for UX verification).
  - 8 i18n keys added to `en.json` + `zh-CN.json`: `settings.notifications.{label, help, testButton, sending, testTitle, testBody, testSent, testFailed}`.
  - 3 vitest tests for the hook (basic invoke, level pass-through, callback identity stability).

### Dependencies

- **shannon-code engine bumped to v0.5.2** (`ede2105` → `a19a15d`). Picks up the P1 notifications orchestrator (`Notifier`, `Cooldown`, `NotificationsConfig`, `Notifier::notify_dedup`, `Notification::source/action_id`).

## v0.2.6 (2026-06-17) — native notification renderer

### Features

- **Native OS notifications (Phase 3 of cross-repo notifications feature).** Desktop now fires system-level notifications via `tauri-plugin-notification`. The Rust side exposes a `send_notification` Tauri command wrapping the plugin's builder API; the frontend side exposes a `useNotification()` React hook that calls it.
  - `Cargo.toml`: `tauri-plugin-notification = "2"` added as optional dep, gated behind the existing `tauri` feature.
  - `src/main.rs`: plugin registered on the Tauri builder; command registered in `invoke_handler!`.
  - `src/commands.rs`: `send_notification(AppHandle, NotificationPayload { title, body, level })` builds and shows a single notification via `NotificationExt::notification()`.
  - `ui/src/hooks/useNotification.ts`: stable-callback hook wrapping `invoke('send_notification', ...)`.
  - `ui/src/__tests__/useNotification.test.ts`: 3 vitest unit tests (basic invoke, level pass-through, callback identity stability).
  - `src/commands.rs` tests: 2 Rust unit tests for payload deserialization.

**Note on integration scope.** Desktop's pinned `shannon-core` rev (`ede2105` = v0.5.1) predates the P1 notifications work in shannon-code (Cooldown, NotificationsConfig, Notifier pipeline). The v0.2.6 release ships the rendering surface only — it does NOT consume `NotificationsConfig` or apply per-source cooldown yet. A future release that bumps the pin past P1 will wire the full pipeline transparently; the frontend hook signature stays stable.

## v0.2.5 (2026-06-17) — engine sync to shannon-code v0.5.1

### Dependencies

- **shannon-code engine bumped to v0.5.1** (`e63ae82` → `ede2105`). All six shannon-* crates (core, types, tools, mcp, skills, agents) now track the v0.5.1 tag, picking up:
  - **Sprint 5 MCP integration**: elicitation TUI (bounded mpsc + spoofing-resistant `[EXTERNAL MCP · {server}]` labeling), MCP prompts as `/{server}:{prompt}` slash commands, `completion/complete` Tab autocomplete, `.mcpb` bundle CLI install.
  - **v0.5.1 security hardening on `.mcpb` install**: symlink path traversal rejection, 10 MB manifest size cap, parse-error data-loss prevention, install preview + `[y/N]` confirmation flow.

## Sprint 5 (2026-06)

### i18n — Phase 5 Long-tail Components (S5-1)

- **17 sub-components migrated**: WelcomeState, Header, ExtensionsHub (agents / datasources / skills loading states), Chat (session export / print aria-labels), MissionControl tab labels, DependsOnEditor aria, LspQuickFixPanel (ICU plural for edit count), AgentMessagesPanel (empty states + team-scoped messages), HistoryView, Goals (active agents / suggest next steps / summarize progress), Perf analyzer, HookTaskPipeline, ErrorBoundary, OPCTask, Editor diagnostics.
- **Source-side fixes**: 5 edits for duplicate IDs (e.g. `loadingInstalled` variant alongside `loading` for installed-extensions spinner vs catalog spinner) and ICU placeholder mismatches (e.g. `lsp.quickFix.applies` now takes `{title, count}` so "Applied: Prefix with _ (2 edits)" renders correctly).
- **Locale parity**: en.json + zh-CN.json both at **1373 keys** with identical ordering. Curated translations (no machine translation). 60+ zh-CN entries added; 60 dead zh-only keys removed.
- **Test setup upgrade**: `setup.ts` auto-wraps `@testing-library/react` render with `<I18nProvider>` globally — individual tests no longer need manual wrapper boilerplate. Existing tests that already wrap are unaffected.
- **Final state**: 806 / 806 UI tests pass. `pnpm tsc --noEmit` clean. i18n migration complete (only `__tests__/` helper files remain unmigrated, which is correct).

## Sprint 4 (2026-06)

### Command Palette Enhancements (S4-3)

- **Fuzzy subsequence matching**: Palette now scores all items via contiguous-match and word-boundary bonuses, ranking the best matches first. Queries like "mc" surface "Mission Control" even though the letters aren't contiguous.
- **Model switch refresh**: Selecting a model in the palette now triggers `refreshConfig()` so the new provider/model propagates immediately to the footer and chat header — no manual reload.
- **Shortcut hints + result count**: Palette footer shows live result count with ICU plural formatting and a keyboard-shortcut legend (`↑↓ navigate ↵ select esc close`).

### i18n — Phase 3 Chat Migration (S4-1)

- **Chat.tsx**: Full migration of the 693-line chat surface — session sidebar (search / pin / export / print aria-labels), message bubbles (branch / copy / like / regenerate), streaming indicators (thinking text), input bar (placeholder / attach / send / stop), delete modal, context panel (usage / active tools / attached files). ~45 new message IDs following `chat.{section}.{key}` convention. MessageBubble and ToolCallDisplay subcomponents each get their own `useIntl()` call.

### i18n — Phase 4 Long-tail Components (S4-2)

- **Batch 1**: Extensions, Routines, Hooks, Profiles pages fully migrated with per-page locale namespaces.
- **Batch 2 (13 components)**: Triage, MissionControl, OPC, OPCTask, Goals, Perf, QuickFix, Editor, Settings, Header, WelcomeState, KeyboardShortcutsHelp, ErrorBoundary. ~150 new locale IDs added to en.json and zh-CN.json with natural Simplified Chinese translations.
- **ErrorBoundary refactor**: Class component couldn't call `useIntl()` hook — split into `ErrorBoundaryInner` (class) + functional wrapper that calls the hook and passes `t()` down as a prop. Reusable pattern for any class component needing i18n.
- **Test infrastructure**: 13 test files updated to wrap rendered components with `<I18nProvider>` from `@/i18n`. Established as the standard test pattern going forward.

### Maintenance

- All 806 UI tests passing. `pnpm tsc --noEmit` clean. No new clippy warnings.
- **#74 README screenshots**: Explicitly declined — documentation refresh deferred until feature work stabilizes.
- **Phase 5 sub-components deferred**: Sub-components under `ui/src/components/{conversations,diff,editor,extensions,lsp,opc,settings,tasks,shared,ui}/` will be migrated in a future phase.

## Sprint 3 (2026-06)

### Command Palette (G3)

- **G3 Palette MVP**: Quick-actions palette (iOS Shortcuts style) with ⌘K trigger, fuzzy search across actions / pages / settings / recent chats / tasks / agents / model switching. Category-grouped results, keyboard navigation.

### Extensions Hub Tier-3 (Sprint 3-11)

- **#71 Stdio form**: Native stdio MCP server config form (command, args, env) in Extensions Hub → Add → stdio, replacing raw JSON editor. Validates required fields, shows preview of generated `.mcp.json` entry.

### Theme Manager (Sprint 3-10)

- **#72 Theme picker**: Dedicated theme settings page with live preview, search, and visual swatches for all 12 themes. Replaces inline dropdown.

### i18n (#73 + #75)

- **#73 Phase 1 Infrastructure**: react-intl v7.1.14 setup with IntlProvider, useIntl hook, locale state with localStorage persistence (`shannon.locale`). Language switcher in Settings → General. en + zh-CN locale files. Welcome.tsx migrated as reference.
- **#73 Phase 2 Core surfaces**: Migrated Sidebar (nav labels / mode toggle / aria-labels / titles), Layout footer (ICU plurals for tokens / sessions / tasks / agents with locale-aware number grouping), CommandPalette (actions / pages / categories / toasts), Tasks (tabs / toasts / aria). ~85 new message IDs. Chat.tsx deferred to Phase 3.
- **#75 Sample seed**: `seed_sample_data` Tauri command populates demo conversations / tasks / agents / routines on first run. Wired into Welcome.tsx finish() for new users.

### Security (D1)

- **D1 README scan**: SecurityBadge now scans extension README alongside description for prompt-injection patterns. Catches malicious instructions buried in installation / usage docs.

### Maintenance

- **C1 Test fix**: `scan_with_readme_truncates_long_body_safely` corrected — scanner counts distinct patterns (not occurrences), so test now uses 3 distinct patterns to escalate to Dangerous.
- **C2 Clippy**: Documented pre-existing warnings (exit 0 — no new warnings introduced).
- **TS cleanup**: Fixed 3 pre-existing TypeScript errors in App.tsx / McpServers.tsx that were out-of-scope during Phase 1.
- **#75 deferred**: Sample seed data NOT auto-seeded for existing users (only new installs via Welcome).

## Sprint 2 (2026-06)

### Extensions Hub (P1–P6)

- **P1 Unified hub shell**: Catalog schema + `AddonInstaller` trait + 4 sub-tabs (Featured / MCP Servers / Skills / Agents / Installed). Installed tab fully wired to backend installer with progress + rollback.
- **P2 MCP marketplace**: MCP Registry browser, OAuth 2.1 PKCE flow for remote MCP servers (Notion / Linear / Slack / GitHub / Gmail), `.mcpb` bundle install for stdio servers. `McpRegistryClient` discovers packages, `mcp_installers` resolves and installs.
- **P3 Skills marketplace**: Federated skill catalog (local + community + plugin sources), marketplace installer with conflict detection.
- **P4 Agents marketplace**: Federated agent catalog, marketplace installer honoring `.claude/agents/*.md` + `.shannon/agents/*.toml` precedence.
- **P5 Native data sources**: Obsidian vault adapter (markdown + frontmatter) + Email IMAP adapter, surfaced as catalog entries alongside MCP / Skills / Agents.
- **P6 Security hardening**: Prompt-injection scanner (`scan_prompt_injection`) with risk classification (Clean / Suspicious / Dangerous), signature verification on `.mcpb` bundles, persisted security reports store. `SecurityBadge` shows risk chip on every community / unknown catalog card.

### Themes (G4 + D2 + D4)

- **G4 Multi-theme**: 3 dark themes added — Solarized, Dracula, Gruvbox — on top of existing Material / Tokyo Night / Tokyo Night Light / Catppuccin / Nord / Ember / Slate.
- **D2 Light variants**: Solarized Light + Gruvbox Light added. Total 12 named themes + System.
- **D4 WCAG AA compliance**: Adjusted primary / on-primary contrast on Solarized (4.08 → 6.89), Solarized Light (3.41 → 5.71), Gruvbox Light (3.33 → 5.57). All 5 new themes pass AA normal-text threshold.

### Conversations (F5)

- **F5 Today view default**: Conversations now defaults to Today tab (was: All). Last selected tab persists to `localStorage` (`shannon-conversations-tab`), restored on next mount. Today aggregates today's chats + due-today tasks + WAC metric + running agents.

### Maintenance

- **A3 reverted**: Removed dead `result_routing` field from `scheduled_routines.rs` `ExecutionPolicy` before merge — never wired in UI, hooks, or tests.

## Sprint 1 (2026-06)

### Brand & Welcome

- **W1 Brand rename**: shannon → Shannon in all user-facing copy; display-text cleanup across UI.
- **W2 Welcome wizard**: 4-step goal-oriented flow (folder → goal → theme → done). Replaces single-prompt folder picker.
- **A3 Developer opt-in**: Welcome "Done" step exposes developer-mode toggle; opted-in users unlock the Board tab + dev sidebar.
- **H2 Welcome dedup**: Verified the duplicate paragraph bug in Welcome step 2 is fixed.

### Navigation & Layout

- **A1 Automations top-level**: Promoted /routines + /hooks + /profiles to a top-level nav group (was nested).
- **W3 Sidebar dual-mode**: Simple mode (default) hides dev-only routes; Dev mode (toggled in Welcome or sidebar) unlocks Board / Perf / Quickfix. Persisted to `localStorage` (`shannon-sidebar-mode`).
- **C3 Legacy redirects**: Old routes (/ops, /agent-load, /exec-mode) redirect to new homes.

### Conversations & Chat

- **B2 North-star WAC**: Today dashboard surfaces Weekly Active Conversations as the headline metric.
- **C1 Conversations filters**: Status filter tabs on Conversations list.
- **C2 Triage bulk ops**: Per-item Delete + multi-select Delete on Triage.
- **F1 Attach fix**: Tauri native file dialog replaces dead button; PDF / image upload now works.
- **F2 Export conversation**: Markdown + PDF export via Tauri save dialog.
- **F4 List as default**: Conversations list is the primary view; Board demoted to dev-only tab.
- **A4 Chat templates**: Refreshed empty-state templates — Email, Summary, Research, Code.
- **W4 Today dashboard**: Today / All / Board tabs + searchable conversations list + Today dashboard with WAC + running agents.

### Billing

- **A2 Demo mode banner**: Promoted demo-data notice to top-of-page alert on /billing.

### Documentation

- **D1 Repositioning**: README rewritten as "Your AI Workspace" narrative.
- **D2 Tasks vs Mission Control**: Architecture doc clarifying scope distinction between Tasks (single-unit work) / Mission Control (aggregated kanban) / OPC (operations center).
- **D3 Repositioning integration**: Core arguments from D1 propagated to landing copy.

### Editor (Phase E)

- **E1 Code editor**: CodeMirror 6 with manual diagnostic squiggles; click opens quick-fix drawer.
- **E1 v2 Auto-diagnostics**: Editor auto-fetches `publishDiagnostics` on file load via LSP `did_open` + diagnostic collection.
- **E5 Performance**: `tracing-subscriber` init + JSON exporter; 9 commands instrumented. Criterion bench at 100 / 1k / 10k task scales (~11M tasks/sec).
- **E4 Hook audit**: Audited 30 Shannon vs 30 Claude Code hook events; identified 5 dead events to wire later. +4 fixture tests.

### Phase D Features (already shipped pre-Sprint 2)

- **C3 Agent message history**: Phase D C3 frontend — `AgentMessagesPanel` UI + Cargo.toml bump.
- **C4 Task dependencies**: `depends_on` editor on routine detail.
- **G6 / G9 / G10**: Task DAG, edit drawer, AgentLoadPanel.
- **G7 / G8 / G11 / G12**: Execution mode, assignee datalist, team filter, MissionControl kanban.
- **P3 Differentiators**: Natural-language cron, templates, hook pipeline, schedule DAG.
- **LSP panel**: `LspQuickFixPanel` full stack, fixed infinite re-render via `EMPTY_SERVER`.

## Earlier (Pre-Sprint 1)

### Desktop Split (Phase A–D)

- **Phase A**: shannon-desktop extracted to standalone repo; Cargo.toml pulls `shannon-*` via git subpath dep with `[patch]` override.
- **Phase B–D**: Tauri + React + Vite scaffold, MD3 design tokens, theme system, sidebar, agent / task / hook / profile pages.
