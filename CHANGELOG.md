# Changelog

All notable changes to Shannon Code are documented here. Entries are grouped by category.

## [Unreleased] — §4.14 W1-P2 · OTLP bridge + full RedactionPolicy + desktop Turn Timeline

### Added

### Phase 2 — autonomous-loop guard rails (feat/goal-phase2 + feat/goal-live-wiring)

- **Progress-based guard rails replace turn-count-as-guard**: `/goal`,
  `/loop`, and `/ralph` now share deterministic drift protection —
  anti-spin (2 consecutive no-tool turns → pause) and stall strikes
  (3-strike budget → pause with a re-planning hint), via the shared
  `repl::loop_guard` module. R15: goal defaults to **unlimited turns**
  (`--max N` is an explicit fallback); `/ralph` defaults to 100;
  `/loop` was always unlimited (plan doc corrected).
- **`goal_get` / `goal_update` tools** (Codex-spec contract): the model
  can report completion or blockers through structured tool calls;
  blocked requires a reason; pause stays user-owned. Wired live via
  `GoalShared` — the tools observe and transition the real goal during
  a query, and transitions are replayed, persisted, and surfaced at
  query completion.
- **`--budget $N` on `/goal`**: live budget signal from the billing
  store (spend since set/resume); exceeding it pauses the goal as a
  recoverable terminal (raise the cap or clear). Defaults off — no
  implicit termination.
- **Recursive-submit fixes**: all three loops queue their continuations
  through `submit_input`'s flat drain loop (O(1) stack depth) instead
  of nesting `handle_query` frames (stack-overflow hazard with
  unlimited loops).
- **Persistence**: active `/loop` and `/ralph` state now persists in
  the session sidecar and is restored by `/resume` / `--resume`.
- **`/ralph` completion hardening**: keywords match only the final
  non-empty line (substring-in-body no longer ends the loop); default
  cap raised 10 → 100; invalid `--max` falls back to the real default.
- Deferred: check-in backoff scheduling (needs a one-shot routine
  primitive), model self-reported progress classification.

- **`/goal` — session goal: a persistent objective with auto-continuation**
  (parity with Claude Code `/goal` and Codex CLI Goals; design + competitive
  research in `docs/plans/2026-09-04-goal-design.md` and
  `docs/research/2026-09-04-goal-competitive-research.md`): `/goal
  <objective>` sets a session-scoped goal that is injected as a non-cached
  system block on every query (survives compaction), auto-continues the
  agent across turns until the model ends a reply with a strict final-line
  completion marker (`GOAL_COMPLETE` / `GOAL_BLOCKED: <reason>`), and
  persists in the session sidecar so `--resume` / `/resume` restore it.
  Anti-runaway guards: iteration cap (`--max N`, default 25, `0` =
  unlimited) flipping the goal to paused, mutual exclusion with `/ralph`
  and `/loop`, and interruption stopping the loop (goal stays anchored).
  Status pill in the status bar (active ◎ / paused ⏸ / complete ✓), desktop
  notification on completion, `--goal` injection for headless `-p` runs,
  help overlay entry, and i18n across all 10 locales. Engine side:
  `QueryEngineConfig::goal` + `GoalSpec` + `set_goal` mirror the existing
  `/focus` pipeline; completion-marker constants
  (`GOAL_COMPLETE_MARKER`/`GOAL_BLOCKED_MARKER`) are shared from
  `shannon-core`. Deliberately deferred (Phase 2): Codex-style
  `get_goal`/`update_goal` tool contract, token/time budget accounting,
  check-in backoff scheduling, anti-spin (no-tool-call detection).

- **`write_files` plugin permission enforcement — "declaration IS sandbox"**
  (closes the last §4.9 scaffolding seam): a plugin manifest that declares
  `write_files` now gets its stdio server processes spawned **inside a
  manifest-derived execution world** at both spawn points (discovery +
  per-call cold spawn). Derivation
  (`PluginPermissionPolicy::spawn_sandbox_policy`): writable roots converge
  to the plugin install dir + the current workspace, everything else stays
  read-only, system binary roots stay executable, and network follows the
  `network` declaration. Linux installs a Landlock fork-init ruleset
  (fail-closed: a failed install aborts the spawn); macOS rides the existing
  Seatbelt bridge; anywhere the backend is missing the spawn chain degrades
  to legacy behavior with a loud `plugin/sandbox` warning — never a silent
  fake sandbox. Undeclared manifests keep byte-for-byte legacy spawns (the
  default-allow compat red line; the derivation is `None` for anything not
  explicitly declaring `write_files`). New pieces: `plugin::spawn_sandbox`
  (`PluginSpawnGuard`), `gated_discover_tools_stdio_guarded`,
  `discover_tools_guarded`, `shannon_tools::sandbox::{plugin_spawn_world,
  plugin_spawn_guard_for_manifest}`; REPL/CLI plugin loaders wired. E2e
  acceptance in `crates/shannon-tools/tests/plugin_spawn_sandbox_tests.rs`
  (kernel-refused out-of-root write vs. in-root success vs. undeclared
  compat control); author-facing semantics updated in
  `crates/shannon-core/src/plugin/PERMISSIONS.md`.
- **OTLP telemetry bridge** (`shannon-core::telemetry`): `telemetry.rs`
  rewritten from atomic counters into an L0→OpenTelemetry bridge. A pure
  `build_span_tree` folds a session's events into the
  `session → turn → tool` span hierarchy (explicit envelope
  `span_id`/`parent_span_id` win over structural ids; interrupted tool
  calls still render), and analytics-projection totals feed OTel counters.
  Traces go out via OTLP gRPC (`opentelemetry-otlp`, batch processor =
  background delivery); metrics export interval comes from the existing
  config fields — previously dead `endpoint` / `trace_export` /
  `metrics_export` are now wired, and `SHANNON_TELEMETRY` keeps its opt-in
  NOOP-by-default contract (nothing is constructed when off; sinks degrade
  instead of failing on unreachable endpoints). An in-memory receiver test
  asserts the exported span tree shape end-to-end.
- **Full RedactionPolicy** (`shannon-core/src/session_log/redaction.rs`),
  replacing the §4.2 minimal mask: built-in token prefixes (unchanged,
  fail-closed) + user extra prefixes / regexes / exact values loaded from
  `~/.shannon/redaction.toml` (override path: `SHANNON_REDACTION_TOML`) +
  env-secret value snapshot. Each `SessionTee` captures one immutable
  policy snapshot per query — masking stays strictly write-time, disk stays
  clean; an acceptance test scans a written log for injected plaintext.
- **Desktop Turn Timeline**: new `trace_timeline(session_id)` Tauri command
  serving `project_turn_timeline(events)` — the per-session L0 projection
  with turn windows, tool waterfall rows (paired call→result with measured
  durations, interrupted calls marked), and the token/cost cumulative curve.
  The `/timeline/:id` panel renders waterfall bars plus an SVG accumulation
  chart; reachable from every session row's ⋯ menu ("Turn Timeline").
  Mock-mode fixture + Playwright spec included.

### Changed

- Deps: `opentelemetry` 0.32 (+ `opentelemetry_sdk`, `opentelemetry-otlp`)
  added to `shannon-core` only; no workspace-level dependency changes.
- `scripts/otel-demo/docker-compose.yml`: one-command Jaeger (UI :16686)
  + Grafana (:3300) stack for accepting the span tree visually; usage in
  the telemetry module docs.

## [Unreleased] — §4.10 W3-2 · manifest v2 + install-time validation + `--dump-config` + ecosystem conventions

### Added


- **Plugin manifest v2** (`manifest_version = "2"`): MCP server references
  (`[[mcp]]` rows; the Claude `mcpServers` map parses into the same list),
  reserved hook-subscription declarations (`[[hooks]]`, validated against
  `HookEventType` at install time), a Shannon compat window
  (`[compat] min/max`), and the reserved `type = "wasm"` slot for the
  deferred §4.16 pilot (clear "reserved, cannot load yet" error instead of
  "unknown plugin type").
- **Install-time validation** shared by git/path/`.dxt`/`.mcpb` installs and
  plugin updates: structural schema checks plus permission-completeness —
  the faces a plugin's shape implies (stdio ⇒ `execute_commands`, remote ⇒
  `network`, tool routing ⇒ `mcp_tools`, command/skill entry reads + prompt
  turns ⇒ `read_files` + `llm_api`) must be declared. **v2 manifests refuse
  to install on gaps; v1/claude legacy manifests install with loud
  warnings**, keeping upgrade paths non-breaking.
- **`shannon --dump-config`**: prints the effective configuration as JSON
  with per-entry provenance. Layers render lowest → highest precedence
  (builtin → user-global `~/.shannon/config.toml` → project
  `.shannon.toml` → env-vars → connected `~/.shannon/providers.toml` →
  cli-overlay); each entry is annotated with the nearer high-precedence
  layer that overrides it (`overridden_by`) and its feeding env var where
  applicable. Golden-snapshot tested.
- **Ecosystem conventions doc**: `crates/shannon-core/src/plugin/ECOSYSTEM.md`
  — GitHub topic `shannon-plugin`, three authoring templates (skill /
  command / tool) in v2 TOML, v1-TOML / v2-TOML / claude-JSON reading
  matrix, and the install-validation rule list.

### Changed

- **Broken plugin manifests can no longer vanish silently**
  (`registry.load_all`). A directory holding a corrupt `plugin.toml` /
  `plugin.json` is now reported via an aggregated `LoadFailures` error that
  names every bad path and reason; all valid sibling plugins still load.
  Manifest-less directories remain benign skips. REPL/CLI load sites print
  the aggregated report as a warning.
- MCP references accept `stdio` transport rows without an explicit
  `type = "stdio"` (inferred default), matching hand-written shorthand.

## [Unreleased] — §4.6 W1-P1 · L0 becomes the only authoritative session record (breaking, DP4)

### ⚠️ Breaking changes

- **Sessions are now event-sourced.** The single-file session snapshot
  (`~/.shannon/sessions/<uuid>.json`) is gone. Every session's durable state
  lives in `<sessions>/<uuid>/events.jsonl`, and everything else — message
  history for `--resume` / `/resume`, token totals, listings, branches — is
  *derived* from that log at read time. Old `.json` snapshots are neither
  read nor migrated: delete them after upgrading. Titles survive via a small
  per-session sidecar (`<uuid>/meta.json`) holding only user-curation fields
  (title / branch lineage); model, timestamps, project path and token totals
  come from the log itself.
- **Transcript files discontinued.** `~/.shannon/transcripts/<sid>.jsonl` is
  no longer written. Full-text search and stats over past conversations are
  now pure functions over the event log (`session_log::search_events`,
  analytics projection), surfaced through the new `shannon trace` family.
- **Legacy recording fixtures replaced.** `crates/shannon-core/fixtures/sessions/*.jsonl`
  (RecordingEntry shape) were converted once into authoritative-format logs
  under `fixtures/session_l0/<name>/events.jsonl`; every fixture-driven test
  now reads them through the typed L0 reader. Tool-chain assertions are
  unchanged — same sequences verified on the new medium.
- **Analytics scatter collection removed.** The unused `AnalyticsStore`
  write path (zero producers/consumers found) is deleted; its eight
  aggregate dimensions live on as a derived projection
  (`project_analytics_jsonl`) bundled by `shannon trace export`.
- **Session-recording capture retired.** `shannon-core/src/recording/` +
  `vcr.rs` are removed; their LLM request/response capture role is fully
  superseded by always-on `request/header` rows carrying the exact wire body.
  Note this does NOT touch the engine wire-fixture hook
  (`SHANNON_RECORD_DIR`) used by `just record` / dogfood evidence scripts.

### Added

- **`shannon trace` subcommand family**: `show <session> [--turn N]
  [--tool X] [--permission]`, `replay <session>` (time-compressed rendering,
  chunks folded), `diff <a> <b>` (seq/kind/payload-digest comparison), and
  `export <session> [--out DIR]` (events + derived analytics + summary).
  Session references accept full UUIDs, unique prefixes, or `latest`.
- Restore path now projects conversation history from L0 via
  `session_log::project_conversation`, with a dedicated restore round-trip
  equivalence suite (`state_integration.rs`) proving
  write → process exit → re-enter → identical in-memory state.
- Engine tee writes into the sessions container owned by `StateManager`
  (still honoring a whole-root `$SHANNON_HOME` override), so redirected
  stacks (`SHANNON_SESSIONS_DIR`) resume from the same location they log to.

### Changed

- Headless runs no longer checkpoint per-turn JSON snapshots; the continuous
  event log makes crash-window tail recovery the resumption mechanism.

## v0.10.0 (2026-08-13) — memory curated layer (ADR-0010), ADR-0005 provider tail closed

### Added

- **Memory storage upgrade to append-only JSONL (ADR-0010, C2'-C5').** Memories now persist as append-only JSONL (`~/.shannon/memories/<project>.jsonl`) under a process-wide flock, replacing the single-shot JSON read/write. Injection is scoped per-project/per-category instead of search-based; write-time Jaccard dedup avoids near-duplicate entries; and a periodic compaction trigger (~24h wall-clock or ≥5 sessions) dedupes, drops stale entries, enforces per-category caps, and prunes the injected prompt to a ~2000-token budget. Compaction is multi-agent-safe — concurrent appends from other agents are reconciled under flock and preserved, and deliberately-deleted ids are not resurrected. (#62)
- **`fallback_models` editor in the Add Provider modal (ADR-0005 G4).** The desktop Add Provider modal's advanced section now has a list editor for `fallback_models`, mirroring the existing `extra_headers` editor; wired through `ProviderInput` → `apply_provider_update` + the `save_provider` insert branch. (#63)

### Changed

- **Retired the `switch_provider` desktop shim (ADR-0005 G6).** The vestigial `switch_provider` Tauri command is removed; the three frontend model-switch surfaces (`Header`, `CommandPalette`, `ModelsSettings`) now route to `configure({ key: 'model', value })`, the canonical store-mutating path. This also fixes a latent bug — `switch_provider` discarded its request argument, so picking a model from those dropdowns had been a no-op since P1.2-B. (#63)
- **Removed the legacy `providers.json` → `providers.toml` migration code (ADR-0005 G2/G3).** Shannon never shipped a release carrying the `providers.json` wire format externally, so the one-shot `migrate_providers_to_toml` startup migration, the `LegacyProviderConnection`/`LegacyProvidersFile` wire types, the `list_providers` empty-store stale-check, and the `IsolatedHome` test fixture are all deleted. No code path reads or writes `providers.json` now. (#61)

### Internal

- Silenced `lru` advisory RUSTSEC-2026-0253 (pop use-after-free on an unreachable code path) in `.cargo/audit.toml` so `cargo audit` stays green. (#59)

## v0.9.0 (2026-08-10) — file-history snapshots + unified `/rewind`, provider read facade + wire alignment

### Added

- First-screen status card showing active provider/model/tier plus available providers and models
- `/model --tier <fast|standard|pro>` command surface (also accepts aliases: `haiku`/`sonnet`/`opus`/`flash`/`mini`/`plus`/`ultra`/`max`)
- `/model --save` flag persists tier choice to `~/.shannon/providers.toml`
- Three-level picker navigation (provider → tier → model)
- `TierName` enum (`fast`/`standard`/`pro`/`auto`) with alias normalization
- `/connect` validates the credential with a 1-token probe at connect time, so a bad key/region/model fails immediately instead of mid-query (fail-soft: a non-auth error warns but keeps the connection)
- Provider allowlist via `SHANNON_ENABLED_PROVIDERS` / `SHANNON_DISABLED_PROVIDERS` env vars — restricts the model picker, first-screen status card, and `/provider` / `/connect` listings; fails open (full list) when an allowlist matches nothing so a typo never bricks the picker
- LiteLLM community pricing table — `/model refresh` now also refreshes `model_prices_and_context_window.json`, so dynamic/custom models show real per-token prices instead of the estimate fallback
- `/model --tier auto` — resolves `auto` to a concrete tier via a lightweight best-default heuristic (standard → pro → fast); `auto` is input-only and never persisted
- `/provider health` — live-probes the active provider (1-token round-trip, 15s timeout, reuses the running key) and inventories every allowed provider's credential status. Informational only; no automatic failover (Shannon ships no model router by design).
- **File-level snapshots + unified `/rewind` (W6-2).** `FileHistoryManager` records per-file content snapshots (pre-modify on Write/Edit/MultiEdit, plus post-turn in `repl/query.rs`), giving `/rewind` three modes: `/rewind [n]` rewinds conversation turns, `/rewind <path>` reverts a single file to its previous AI-saved version (confirms before overwriting; `--yes` skips confirm), and `/rewind code|both <n>` reverts file changes to their state at turn N. `/undo` and `/checkpoint` are now aliases of `/rewind`. Configurable via `SHANNON_FILE_HISTORY` / `SHANNON_FILE_HISTORY_DIR` / `SHANNON_FILE_HISTORY_TTL` env (on by default).

### Changed

- `/help` now opens a modal overlay instead of injecting a System message into chat history (prevents `<file>`/`<line>`/`<character>` placeholders from leaking into LLM context)
- StatusBar pill format upgraded from `[model]` to `[provider/model · tier]`
- `arg_hint` placeholders renamed from `<file>` to `<FILE_PATH>` (ALL_CAPS) to reduce LLM misidentification risk
- `MODEL_CATALOG` is now the canonical pricing source of truth (SSOT) — cost tracking resolves per-model pricing through the catalog first, then file/env overrides, then LiteLLM, then a documented `$3/$15` fallback. Local catalog entries no longer get mispriced as hosted (bare `qwen` alias removed).
- Model picker shows an honest cost label — models with unknown pricing or context windows render `unknown` instead of fabricating a 200K window or a default price.
- Config files (`config.toml`, `.shannon.toml`, `providers.toml`) now resolve `{env:VAR}`, `{env:VAR:-default}`, and `{file:/abs/path}` / `{file:~/.shannon/x}` tokens in every string field. Single-pass so `{env:X}` whose value is `{env:Y}` stays literal (no recursive injection); `file:` paths must be absolute or `~/.shannon/`-rooted and may not contain `..`. Lets users reference secrets without inlining them, strengthening A1.
- **Provider store unification (Phase 2 task 4).** Desktop-managed provider connections (Add Provider modal → `~/.shannon/desktop/providers.json`) now round-trip through the engine's `~/.shannon/providers.toml` via `ProviderConfigStore::upsert_profile / remove_profile`. A process-level `Mutex<ProviderConfigStore>` on `AppState` serializes the read-modify-write so concurrent `save_provider` / `set_active_provider` / `delete_provider` calls can't clobber each other. On first launch, a one-shot migration lifts any existing `providers.json` entries into the engine store and removes the legacy file. Two distinct `openai-compatible` connections (e.g. GLM + Kimi) now keep their desktop slugs as the engine profile id, fixing the OpenAI-collapse that the `set_active(&LlmProvider, ...)` path caused. `ProviderConnection` gains the v2 `ProviderProfile` fields (`models_url`, `extra_headers`, `default_max_tokens`, `fallback_models`, `quirks`, `tiers`) so the desktop's UI surface matches the engine schema; the engine's runtime path still only consumes `extra_headers` + `default_max_tokens` end-to-end.
- **Stopped per-edit git auto-commit.** Shannon no longer creates a git checkpoint after every Edit/Write/Bash tool call; the REPL-side `CheckpointManager` git machinery (`create_checkpoint` / `revert_to` / `undo_last` / `preview_revert`) is removed. `/rewind code|both <n>` reverts through on-disk content snapshots instead of `git reset`, so it works in non-git directories and never rewrites git history.
- **ProviderConnection wire alignment (TD-4, ADR-0009 Phase 2).** The desktop's `ProviderConnection` DTO now mirrors the engine's `ProviderProfile` schema (`label`→`display_name`, `provider_kind`→`kind` kept as a String slug; dropped dead `api_key`/`model`/`created_at`; added `has_api_key: bool` derived from the credential store — fixes a pre-existing dead signal where the UI's "has key" indicator was always false since Phase 1). Legacy `providers.json` reads preserved via separate `LegacyProviderConnection`/`LegacyProvidersFile` structs for the one-shot migration.

### Fixed

- `/connect` no longer drops previously connected providers. The REPL connect path now writes `~/.shannon/providers.toml` through `ProviderConfigService::connect` (the same write path as `shannon providers add`), so `/connect A` then `/connect B` keeps both — previously the second `/connect` overwrote the file with a single-provider config and silently lost `A`. `/disconnect` still removes one provider; anyone who relied on `/connect` as "reset to one provider" can `/disconnect` the others.
- Status card now renders the "available providers/models" list from `MODEL_CATALOG` and connected/disconnected markers from `~/.shannon/providers.toml` in real time (was a static placeholder).
- `/model --tier <t> --save` tier override now survives a restart. `persist_model_to_providers_toml` writes `active_target` (provider + model id) via `ProviderConfigStore::set_active`, not just the tier name, so `resolve_active_target` reads back the chosen model on next launch. Verified by `store_set_active_survives_save_load_cycle`.

## v0.8.0 (2026-08-06) — provider write/read consolidation, local voice, CI gates

> Note: between v0.5.5 and this entry the CHANGELOG fell behind the unified-version releases (v0.6.x / v0.7.x). Entries below are reconstructed from `git log` per tag; the `[Unreleased]` block above accumulated across the same window and has not yet been redistributed into per-version sections.

### Features

- **Single write path for `providers.toml` (P2-2 Wave 6, PR #34).** Every write across CLI / REPL / Desktop flows through `ProviderConfigService`'s RAII `flock` + lock-then-reload read-modify-write (ADR-0008 Decision 3). Eliminates the concurrent-write clobber hazard on `save_provider` / `set_active_provider` / `delete_provider`.
- **Provider Store Read Facade (ADR-0009, accepted).** `ProviderReadSnapshot` consolidates the desktop's scattered `provider_store` read sites into one typed snapshot, built under one short-lived lock and released before any further `.await` — the read-side complement to the write-path `ProviderConfigService`.
- **Local voice (P2-5e).** On-device speech-to-text via `whisper-rs` backend + desktop frontend.
- **Chat polish (P2-5d).** Design tokens, `MessageBubble` refactor, Markdown rendering, Composer redesign, accessibility pass.
- **MCP integrations.** Notion MCP (P1-3c) and Linear MCP (P1-3d).

### Changed

- **C2 — drop `providers.json` dual-write (PR #37).** `save_provider` / `delete_provider` / `set_active_provider` R-M-W against the engine store only; the legacy `~/.shannon/desktop/providers.json` write-through cache is retired.
- **Semver gate required (S1-6/B1/B2, PR #40).** `cargo-semver-checks` flipped from advisory (`continue-on-error`) to a required merge gate; baseline `v0.8.0`. Pre-1.0 breaking changes still allowed as long as the minor bumps.

### Fixed

- CI: install Tauri + libdbus system deps in the metrics job; relocate `audit.toml` to `.cargo/` (cargo-audit 0.22+ discovers project config there, not at repo root); exclude `shannon-mcp-saas` from musl (keyring→libdbus-sys not musl-portable) and `shannon-desktop` from semver (Tauri GTK/WebKit rustdoc deps the job can't install); fix ~96 workspace intra-doc-link lint errors across 11 crates; strengthen Rust gates (P2-4: doc build, rustsec-audit, cross-platform matrix).

## v0.7.1 (2026-07-21) — gateway supervisor, engine discovery

### Features

- **Gateway supervisor prefers OS-managed service.** When `gateway.managed` is on, the desktop first tries an OS-managed `shannon-gateway` service before spawning its own subprocess.
- **Engine discovery — reuse existing api_server on :33420.** The desktop detects and reuses an already-running engine instead of starting a duplicate.

### Fixed

- Service probe hardened (per-platform service name, deterministic timeout + test).
- Windows: gateway built + installed via `install.ps1`; post-install hint block expanded to 5 steps.
- Surfaced the `shannon-code` → `shannon` rename in the desktop window title + docs.

## v0.7.0 (2026-07-19) — unified release & install story

### Features

- **Hermes-modeled unified release/install story.** One tag ships CLI + desktop + gateway together; `cargo-dist` + `tauri-action` + `gh release` pipeline.
- Windows bundler switched MSI → NSIS; added a Windows icon.
- Repository hygiene: Dependabot config, CODEOWNERS, issue templates, CONTRIBUTING.

### Fixed

- Release pipeline: `gh release edit --draft=false` (not softprops), `shasum -a 256` for the CLI checksum (macOS has no `sha256sum`), justfile release-prep echo, internal dep-pin + desktop version bumps with the workspace.

## v0.6.0 (2026-07-17) — OSS metadata, monorepo cleanup, CI hardening

### Features

- **OSS metadata + monorepo cleanup (phase6).** LICENSE, README polish, and the top-level restructure pairing `docs/archive/legacy-archives/` (markdown) with `legacy-archives/` (code + config) for pre-unification artifacts.
- **Desktop + gateway matrix release workflow.** Per-OS matrix (`cargo-dist` + `tauri-action`), upload-artifact scoping.

### Fixed

- CI hardening (F1-F7): Tauri apt deps, semver baseline, Node 24, serial tests; `shannon-desktop` excluded from semver + musl.
- Serialized flaky tests (`roll_over_resets_spend` date test, MCP config isolation) with `#[serial]`.
- Desktop release pipeline H-fix series: aarch64 cross-compile, Tauri deps for cargo-dist Linux, musl→gnu targets, bundle path globbing, YAML indent.

## v0.5.5 (2026-06-17) — notifications next phase (T-series + C9)

### Features

- **3 new webhook templates (C9, PR #36).** `WebhookTemplate` gains three variants: `Teams` (Office 365 connector: `{"text": "**{title}**\n{body}"}`), `Telegram` (Bot API Markdown: `{"text": "*{title}*\n{body}", "parse_mode": "Markdown"}` — caller includes `chat_id` via URL query), `DingTalk` (`{"msgtype": "markdown", "markdown": {"title": ..., "text": ...}}`). Renders verified via JSON parse in unit tests.
- **Webhook retry + bumped default timeout (T7, PR #36).** `WebhookHandler::send` now retries up to 3 attempts with exponential backoff (500ms → 1s → 2s, ±25% jitter via `rand`). Returns early on 2xx; logs `warn` on non-success status or transport error. Default `timeout_ms` bumped 3000 → 5000 to accommodate slow chat APIs (Slack/Discord can take 2–3s under load).
- **3-tier volume presets (T5, PR #36).** New `NotifPreset` enum (`Quiet` / `Balanced` / `Verbose`) on `NotificationsConfig.preset: Option<NotifPreset>`. `Quiet` = errors only; `Balanced` = errors + `permission:*` + `agent:*` sources; `Verbose` = all (subject to `minimum_level`). `Notifier::with_preset()` builder; filter applied before `minimum_level`. Defaults to `None` to preserve existing behavior.
- **Permission prompt notification (T2, PR #37).** REPL now fires a Warning notification when a permission dialog becomes visible in the event loop. Body shows the first 3 lines of the prompt description. Default is informational only — no inline approve button, per security trade-off documented in roadmap. 10s per-tool cooldown via `notify_dedup`.
- **Agent exit notification (T3, PR #37).** Sidebar's `refresh_agents()` extends the existing completed/failed detection with a third branch that diffs `prev_names` against `current_names` — catches agents that vanish from the registry without transitioning to Completed/Failed (signal kill, team teardown, coordinator drop). 5s cooldown coalesces batch teardowns.

### Tests

- `shannon-core::notifier`: +6 tests for C9 templates, T7 retry behavior, and T5 preset filtering.
- T2/T3 verified via `cargo nextest run -p shannon-ui` (1351 passed, 1 skipped).

## v0.5.4 (2026-06-17) — CLI webhook runtime fix

### Fixes

- **CLI webhook actually fires (PR #35).** Two compounding bugs made the v0.5.3 CLI webhook wiring non-functional for headless users:
  - **Config never loaded.** `load_headless_webhook_config` used `ConfigBuilder::new().build()`, but `ConfigBuilder::new()` doesn't auto-load any files. Even calling `.load_local_toml()` didn't help because the underlying `load_config_file` only does simple `key=value` parsing — it explicitly skips nested TOML tables like `[notifications.webhook]`. Rewrote the loader to call `toml::from_str::<ShannonConfig>` directly (toml crate is already in shannon-cli's deps). Reads `.shannon.toml` first, then falls back to `~/.shannon/config.toml`.
  - **No tokio runtime.** `fire_headless_completion_notification` runs AFTER the headless runtime block has been dropped, so `WebhookHandler::send` → `tokio::spawn` failed with `HandlerFailed { name: "webhook", reason: "no tokio runtime ..." }`. Wrapped the webhook send in a fresh `Runtime::new()` with a 3s `block_on` to keep the fire-and-forget task alive until delivery completes.

  Verified end-to-end with `nc -lk` listener: HMAC-SHA256 signature header present, Slack template body renders correctly.

## v0.5.3 (2026-06-17) — notifications next phase (Bundle A + Bundle B)

### Features

- **Webhook notification sink (Bundle B, commit e697172).** New `WebhookHandler` in `shannon-core::notifier` delivers notifications to any HTTP endpoint with six template formats: Slack (`{"text": "...", "blocks": [...]}`), Discord ({"content": "...", "username": "Shannon"}), Feishu/飞书 (`{"msg_type": "text", "content": {"text": "..."}}`), WeChat Work/企业微信 (`{"msgtype": "text", "text": {"content": "..."}}`), `Custom(String)` for user-supplied templates, and `Raw` (plain JSON envelope). Optional HMAC-SHA256 signing via `X-Shannon-Signature: sha256=<hex>` header when `secret` is configured — matches GitHub/Stripe webhook convention so receivers can verify authenticity. Fire-and-forget via `tokio::spawn` so a slow or unreachable endpoint never blocks the notifier pipeline. `WebhookConfig { url, secret, template, timeout_ms = 3000, include_body = false }` lives under `[notifications.webhook]` in `.shannon.toml`. CLI (`shannon-cli::main::fire_headless_completion_notification`) and desktop (`attach_notification_handler`) both auto-attach the handler when configured. Single-pass template substitution reuses the PR #31 security pattern — substituted values are never re-scanned for placeholders.
- **Desktop click-to-foreground (Bundle A).** `shannon-desktop::main` now listens for `notification-clicked` Tauri events and calls `unminimize + show + set_focus` on the main window. macOS and Windows already focus the app via native bundle-id behavior; this listener is a defensive fallback for Linux DEs and any future Tauri plugin versions that route desktop clicks here.

### Tests

- `shannon-core::notifier`: 17 new unit tests covering all six templates, HMAC signing, sanitization, and config parsing.
- `shannon-core/tests/webhook_integration.rs` (new): 7 mockito-backed integration tests verifying HTTP delivery, HMAC header, non-blocking behavior on slow/unreachable endpoints, runtime-missing error path, and Feishu/WeChat payload schemas.

## v0.5.2 (2026-06) — notifications feature (Phase 1 + Phase 2 + wiring)

### Features

- **Notifications core types + config (Phase 1, PR #30).** New `NotificationsConfig` and `NotificationCooldownConfig` in `shannon-core::notifier` with `interactive_default()` (sound on, level=info) and `headless_default()` (disabled — opt-in via config or `--notify`). `Cooldown` struct (DashMap-backed) provides per-source dedup with configurable windows (`permission_ms=0`, `query_complete_ms=0`, `tool_complete_ms=3000`, `error_ms=5000`, `agent_idle_ms=10000`). `Notification` struct gains `source: Option<String>` and `action_id: Option<String>` for richer routing. `Notifier` gains `with_cooldown()`, `with_minimum_level()`, `notify_dedup()` which returns `Ok(false)` when suppressed. `ShannonConfig.notifications: Option<NotificationsConfig>` with merge semantics. `NotificationLevel` serde-hardened with `rename_all="snake_case"` and `alias="critical"` for back-compat.
- **CLI shell-out notifier (Phase 2, PR #31).** New `shannon-cli::notifications::ShellNotifier` fires OS-native notifications by spawning platform binaries: `notify-send` (Linux), `osascript` (macOS), `powershell BurntToast` (Windows). Spawns via `std::process::Command` args array (no shell). New `--notify` CLI flag opt-in for headless mode. `fire_headless_completion_notification` maps exit code → notification level (success/warning/error) with source key `headless:{exit_code:?}`.
- **REPL notification wiring (PR #33).** Sidebar's `refresh_agents` now routes agent-completion events through the shared `Notifier` via `notify_dedup(&notification, 10_000)` so the `notifications_enabled` gate is honored and same-agent successive refreshes coalesce within a 10s window (previously constructed a fresh `DesktopNotifier` per iteration and called `.send()` directly, bypassing both gate and cooldown). `ReplState::new()` attaches `Cooldown::new()` to the shared `Notifier` so `notify_dedup` actually dedups across all callers. `loop_engine::notify_query_complete` switches from `notify` to `notify_dedup(..., 0)` — source key already set, window=0 matches the configured `query_complete_ms` default.

### Security

- **Shell-out injection hardening (commit f0d2675 on PR #31).** Security review of the initial P2 implementation identified three issues, all fixed before merge:
  - **AppleScript command injection** (CRITICAL): macOS template wraps values in `"..."` AppleScript strings but `sanitize()` did not escape `"` or `\`. A malicious title like `Evil ") & (do shell script "rm -rf ~")` could break out and execute arbitrary shell commands. Fixed: `escape_applescript()` escapes `\` first then `"`.
  - **PowerShell command injection** (CRITICAL): Windows template wraps values in `'...'` PowerShell strings but `'` was not escaped. Fixed: `escape_powershell()` doubles single quotes (the correct PowerShell escape).
  - **Template injection** (HIGH): Chained `str::replace` calls re-scan substituted values, so a title containing literal `{body}` would have body content injected. Fixed: single-pass `substitute()` helper scans the template once — substituted values are not re-interpreted.
  - **Arbitrary binary execution** (HIGH, acknowledged): `ShellNotifier::with_spec()` accepts any binary path; documented as a developer-API trust boundary. Platform-default path (`CommandSpec::platform_default()`) is the only user-reachable path. MVP does not expose config-driven binary selection.
  - **Test coverage**: 11 new unit tests verify escaping correctness via balanced-quote counters that simulate AppleScript/PowerShell parsing. The exact malicious payloads from the security report are used as test inputs.

### Tests

- P1: +15 unit tests in `shannon-core::notifier`.
- P2: +20 unit tests in `shannon-cli::notifications` (9 original + 11 security hardening).

## v0.5.1 (2026-06) — `.mcpb` install security hardening

### Security

- **Symlink path traversal**: `shannon mcp install` now refuses to follow symlinks for the target file (`.mcp.json` or `~/.shannon/settings.json`), blocking a planted symlink from redirecting writes to arbitrary files.
- **Zip bomb DoS**: `.mcp.json` entries larger than 10 MB uncompressed are rejected before reading.
- **Data loss on parse error**: An existing settings file that fails to parse as JSON now aborts the install (preserving the original file) instead of being silently reset to `{}`.
- **Install preview + confirmation**: The CLI now prints each server's `name -> command args` with `[OVERWRITE]` markers and prompts `[y/N]` before writing. `--yes` skips the prompt for scripts; `--dry-run` previews without writing.

## v0.5.0 (2026-06) — Sprint 5: Deepen MCP Integration

### MCP

- **Elicitation TUI**: Server-initiated `elicitation/create` requests surface as a ratatui `InputDialog` in the REPL, with responses delivered back over a bounded mpsc + oneshot channel. UI prefix `[EXTERNAL MCP · <server>]` distinguishes server-originated prompts from Shannon's own dialogs, capped at 200 chars to prevent spoofing abuse.
- **MCP prompts as slash commands**: Server prompts auto-register as `/{server}:{prompt}` aliases alongside the canonical `/mcp__{server}__{prompt}` form. New `/mcp prompts` lists every server prompt with descriptions.
- **Tab autocomplete via `completion/complete`**: Typing an argument after an MCP prompt slash command queries the originating server for argument completions (800ms timeout, silent fallback to local completion on miss).
- **`.mcpb` bundle install**: `shannon mcp install <bundle> [--user]` extracts a `.mcpb` zip archive (containing `.mcp.json`) and merges `mcpServers` into either the project's `.mcp.json` or `~/.shannon/settings.json`. Preserves existing servers and non-mcp keys; overwrites same-name entries.

### Security

- **Elicitation channel hardening**: Bounded `mpsc::channel(16)` replaces unbounded sender to prevent flood-based DoS.
- **Spoofing-resistant UI**: Server-originated dialogs visually distinct from Shannon's own.

## v0.1.0 (2026-05)

Initial public release with full feature set.

### Core Features

- **Multi-provider LLM support**: Anthropic, OpenAI, Ollama, DeepSeek, any OpenAI-compatible endpoint via adapter pattern
- **Streaming query processing**: SSE byte stream → `SseStream` → `MessageStream` with chunk boundary buffering
- **Session management**: Persistence, history, search, resume by ID (`--resume`, `--continue`)
- **Context compression**: Auto-compact, micro-compact, conversation phase tracking (Initialization → Active → Extended → Critical)
- **Prompt caching**: Three-layer Anthropic cache breakpoint injection — system prompt, last tool definition, last user message
- **Extended context window**: Phase-based budget reallocation, model-aware context sizes
- **Progressive context loading**: Head/tail preservation, auto-summarize, automatic truncation of large files

### Tool System

- **File operations**: Read, Edit, Write, MultiEdit with three-way merge and conflict resolution
- **Bash execution**: Sandboxed shell commands with streaming output and timeout control
- **Git integration**: Status, diff, log, commit, branch management
- **Web search**: Real-time information retrieval
- **Image analysis**: Screenshot understanding via `AnalyzeImageTool`
- **Notebook editing**: Jupyter notebook cell read/edit/insert/delete
- **Tool result cache**: TTL-based expiration, DashMap concurrent access, file-path invalidation
- **Tool orchestration optimization**: Dedup/parallel/sequential execution analysis, intelligent call grouping

### MCP (Model Context Protocol)

- **Full protocol implementation**: stdio, SSE, streamable HTTP transports
- **Dynamic tool registration**: `tools/list` with deferred schema loading
- **On-demand tool search**: `mcp__tool_search` with exact lookup and fuzzy search
- **Resource management**: Subscription tracking, update notifications
- **Webhook/channel support**: HMAC-SHA256 signing, event filtering, exponential backoff retry

### Multi-Agent System

- **Team coordination**: `TeamCreate`, `SendMessage`, `TaskCreate/Update/List`
- **Worktree isolation**: Per-agent git worktrees with working directory isolation
- **Per-agent config**: Model override, tool restrictions, working directory
- **`/batch` command**: Parallel worktree-isolated PR creation
- **Agent dashboard**: `AgentBarWidget` with 3 views, `AgentsPanel` sidebar

### Permission & Safety

- **Rule-based classifier**: Pattern matching for known safe/dangerous operations
- **LLM auto-classifier**: Async fallback for ambiguous cases (confidence < 0.7)
- **Permission profiles**: Strict, Balanced, Permissive, Custom (`.shannon/profiles/*.toml`)
- **4-tier precedence**: Hard deny > Soft deny > Allow > Explicit intent
- **Headless permissions**: `FullAuto` by default, `BypassPermissions` only with explicit `--yes`

### Commands & Skills

- **Built-in commands**: `/help`, `/config`, `/model`, `/compact`, `/undo`, `/rewind`, `/diff`, `/batch`, `/team`, `/cost`, `/search`, `/doctor`, `/routine`, `/preset`, `/session`
- **Skill framework**: Discovery, loading, execution from `.shannon/skills/` and plugins
- **Plugin system**: Manifest parsing, tool/command/skill plugin types
- **Hook system**: 32+ events (tool execution, compaction, config changes, agent lifecycle)
- **Triggered routines**: Hook-event-driven auto-execution (e.g., auto-lint after edits)

### Terminal UI

- **Interactive REPL**: Command history, search, vim mode
- **Markdown rendering**: Syntax highlighting, collapsible thinking, tool grouping
- **Diff visualization**: Colored output with stats
- **Token counter**: Context window bar, cost tracking, cache stats
- **Virtual scroll**: Progress indicators

### CI & Non-Interactive Mode

- **`--prompt` flag**: Non-interactive mode with NDJSON streaming
- **`--schema` flag**: JSON Schema validation for structured output
- **`--pipe` flag**: Pipe mode for automated workflows
- **`--diff-only` flag**: Only output file diffs
- **Tool restrictions**: `--allowed-tools`, `--max-turns` for CI safety
- **Deep links**: `shannon://prompt?text=<>` and `shannon://resume?id=<>` URL scheme

### Infrastructure

- **LSP integration**: 6 LSP tools, automatic background `cargo check` diagnostics
- **Memory system**: Persistent store, auto-dream extraction, consolidation
- **File checkpointing**: Git-based checkpoints with diff preview before revert
- **Auto-updater**: GitHub Releases-based update checking
- **Diagnostics & Doctor**: Environment health checks, error pattern analysis
- **Performance benchmarks**: `criterion` benchmarks with regression thresholds
- **i18n**: 10 languages via `rust-i18n`
- **VS Code extension**: WebView chat panel, diff viewer, NDJSON communication
- **Error recovery**: Configurable retry with exponential backoff + jitter

### Testing

- ~7,889 tests across 12 crates
- Every `src/**/*.rs` has at least one `#[test]`
- `mockito` HTTP mocking — never hits real APIs
- YAML declarative scenario tests
- Record/replay system for real API fixtures
- Performance regression thresholds
