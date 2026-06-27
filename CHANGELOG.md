# Changelog

All notable changes to Shannon Desktop are documented here. Entries are grouped by sprint and category.

## [Unreleased] — Models P2 (managed providers)

### Models P2 — managed providers store + generic OpenAI-compatible test

Branch `s2/models-p2-rust`. First half of the Models P2 split (Rust backend;
the UI rewrite follows in a second, UI-only PR). Adds a managed multi-provider
roster so users keep several connections configured and switch between them,
plus a generic OpenAI-compatible connection test that closes the gap where GLM
/ MiniMax / Kimi previously fell through to "unknown provider".

#### Rust
- **New `~/.shannon/desktop/providers.json` store** (`config.rs`):
  `ProviderConnection` (id / label / provider_kind / api_key / base_url /
  model / created_at) wrapped in `ProvidersFile` (active_provider_id +
  providers list). `load_providers` / `save_providers` mirror the existing
  `mcp-servers.json` pattern, including owner-restricted permissions on write.
- **Four managed-provider commands** (`commands_config.rs`, registered in
  `main.rs`):
  - `list_providers` — masks API keys; lazily migrates the legacy singular
    config into one seeded entry on first call (so existing users see their
    current connection, not an empty list).
  - `save_provider` — insert or upsert by id; preserves the stored key when the
    frontend sends `"***"` or empty (editing the label never blanks the secret).
  - `delete_provider` — clears `active_provider_id` if it pointed at the
    removed entry.
  - `set_active_provider` — mirrors the connection into `DesktopConfig`'s
    singular fields, rebuilds the engine client config, persists both stores,
    and emits `CONFIG_UPDATED` (tray + open windows refresh their label).
- **`test_provider_connection` gains an optional `base_url`** and a new
  `openai-compatible` kind (`GET {base_url}/models` with `Authorization:
  Bearer`), enabling GLM/Zhipu, Moonshot/Kimi, MiniMax, Together, Groq, etc.
  Built-in kinds (anthropic/openai/deepseek) honor an optional `base_url`
  override; anthropic keeps `x-api-key`; ollama keeps its authless tags probe.
  The new arg is optional, so existing two-arg callers stay compatible until
  the UI PR passes `base_url`. URL/auth resolution is extracted into a pure
  `provider_probe_url` helper (unit-tested, no network).
- **No `DesktopConfig` schema change**: the active provider still drives the
  existing singular fields, so `config.json` and the engine-facing contract are
  unchanged — this PR is additive.
- Tests: +13 unit tests (probe-URL/auth matrix, slugify + de-dup id, key
  masking, `ProvidersFile` round-trip, optional-field deserialization).

#### Rust (security hardening)
- **`validate_base_url` guards every user-supplied `base_url`** (the test
  probe, the ollama host, and `save_provider` persistence): parses with the
  `url` crate, requires an `http`/`https` scheme, rejects embedded credentials,
  missing hosts, and unparseable input, and drops fragments. Private/loopback
  hosts are **intentionally allowed** — `http://localhost:11434` (Ollama) and
  self-hosted models on private networks are first-class use cases, and the URL
  is supplied by the local user (no untrusted/remote input vector reaches this
  path). +6 unit tests covering rejected schemes / credentials / malformed
  input and the http+localhost allow case.

### Models P2 — managed-providers UI (provider roster + modal)

Branch `s2/models-p2-ui`. Second half of the Models P2 split — the UI rewrite
that surfaces the managed-providers store from #70. Replaces the old
single-provider quick-setup presets and standalone API-key box with a roster
users add / edit / test / activate / delete through, while keeping the tested
Active Model, Performance Strategy, and Global Parameters sections intact.

#### UI
- **`ModelsSettings` rewritten** (`components/settings/ModelsSettings.tsx`):
  - New `ProvidersSection` lists configured connections as rows — key-set dot,
    active badge, and Test / Activate / Edit / Delete actions. Loads via
    `listProviders` on mount and refreshes from each command's returned
    `ProvidersFile` after a mutation.
  - New `ProviderModal` (built on the shared `Modal` primitive) collects label
    / kind / base_url / api_key / model, with one-tap QUICK_FILL chips for
    anthropic, openai, deepseek, glm, kimi, minimax, ollama, and custom
    (OpenAI-compatible). Saving re-submits the masked `"***"` so editing a
    label never blanks the stored secret; the active provider drives the
    existing Active Model grid unchanged.
  - Test results surface through a `toastTestResult` helper; activation /
    deletion toasts confirm the outcome.
  - Removed `QuickSetupPresets` and the standalone API-key input — superseded
    by the roster. Performance Strategy, the Active Model provider tabs + grid,
    and the Global Parameters sliders are preserved verbatim.
- **Typed bridge** (`lib/tauri-api.ts` + `types/index.ts`): `ProviderKind`
  union, `ProviderConnection`, `ProvidersFile`, `ProviderInput`; wrappers
  `listProviders` / `saveProvider` / `deleteProvider` / `setActiveProvider`;
  `testProviderConnection` now passes the optional `baseUrl`.
- **Test mock defaults** (`__tests__/setup.ts`): the four new providers
  commands resolve to an empty `ProvidersFile`; existing suites are unaffected.
- **i18n** (`en` + `zh-CN`, 43 keys each, parity-checked):
  `settings.models.providers.*` covers section chrome, modal fields and kind
  labels, QUICK_FILL, and every toast (added / saved / activated / tested /
  deleted and their failure variants).
- Tests: the `ModelsSettings` suite is updated to assert the new providers
  section and Add-provider button; full vitest suite green (1254 tests).

## v0.3.7 (2026-06-27) — UI design overhaul + Week D (Plan Mode, Diff Preview) + PM-audit follow-ups + Settings P1 (Models/Notifications) + i18n completion

### i18n completion — MermaidRenderer deep audit (last hardcoded strings)

Branch `s2/i18n-deep-audit`. Final pass of the i18n audit. The deeper sweep
confirmed the app is otherwise fully internationalized; the only remaining
hardcoded user-facing strings were three inside `MermaidRenderer`'s sandboxed
`srcDoc` (loading placeholder, render-failed message) and the diagram `title`
fallback.

#### UI

- **MermaidRenderer.** The iframe `srcDoc` is now built by a
  `buildSrcDoc(source, loadingLabel, failedLabel)` helper with the labels
  pulled from `react-intl`; the SVG `title` fallback uses
  `artifact.mermaid.diagramTitle`. The `srcDoc` is memoised on
  `[source, loadingLabel, failedLabel]` so the iframe rebuilds only when
  something actually changes.
- 3 new i18n keys (`en` + `zh-CN`): `artifact.mermaid.loading`,
  `artifact.mermaid.renderFailed`, `artifact.mermaid.diagramTitle`.

### Notifications Phase 1 — one outbound surface + disable dead Email control (N1/N5)

Branch `s2/notifications-p1`. First implementable slice of the Settings
redesign PM plan (§2): the two trust-eroding issues in Notifications.

#### UI

- **One outbound surface (N1).** `WebhookSection` and `OutboundSection` are
  now wrapped together under a single `<section>` with a shared "Outbound
  notifications" header (`settings.notifications.outbound.sectionTitle` +
  `.sectionDesc`), so the relationship between the two blocks is stated
  rather than implied. `OutboundSection`'s own title dropped h3 → h4 to read
  as a sub-heading of the unified surface.
- **Disable-or-ship Email (N5).** The Email channel card no longer opens a
  no-op wizard that toasts "coming soon." It is now a clearly-disabled card —
  `comingSoon: true`, a "Coming soon" badge (`...channel.comingSoon`) and an
  "Email inbound coming in Phase 2" note — and the dead `<EmailWizard>`
  render block was removed (`EmailWizard.tsx` retained for Phase 2). The
  card's onClick guards on `!comingSoon`, so clicking does nothing.
- New i18n keys (`en` + `zh-CN`): `settings.notifications.outbound.sectionTitle`,
  `.outbound.sectionDesc`, `.channel.comingSoon`.
- Test updated: the Email-wizard test now asserts the coming-soon badge and
  that clicking the card does not open a wizard.

### Models Phase 1 — one API-key path + sliders wired to config + real connection test (M1/M4)

Branch `s2/models-p1`. First implementable slice of the Settings redesign PM
plan (§1): the two highest-trust fixes in Models.

#### UI

- **One authoritative API-key path (M1).** The "Test connection" button in the
  bottom API-key box now calls the real `testProviderConnection(provider,
  apiKey)` — previously it only re-ran `refreshModels`, so a bad key looked
  healthy. The result is shown via a `switch(result.kind)`
  (`success` / `invalid_key` / `rate_limited` / `provider_error` /
  `network_unreachable` / `unknown`), each with its own message
  (`settings.models.testResult.*`). The button is disabled until a key is
  present (`(keyDraft ?? '').trim() || config?.api_key`). The presets' inline
  `switchProvider` path stays for quick setup; the bottom box is the
  canonical key surface.
- **Sliders wired to config (M4).** Temperature and max-tokens sliders now
  read from config (`config?.temperature ?? 0.7`, `config?.max_tokens ?? 4096`)
  instead of the literal `0.7` / `4096` that reset on every remount.
  `ParameterSlider` keeps a local copy for the input but re-syncs to the prop
  via `useEffect`, so external config changes (e.g. `switchProvider`)
  propagate.
- 7 new i18n keys (`en` + `zh-CN`): `settings.models.testResult.success` /
  `.failed` / `.invalidKey` / `.rateLimited` / `.networkUnreachable` /
  `.providerError` / `.unknown`.
- Test setup: `testProviderConnection` added to the `tauri-api` mock defaults.

### Settings redesign PM plan (Models + Notifications) + i18n gap fixes

Branch `s2/settings-review`. A senior-PM review of the two Settings areas the
user flagged as "配置方式和 UI 组件设计不合理" — Models and Notifications —
delivered as a **proposal doc** (not an implementation): competitor research
(LibreChat, Cherry Studio, LobeChat, Linear, Slack 2026), a root-cause
problem table per area, a phased redesign (Phase 1/2/3), and scope-discipline
notes. The plan drives the Phase-1 implementation in the two entries above.

#### Docs

- **`docs/product-review/settings-redesign-pm-plan.md`** — the PM plan
  (proposal; P2/P3 awaiting approval).

#### UI

- **i18n gap fixes** bundled with the review: the remaining hardcoded
  user-facing strings in six components moved to message keys — `DiffViewer`,
  `LspQuickFixPanel`, `ModelsSettings` (performance-strategy labels), `modal`
  (close `aria-label`), `Hooks`, `MissionControl`, `OPCTask`.
- **Test setup hardening.** `__tests__/setup.ts`'s `render()` mock already
  auto-wrapped each tree in `I18nProvider`; it now also wraps `rerender()`,
  so tests that exercise `rerender` (e.g. the Modal "restores body scroll on
  close" case) keep the intl context instead of throwing "Could not find
  required intl object".

### silent-catch error-feedback sweep — surface the real cause in failure toasts

Branch `s2/silent-catch-sweep`. Follow-up to the shared `toastError` helper
shipped in the PM-audit-followups batch (commit `023c208`): migrates the
catch sites that toasted a generic "Failed" message and discarded the caught
error, so users now see *why* an action failed in the toast description.

#### UI

- **Silent-catch → `toastError`.** Every catch block that called
  `toast.error(t('…failed'))` (or the `intl.formatMessage` equivalent)
  without passing the error now calls `toastError(key, e)`, which puts the
  translated title in the toast and the normalised cause in the description.
  Arrow callbacks that discarded the error entirely (`.catch(() => …)` in
  MessageBubble, CommandPalette, OPCMissionFocus, ExtensionsHub) now bind and
  forward it. The now-redundant `console.warn`/`console.error` lines in those
  blocks were removed (the toast description replaces them). 52 sites across
  20 modules — scheduled-tasks hook, Tasks/Routines/Hooks/Profiles/Welcome
  pages, OPC kanban/agent-swarm/mission-focus, the notification wizards,
  models/notifications settings, extensions hub, skill approval/review,
  command palette, and the diff/routine-template helpers.

- **Scope set by verifying current code, not the audit estimate.** The PM
  audit estimated ~64 sites; re-checking against current code, only 52 were
  genuinely silent. Left untouched: sites that already surface the cause
  (McpServers/InstallDialog/McpAddServerDialog via `safeErrorMessage`,
  DiffDialogMulti via a `description` arg, OutboundSection already migrated),
  pre-API validation guards (`*Required`, `needNameAndCommand`,
  `noPackageMetadata`, URL-format checks), and the Welcome
  `switch(result.kind)` result branches that report provider status rather
  than a caught error.

- No new i18n keys (the existing failure keys are reused as the toast title).
  One existing Welcome test updated to assert the cause now appears in the
  description slot.

### per-page feature gaps — Profiles conflict detection, Mission Control board filter, Editor diagnostic format

Branch `s2/perpage-features`. Closes three feature-sized gaps surfaced by the
§P1 per-page PM-audit re-verification (`docs/product-review/pm-audit-followups.md`)
that were genuinely missing (the rest were already shipped or stale). Two
other audit items — Routines run-now/history and pinned-message persistence —
are backend-dependent and remain deferred.

#### UI

- **Profiles — within-profile conflict detection + create-form validation.**
  A profile whose Deny list overlaps its Auto-approve or Confirm list now
  shows a warning banner naming the conflicting tools (`ruleConflicts()`
  helper). The create form gains duplicate-name detection and rule-conflict
  detection: both block save and surface a warning (inline under the name
  field and in the footer). New `profiles.conflict.ruleConflict` and
  `profiles.conflict.duplicateName` i18n keys (en/zh-CN). Six new Profiles
  tests.

- **Mission Control — board column filter.** The status chips in the header
  are now buttons: clicking one isolates that column on the board
  (jump-links there first if needed), clicking it again restores all
  columns. Built on KanbanBoard's existing `columns` prop — no new board
  state. New `missionControl.filter.focus` / `.clear` i18n keys (en/zh-CN).
  One new Mission Control test.

- **Editor — LSP diagnostic sentence format.** Quick-fix panel diagnostic
  messages now render in sentence case (first letter capitalized) for
  readability. One new LspQuickFixPanel test.

### P1 design-system batch — label-xs token + Banner primitive

Branch `s2/design-system-p1`. Closes the two genuine gaps in the P1 design
system (audit tasks 17–20); the rest — elevation/duration/icon-size/headline
tokens, global `focus-visible` rings, and 18 shared primitives — shipped in
PR #50.

#### UI

- **`--text-label-xs` token (bug fix).** The 11px label token was absent from
  the `@theme` block, so the `text-label-xs` utility generated no CSS and 86
  label usages were silently rendering at the inherited font size. Added at
  11px matching the `--text-label-sm`/`-md` recipe (line-height 1, tracking
  0.04em, weight 500).

- **`<Banner>` primitive.** The one missing shared primitive — a dismissible
  status surface with `role=alert` (error tone) / `role=status` (otherwise)
  and `aria-live=polite`, with bar/card variants and info/warning/error/success
  tones. Migrates the two hand-rolled banners (Chat "API key missing", Tasks
  inline error); the Tasks close button gains an `aria-label`. New
  `common.dismiss` i18n key (en/zh-CN).

### Triage keyboard navigation — navigate the triage list without the mouse

Branch `s2/triage-keyboard-nav`. Accessibility win from the §P1 per-page
PM-audit sweep: the Triage item list had no keyboard support, so it was
mouse-only.

#### UI

- **Triage list keyboard navigation.** The triage item list is now a
  focusable region (`role="list"`, `tabIndex=0`, descriptive `aria-label`).
  When focused: `j`/`↓` move to the next item, `k`/`↑` to the previous,
  `Enter` marks the focused item read, `a` archives it. The focused card
  gets a primary focus ring and scrolls into view; keystrokes from form
  controls (the filter chips and read-filter) are ignored so those keep
  working. New `triage.list.aria` i18n key (en/zh-CN). Three new Triage
  tests (j+Enter mark-read, a archive, form-control guard).

### PM audit follow-ups batch — Naming + error feedback + empty states + chat attach + skill drawer

Branch `s2/pm-audit-followups-batch`. Closes the remaining P0 honesty
findings and the §A–§C cross-cutting items from the senior PM audit
(`docs/product-review/03-senior-pm-audit.md`) that PR #50 did not reach.

#### UI

- **PM naming pass (audit §A).** Seven user-visible labels renamed to match
  the rename table in `03-senior-pm-audit.md` §A: Extensions → Integrations,
  Data Sources → Connections, Worktrees → Workspaces, Routines → Schedules,
  Hooks → Automations, Permission Profiles → Approval Profiles, and the OPC
  "Save Focus" → "Save Mission" copy. Labels only — route paths, code
  identifiers, and i18n keys are unchanged so bookmarks, tests, and downstream
  consumers keep working. (`ed8a41d`)

- **Shared error-feedback helper (audit §B).** New `ui/src/lib/errorToast.ts`
  exports `errorMessage(e)` (normalises unknown catch values to a display
  string) and `toastError(key, e)` (sonner toast with the translated title and
  the real cause in the description slot). Eleven catch blocks across seven
  components (Header, MyAgents, AdvancedSettings, BillingSettings,
  GeneralSettings, ModelsSettings, OutboundSection) migrated from
  `console.warn` + generic `toast.error('Failed')` to the helper, so users now
  see *why* an action failed. Bulk replacement of the remaining silent catches
  is tracked as a follow-up. (`023c208`)

- **Empty-state CTAs wired (audit §C).** Six of eleven `EmptyState` usages
  lacked an action; now wired: WorktreePanel (Refresh workspaces), Goals (Ask
  AI to suggest tasks), Triage no-match (Clear filters), OPCAgentSwarm (Spawn
  agent), ExtensionsHub (Clear search / Reload), MyAgents (Create first agent).
  AgentMessagesPanel left intentionally without a CTA — purely informational.
  (`fbf516e`)

- **Chat attach picker polish.** The attach button (US-CHAT-08, wired in
  `bf8a933`) now opens with two filter presets (Images / All Files) and renders
  image thumbnails (png/jpg/jpeg/gif/webp/bmp/svg) on the attached-file chips
  via `convertFileSrc`; non-images keep the description-icon chip. (`4f5ade4`)

- **Skill detail drawer.** Skill card bodies now open a right-side drawer
  showing full metadata (author, version, license, stars, source repo, last
  updated, tags, homepage), following the `TaskDetailDrawer` pattern
  (role=dialog, aria-modal, Escape/backdrop to close, click-on-panel doesn't
  dismiss). The card Install button still works independently; the drawer
  mirrors it for symmetry. (`7823238`)

#### Fixes

- **DataSources page leaked hardcoded English.** The "Verified" badge, the
  "Query coming soon" notice, and the install/uninstall feedback strings were
  literal English bypassing i18n; the `extensions.datasources.verified` and
  `.required` keys already existed but were unused. Now routed through `t()`,
  three new keys added (en + zh-CN), and the untranslated
  `extensions.datasources.noInstalled` value in `zh-CN.json` corrected.

### Week D — Plan Mode + Diff Preview + Voice/Artifact/Self-improve wire-ups

#### Features

- **Plan Mode toggle (D4).** Dedicated `PlanModeToggle` in the chat
  composer area; clicking it switches `approval_mode` to `plan` via
  `api.configure`. When active, an "exit plan" banner sits above the
  message list with a one-click return to the prior mode. 8 tests.
  (`s2/week-d-diff-preview`)

- **Diff Preview (D5).** Files-changed summary bar with file counts,
  expand/collapse for each file, and "Review all" bulk action. Wired
  into the existing file-diff endpoint. 7 tests.

- **Voice Mode UI shell (C1/D1).** `MicButton` + animated `VoiceOrb`
  + `useVoice` hook. In stub mode the hook returns a placeholder
  transcript after `simulateLatencyMs`. Drop-in ready for a real STT
  backend. 16 tests.

- **Artifact Panel Phase 1 (C3/D2).** Detects HTML / SVG / mermaid /
  long-markdown artifacts inside chat messages and surfaces them as
  inline chips. Click a chip to open the side panel with Preview /
  Code tabs, Copy, Export. 19 tests.

- **Self-Improvement Phase 1 (C5/D6).** Tauri API stubs for
  `list_skill_candidates`, `approve_skill_candidate`,
  `reject_skill_candidate`, `list_agent_authored_skills`.
  `AgentAuthoredBadge` + `SkillApprovalModal` primitives ready for
  hook-up. 12 tests.

- **Voice MicButton wire-up (E1).** Composer-integrated mic button
  with idle / recording / processing states; orb animates above the
  composer while recording.

- **Skills page filter pill + AgentAuthoredBadge (E2).** Installed
  skills section gains All / Curated / Agent-authored tabs with
  counts; agent-authored rows show the badge and an `auto_fix` icon.
  3 tests.

- **SkillApprovalModal trigger (E3).** Advanced Settings → Skill
  Extraction card now shows a pending-count badge and Review button
  when candidates exist; clicking Review walks through the queue.
  3 tests.

- **Artifact Phase 2 renderers (F1).** `MermaidRenderer` uses a
  sandboxed iframe with strict CSP (script-src limited to
  `cdn.jsdelivr.net/npm/mermaid@11`) so diagrams render without
  adding mermaid to the bundle. `DocumentRenderer` uses
  `react-markdown` + `remark-gfm` + `rehype-sanitize` +
  `rehype-highlight` with MD3-styled components. 6 tests.

- **Artifact Phase 3 polish (F2).** Resizable panel (drag handle on
  left edge, width persisted to `localStorage`); fullscreen toggle;
  auto-open toggle (when on, new chips auto-open the panel on
  mount); `Cmd/Ctrl+Shift+A` shortcut cycles through open artifacts.
  6 tests.

- **Pending-skill badge on Header bell (H1).** `usePendingSkillCandidates`
  hook polls every 30 s. When the queue is non-empty, the bell shows
  a red count badge and clicking it opens `SkillApprovalModal`
  directly; with an empty queue the bell falls back to the existing
  Triage navigation. 3 tests.

- **Voice Phase 2 — Web Speech API (B1).** `useVoice` now prefers
  `window.SpeechRecognition` / `webkitSpeechRecognition` when available,
  falling back to the stub implementation when the browser doesn't
  expose the API (e.g. jsdom tests). Adds `supported` and surfaces
  recognition errors. 3 new tests.

- **Artifact code-tab syntax highlighting (B2).** Code tab in the
  ArtifactPanel now uses a shared `CodeBlock` component (highlight.js
  core, 12 languages registered). Languages are resolved from artifact
  kind (HTML/SVG/mermaid/markdown) or auto-detected. 5 tests.

- **Plan Mode enhancements (B3).** Banner gets a dismiss button, and
  `Cmd/Ctrl+Shift+P` globally toggles plan mode on or off. 5 new tests.

- **Diff Preview syntax highlighting (B4).** `DiffViewer` now renders
  per-line highlighted HTML using a shared hljs setup. Language is
  resolved from the diff's language hint plus the file extension
  (covers TypeScript, JavaScript, Python, Rust, Bash, YAML, Markdown,
  JSON, HTML, XML, CSS). Open highlight spans carry across newlines so
  multi-line constructs (block comments, template literals) stay
  colored on every line. 10 new tests in `diff-highlight.test.ts`.

- **Keyboard shortcuts help overlay (B5).** Rebuild of the `?` overlay:
  grouped into Global / Navigation / Chat / Diff Review sections,
  adds an inline search input, and surfaces the new Plan Mode toggle,
  Artifact cycle, and per-hunk diff review shortcuts. 11 tests.

- **Long-list pagination for Skills catalog (B6).** New
  `usePagedVisible` hook paginates client-side lists with a "show
  more" affordance. Applied to the Skills catalog so 24 cards show
  initially, with the rest loaded on demand. 6 hook tests.

- **Skill candidate storage (C1).** New
  `commands_skill_candidates.rs` module backs the four Tauri commands
  the UI was already calling (`list_skill_candidates`,
  `approve_skill_candidate`, `reject_skill_candidate`,
  `list_agent_authored_skills`). Candidates persist as JSONL at
  `~/.shannon/desktop/skill-candidates.jsonl`; promoted skills land at
  `~/.shannon/skills/agent-authored/<slug>.json`. 5 Rust tests.

- **Pattern detection daily cron (C2).** New
  `skill_pattern_detection.rs` module + `trigger_skill_pattern_detection`
  Tauri command. Scans `~/.shannon/sessions/*.json` modified in the
  last N days, computes a normalized signature per tool_use block
  (tool name + sorted arg keys), and emits SkillCandidate entries for
  signatures seen in 2+ sessions with 3+ total occurrences. Defaults
  to a 7-day lookback. Can be wired into the scheduled-tasks layer
  for automatic daily runs. 6 Rust tests.

- **Self-improvement Phase 3 — live catalog refresh (D6).**
  `approve_skill_candidate` and `reject_skill_candidate` now emit a
  `skill-catalog-changed` Tauri event. The Skills tab subscribes via
  `useTauriEvent` and re-pulls its installed + agent-authored lists,
  so approving a candidate no longer requires a page reload to see it.

- **Self-improvement Phase 4 — LLM skill refinement (D6).** New
  `refine_skill_candidate(id)` Tauri command calls the configured
  LLM with a skill-procedure refinement prompt, stores the rewritten
  steps back into the candidate, and marks `refined=true`. Falls
  back to the original procedure on LLM error. `SkillCandidate` gains
  a backwards-compatible `refined:bool` field (`#[serde(default)]`).

- **Self-improvement Phase 5 — privacy opt-out + docs (D6).**
  New `skill_detection_enabled` config flag (default: true). When
  disabled, `trigger_skill_pattern_detection` returns 0 without
  scanning sessions. Toggle surfaced in Settings → Advanced.
  `signature_of` now documented as key-only by design — no file
  paths, tokens, or argument values ever leave the session log.

- **Voice Phase 3 multi-provider scaffold (D2).** New
  `lib/voice/` module with a provider abstraction:
  `VoiceProvider` interface + three concrete providers
  (`stub`, `webspeech`, `remote`) + a factory that picks one based on
  runtime support. The remote provider posts audio blobs to a
  configurable endpoint with optional bearer auth — ready for a
  Whisper / Deepgram / AssemblyAI backend. 15 tests. useVoice stays
  on Web Speech for now; a follow-up will refactor it to consume
  this abstraction.

#### Documentation

- **Week D design docs.** D1 Voice Mode, D2 Artifact Panel, D6
  Self-Improvement Loop — three planning documents under
  `claudedocs/` describing scope, phasing, and explicit deferrals
  for multi-day / cross-repo work.


### P0 PM review fixes — Demo mode + i18n + Welcome rendering

#### Fixes

- **Demo-mode crashes on /triage, /memory, /extensions/featured.**
  Three pages threw raw errors when run with `VITE_MOCK_MODE=1`
  because mock handlers were missing or returned the wrong shape.
  `MOCK_TRIAGE_STATS` had `as unknown as TriageStats` hiding a
  missing `by_kind` field; `Triage.tsx` now null-guards
  `Object.entries(stats.by_kind)`. New `ui/src/lib/mock/data/memory.ts`
  plus eight handlers (`list_memories`, `list_memory_projects`,
  `get_memory_stats`, `create_memory`, `update_memory`,
  `delete_memory`, `search_memories`, `list_featured_vendors`)
  cover the Memory page and Extensions Hub Featured tab.
  (`s2/p0-pm-review-fixes`)

- **Welcome page option cards rendered as 32-pixel-wide slivers.**
  Tailwind v4 ships `--container-2xl/3xl/…/7xl` defaults but
  `xs/sm/md/lg/xl` silently fall through to the spacing scale, so
  `max-w-xl` resolved to `32px` instead of `36rem`. Patched
  `ui/src/index.css` with explicit `@layer utilities` overrides for
  the five broken sizes. Welcome now renders the 2×2 grid of task
  option cards as designed.

- **Vite mock mode failed to start (`Cannot read file:
  /src/lib/mock/coreMock.ts`).** The `@tauri-apps/api/core` alias was
  a bare specifier that esbuild's pre-bundle phase treated as a
  filesystem path. Switched to `path.resolve(__dirname, …)` so the
  alias resolves to an absolute path before the bundler sees it.

- **Header page titles were hardcoded English, bypassing i18n.**
  `Header.tsx` used a switch on `pathname` returning English string
  literals. Replaced with a `TITLE_MAP` of route-prefix → i18n key
  and routed all titles through `intl.formatMessage`. Eleven new
  `header.title.*` keys added to `en.json` and `zh-CN.json`.

- **Mock `status.version` was a hardcoded `"0.4.2"`.** Now sourced
  from `__APP_VERSION__`, a build-time constant injected via Vite
  `define` from `package.json`. `ui/src/vite-env.d.ts` carries the
  global type declaration so TS stays happy.

### UI design overhaul (PR #50)

#### Tooling

- **Local dev debugging scripts.** `scripts/dev-start.sh` + `dev-stop.sh`
  orchestrate vite + `cargo run` with PID files, port-readiness polling,
  and graceful cleanup. Logs land in `${XDG_RUNTIME_DIR:-/tmp}/shannon-dev/`.

#### UI (from PR #50 — full list at the PR description)

- **P0 honesty pass:** removed misleading buttons, dirty guards on Memory
  + OPC approve/rollback, billing demo-mode locks, ConfirmDialog for all
  destructive ops, floating-branch warning in InstallDialog.
- **P1 accessibility + shared primitives:** LoadingState, ErrorState,
  ConfirmDialog, SkeletonLoader (+ variants), focus-visible rings,
  focus trap on 18 modals, virtualized chat, mod+n, Editor Ask AI.
- **P2 page redesigns:** Chat / Memory / Extensions / Settings, chart
  palette tokens, StreamingResponse extraction.
- **P3 polish:** brand icons, micro-interactions, i18n audit.
- **Fix:** Tailwind 4 parser bug (stray `)` in `@custom-variant`).

#### Documentation

- **PM audit follow-ups.** `docs/product-review/pm-audit-followups.md`
  lists all items unresolved after PR #50 with a 4-week recommended
  sequence.
- **Documents Extension Phase A plan.** `docs/extensions/documents/phase-A.md`
  scopes the Q4 2026 extension work — pandoc-based DOCX/PDF generation
  via host tools, no core code.

### S2 follow-ups — Engine pin + dev-mode schema validation

#### Tooling

- **Local dev debugging scripts.** `scripts/dev-start.sh` launches
  vite on :1420, waits for the port to respond (up to 60 s), then
  starts `cargo run`. `scripts/dev-stop.sh` reads the PID files and
  kills both processes, with `pkill` and port-free fallbacks for
  stray children. Logs land in `${XDG_RUNTIME_DIR:-/tmp}/shannon-dev/`.
  Replaces the two-terminal dance that left tauri caching
  connection-refused state when vite was slow to bind.

#### Changes

- **Engine pin bumped to `d49e7f5`.** Picks up the D1 Phase 3 cleanup
  (shannon-code PRs #58–#64): `shannon-engine` is now a direct dep,
  and all desktop imports of the migrated modules (`api`, `state`,
  `permissions`, `hooks`, `compact`, `context_pressure`, etc.) go
  through `shannon_engine::*` instead of the removed deprecated shims
  in `shannon-core`.

- **Engine pin bumped to `ff02637`.** Picks up shannon-code PRs #49
  (D1 phase 1 internal reorg), #50 (B3 JSON Schema emit for events),
  #51 (C4+T5 `#[stable_api]` macro), #52 (C5 semver baseline pinned
  to `v0.5.5` git tag, flipped to blocking).

- **Dev-mode JSON Schema validation for Tauri events.** New
  `useTauriEventValidated` hook wraps `useTauriEvent` with an
  ajv-based payload check against `ui/src/schema/events.schema.json`
  (mirrored from `shannon-types`). Mismatches log a `console.warn`
  in dev only (`import.meta.env.DEV` gate) — production hot path is
  unchanged. Unmapped event names skip validation. Three unit tests
  cover the valid/mismatch/unmapped paths.

- **Schema-sync check.** `scripts/check-schema-sync.sh` verifies
  `ui/src/schema/events.schema.json` matches the canonical copy in
  `../shannon-code/crates/shannon-types/schema/`. Wired into the
  `scripts/local-check.sh` pre-push gate so drift fails before push.

### D-group + P0 fixes — Skill loop + events refactor

#### Fixes

- **Skill loop OOM root cause.** `SkillProposalReviewPanel.tsx` had
  `useEffect(..., [open, t])` where `t` was an inline closure recreated
  every render, causing an infinite re-render loop that eventually
  triggered `v8::FatalProcessOutOfMemory`. The fix wraps `t` in
  `useCallback(..., [intl])` so the dep is stable across renders. UI
  suite: 94/94 files pass in 53 s (previously 127 s + 1 file hung).
  (`s2/p0-skill-loop-fixes`)

- **Skill loop tool_calls collection.** `commands.rs::send_message` now
  collects real `tool_call_count` and `tool_names_used` from the
  `QueryEvent::ToolUseRequest` stream (previously a placeholder 0).
  `duration_secs` uses actual elapsed time from `Instant::now()` rather
  than the configured minimum threshold. The skill loop evaluator now
  has the data it needs to judge task complexity accurately.

#### Changes

- **Events moved to `shannon_types::events` (D4).** All 23 event
  payload structs + the `event_names` module now live in the engine
  crate so the shell and engine share a single wire-format contract.
  `src/events.rs` shrinks from 465 to ~160 lines: a re-export plus
  the two Tauri-specific `emit_task_step` / `emit_task_retry` helpers
  that cannot move because they depend on `tauri::AppHandle`. The
  engine also gains `EventEnvelope<T>` and `EVENT_SCHEMA_VERSION = 1`
  for future event families that need forward-compatible negotiation.
  No wire-format change: field names, serde attrs, and event-name
  strings are identical. UI tests 1012/1012 pass; Rust tests 324/324
  pass.

- **Engine pin bumped to `30b4a35`.** Picks up shannon-code PRs #45
  (D4 events) and #46 (D3 API semver — workspace version aligned to
  0.5.5, `STABILITY.md` policy, advisory `cargo-semver-checks` CI).

#### Documentation

- **Skill loop setup guide.** New `docs/user/skill-loop.md` covering
  enable/disable, tuning thresholds, dedup behavior, privacy
  contract, and a troubleshooting table. Distinct from the design
  doc (`docs/architecture/e2-skill-loop.md`) which covers internals.

### P1.1 M1 — Diff review loop (single file)

#### Features

- **Per-hunk accept/reject/undecided controls.** `DiffViewer` renders a
  header pill above each hunk with the current decision (Undecided /
  Accepted / Rejected). Clicking the header cycles the decision:
  pending → accept → reject → pending. Accepted hunks get a tertiary
  left-border accent; rejected hunks get an error accent. Background
  shading on individual add/del lines mirrors the decision so the user
  sees at a glance which changes will land on disk.
  (`s2/p1.1-diff-review-m1` Day 3)

- **Bulk controls: Accept all / Reject all / Reset.** The DiffDialog
  review toolbar surfaces three bulk buttons plus a `decided / total`
  counter so the user can see how many hunks still need attention.

- **Apply flow (client-side merge → save_text_file).** `Apply {N}`
  button in the dialog footer calls `mergeFile(old, new, decisions)`
  (pure function in `lib/diff-merge.ts`) then writes the result via
  the existing `save_text_file` Tauri command. Zero Rust changes —
  the existing `apply_diff` command has different semantics (it only
  blanks out rejected line ranges from the on-disk file) and would
  have required a schema change. Toasts success/failure via `sonner`
  and closes the modal on success.

#### Added

- `ui/src/lib/diff-merge.ts::mergeFile` — pure function that walks
  hunks and emits a merged file string per the decisions Map. Uses
  `diffArrays` from the `diff` package for correct line-level context
  matching (`diffLines` over-groups adjacent edits).
- `ui/src/components/diff/DiffViewer.tsx` — controlled component
  accepting `decisions: Map<string, HunkDecision>` and an optional
  `onToggleHunk(id)` callback. Decision state is owned by the caller.
- `ui/src/components/diff/DiffDialog.tsx` — owns decisions Map state,
  wires bulk controls + Apply footer, toasts results via `sonner`.
- 15 i18n keys (`diff.dialog.apply*`, `diff.review.*`) in en + zh-CN.

#### Tests

- `ui/src/__tests__/diff-merge.test.ts` — 18 unit tests covering
  identical content, replacement, pure insertion/deletion anchoring,
  multi-hunk, mixed accept/reject, trailing-newline preservation,
  stable content-addressed hunk ids.
- `ui/src/__tests__/DiffViewer.test.tsx` — 12 tests including 7
  Day-3 cases (default state pills, accepted/rejected rendering,
  toggle callback, disabled state, multi-hunk).
- `ui/src/__tests__/DiffDialog.test.tsx` — 10 tests including 5
  Apply-flow cases (disabled-until-accepted, all-accept writes
  new_content, all-reject stays disabled, save failure toasts + keeps
  modal open, Cancel closes without saving).

## v0.3.6 (2026-06-22) — S2 P0/P1/P2 + supply-chain hardening

### P0.2 — Per-session worktree

#### Features

- **New session → worktree isolation.** A secondary "New in worktree" button
  in the sidebar creates a new session and immediately provisions a git
  worktree for it (via `shannon_core::scheduled_worktree::create_for_task`).
  The worktree path becomes the session's `working_dir`, so all subsequent
  agent actions in that session are isolated to its own checkout. Mirrors
  Codex Desktop's flagship per-session isolation feature.

#### Added

- `src/commands_sessions.rs::create_session_worktree` — new Tauri command
  wrapping the existing `shannon_core::scheduled_worktree::create_for_task`
  helper. Registered in `main.rs::invoke_handler!`.
- `ui/src/lib/tauri-api.ts::createSessionWorktree` — typed wrapper.
- `ui/src/context/AppContext.tsx::createSessionInWorktree` — orchestrates
  newSession → createSessionWorktree → state refresh.
- `ui/src/components/Sidebar.tsx` — ghost "New in worktree" button below
  the primary "New chat" button.
- `sidebar.worktree.new*` i18n keys (en + zh-CN).

### P0.3 — Auto-updater config

#### Features

- **Tauri auto-updater configured.** `plugins.updater` in `tauri.conf.json`
  now has an endpoint (`https://gitea.diff-lab.com/.../latest/latest.json`)
  and a pubkey placeholder. `bundle.createUpdaterArtifacts` stays **false**
  until an operator generates a real Ed25519 keypair and replaces the
  placeholder — shipping with the placeholder would let the updater accept
  any signature.

#### Docs

- **`docs/updater-setup.md`** walks through the 5-step activation
  (keypair generation, CI secret configuration, pubkey replacement,
  flag flip, `latest.json` publishing). Without this, every Shannon
  Desktop release required users to manually download + reinstall.

### P0 iterations — C1+C2+C3

#### Features

- **Sidebar sessions: drag-and-drop reorder + search (C1+C2).** Sessions
  section in the sidebar now supports drag-to-reorder (persisted to
  localStorage) and fuzzy search by title. Visible limit raised from 5
  to 8. Empty-state message when search returns nothing.
  (`s2/p0-1-sessions-sidebar`)

- **Worktree auto-cleanup on session delete (C3).** `delete_session`
  now inspects the session's `working_dir`; if it lives under the
  default worktree base (`.shannon/scheduled-worktrees/`), the worktree
  is removed via `shannon_core::scheduled_worktree::remove`. Failures
  are logged via `tracing::warn` but do not block session deletion —
  orphan worktrees can be cleaned up via `prune_task_worktrees`.
  (`s2/p0-2-worktree-session`)

### supply-chain-hardening

#### Security

- **Rustup install hardened.** `release.yml::Install Rust` step in the
  build job no longer pipes `sh.rustup.rs` directly to `sh`. Instead it
  downloads the pinned `rustup-init` binary from
  `static.rust-lang.org/rustup/archive/{RUSTUP_VERSION}/{TARGET}/` along
  with the official `.sha256` file, verifies the checksum with
  `sha256sum -c`, then executes. Version is controlled by the
  `RUSTUP_VERSION` env var (currently `1.28.2`) for explicit bump cadence.

- **Supply chain trust model documented.** New `docs/supply-chain.md`
  explains what each dependency source is, what trust basis it relies on,
  and how the hardening measures mitigate supply chain attacks. Includes
  incident response playbook.

#### CI/CD

- **`workflow_dispatch` branch filter.** Manual workflow triggers
  restricted to `branches: [main, dev]` — protected branches only.

### S2 P1.1 commands.rs split + follow-ups

#### CI/CD (release pipeline — v0.3.6 betas)

- **AppImage bundling fixed in rootless DinD (beta7).** `APPIMAGE_EXTRACT_AND_RUN=1`
  env var tells `linuxdeploy` + plugins to extract their own AppImage to a
  temp dir and run from there — pure userspace, no `/dev/fuse` required.
  Costs ~5s per build. Without this, rootless DinD runners cannot
  self-mount AppImages.
- **China mirrors for Linux + macOS release builds (beta8 → beta11).** Cut
  Linux build time from ~95 min to ~48 min by mirroring rustup artifacts,
  crates.io index, and npm registry:
  - **rustup**: `RUSTUP_DIST_SERVER=https://rsproxy.cn` +
    `RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup` (no `/dist` suffix — the
    shell script appends it).
  - **crates.io**: sparse protocol at `sparse+https://rsproxy.cn/index/`.
  - **npm**: `https://registry.npmmirror.com`.
  - Selected rsproxy.cn over tuna/bfsu/USTC because those prune old
    toolchain versions (`rust-1.88.0` returns 404); rsproxy keeps the full
    archive. Three corrections to `RUSTUP_UPDATE_ROOT` were needed (beta9
    `/rustup/rustup` → beta10 `/rustup/dist` → beta11 `/rustup`); the
    rustup-init shell script was the source of truth for the path shape.
  - Mirrors scoped to `runner.os != 'Windows'` (Windows runner not yet
    verified for mirror connectivity).

### superseded by v0.3.6 betas — S2 P1.1 commands.rs split + follow-ups

Multiple extraction PRs to shrink `commands.rs` (~140KB → target ~40KB), plus UI cleanup and CI/docs improvements. All based on `dev` @ `87e854b`.

#### Refactors (commands.rs split — S2 P1.1)

- **Extracted `commands_chat.rs`** (PR #9) — `get_conversation`, `list_models`, `get_status`, `cancel_query`, `list_tools`.
- **Extracted `commands_sessions.rs`** (PR #10) — `new_session`, `list_sessions`, `search_sessions`, `load_session`, `export_session`, `switch_session`, `set_session_working_dir`, `delete_session`, `rename_session`, `duplicate_session`, `branch_session`.
- **Extracted `commands_plugins.rs`** (PR #12) — `list_plugins`, `install_plugin`, `install_plugin_from_git`, `uninstall_plugin`, `enable_plugin`, `disable_plugin`, `update_plugin`, `list_plugin_marketplace`, `list_catalog_upstreams`.
- **Extracted `commands_agents.rs`** (PR #13) — `list_agents`, `list_agent_definitions`, `create_agent_definition`, `delete_agent_definition`, `list_agent_messages`, `list_agent_message_teams`, `record_agent_message`. Fixed missing `#[tauri::command]` on `list_agents`.
- **Extracted `commands_billing.rs`** (PR #14) — `get_billing_plan`, `get_cost_history`, `get_billing_history`.
- **Extracted `commands_permissions.rs` + `commands_files.rs`** (PR #18) — `request_permission`, `respond_permission`, `save_text_file`.

#### Fixes

- Removed stray `.vite/vitest/results.json` artifact and broadened `.gitignore` to cover `.vite` everywhere (was only `/ui/.vite`).
- Fixed i18n-parity CI workflow to use `https://gitea.com/actions/checkout@v4` mirror (runner can't reach github.com).
- Fixed flaky `LspQuickFixPanel` refresh-button test — waited for `disabled === false` before clicking.
- Fixed flaky `Featured` loopback-fallback test — switched from `getByText` to `await findByText`.
- Suppressed `tauri_plugin_shell::Shell::open` deprecation at call site with TODO for `tauri-plugin-opener` migration.
- Added `bundle.icon` array to `tauri.conf.json` — fixes AppImage bundler failure ("couldn't find a square icon").
- Set `bundle.createUpdaterArtifacts` to `false` — updater plugin has empty pubkey/endpoints, so the signing requirement was failing release builds.

#### CI/CD

- **Multi-platform release workflow** (`.gitea/workflows/release.yml`). Matrix builds `.deb`/`.rpm`/`.AppImage` (Linux), `.msi`/`.exe` (Windows), `.dmg` (macOS arm64 + x86_64). GitHub HTTPS traffic routed through `gh-proxy.com` via `git config url.insteadOf`; `shannon-code` sibling cloned at pinned rev `00510a7`. Branch guard rejects non-`main`/non-`v*` tag triggers.

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
