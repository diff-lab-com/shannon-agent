# Shannon Desktop — Comprehensive Review & Improvement Plan (2026-06-26)

**Synthesis of:** `05a-novice-pm-audit-2026-06-26.md` · `05b-ui-design-audit-2026-06-26.md` · `05c-competitive-analysis-2026-06-26.md`
**Branch:** `s2/ui-design-overhaul` (commit `e7724c3`)
**Date:** 2026-06-26

---

## Executive summary

Shannon Desktop has undergone a substantial transformation since the
2026-06-15 baseline audit. The Simple/Dev sidebar split, the 4-step Welcome
wizard, the Extensions Hub redesign, and the billing demo banner are the
highest-impact improvements. The product is now usable for a technical new
user in under 5 minutes (up from ~30 min).

However, three categories of work remain before Shannon can credibly compete
with Claude Desktop, ChatGPT Desktop, WorkBuddy, and Hermes Desktop:

1. **Honesty regressions and dead interactive elements** introduced during
   the redesign sprint. The Header has three dead buttons, the OPC HIL gate
   is incorrect, and the Skills subsystem bypasses the theme engine
   entirely (26 raw color violations across 2 files).
2. **Design-system fragmentation** from a fast-moving sprint. The shared
   `Button` primitive is bypassed on ~15 pages, there is no shared `Modal`/
   `Dialog`/`Card` primitive (25+ hand-rolled modals, 4 card recipes), and
   four incompatible input focus-ring recipes coexist.
3. **Category-level feature gaps** versus competitors. No voice mode, no
   artifact/visual builder, no self-improvement loop, thinner skills
   catalog than Hermes (118+ skills) or WorkBuddy (100+ skills).

This doc merges all three audit lenses, deduplicates overlapping findings,
and proposes a four-phase implementation roadmap (Weeks A-D).

---

## Part 1 — Findings merged and deduplicated

### P0 — Critical honesty bugs (must fix before next release)

| # | Finding | Pages affected | Source |
|---|---------|----------------|--------|
| P0-1 | **Header notifications bell has no onClick handler** — users expect a dropdown panel; clicking does nothing | `Header.tsx:134` | 05a |
| P0-2 | **OPC HIL Approve/Rollback/Revision buttons gate on `hasRunningTasks`** — any running task anywhere shows the panel; Approve may silently no-op if no permission is pending | `OPCTask.tsx:141` | 05a |
| P0-3 | **Skills subsystem uses raw `text-gray-*` / `bg-blue-600` Tailwind** — 26 violations across 2 files; will render broken on every non-default theme | `SkillProposalsToast.tsx`, `SkillProposalReviewPanel.tsx` | 05b |
| P0-4 | **5 Data Sources adapters (Slack, Discord, Telegram, RSS, iCal) install as queryable but are config-only stubs** — "Query coming soon" badge is small and easy to miss | `DataSources.tsx:399` | 05a |

### P1 — High-impact design / UX issues

| # | Finding | Pages affected | Source |
|---|---------|----------------|--------|
| P1-1 | **Header OPC search input has no onChange/onSubmit** — dead input | `Header.tsx:81-87` | 05a |
| P1-2 | **Header avatar circle is non-interactive** — users expect account/profile menu | `Header.tsx:140-142` | 05a |
| P1-3 | **Primary `Button` primitive bypassed on ~15 pages** — 7+ incompatible primary button recipes (`py-2`, `py-sm`, `py-md`, `py-3`, `rounded-lg`, `rounded-xl`, `rounded-full`) | Chat, Profiles, Welcome, OPCTask, Extensions, Tasks, Settings, Hooks, Routines, BillingSettings | 05b |
| P1-4 | **No shared `Modal`/`Dialog` primitive** — 25+ hand-rolled modals with 3 backdrop opacities (30/40/70), 6 z-index values, 4 max-widths | Chat, Header, Triage, Settings (x7), OPC, Memory, Extensions, Diff | 05b |
| P1-5 | **No focus ring on OPCTask revision textarea and Goals search** — only `focus:border-primary`, no ring. WCAG 2.1 SC 1.4.11 violation | `OPCTask.tsx:179`, `Goals.tsx:36` | 05b |
| P1-6 | **Four incompatible input focus-ring recipes** — ring-1/ring-2/ring-3, `primary` vs `ring`, `/20` vs `/30` vs full opacity | Chat, Welcome, Routines, Profiles, Hooks, MyAgents, QuickFix, Editor | 05b |
| P1-7 | **Triage kind labels are hardcoded English** — bypass i18n entirely; zh-CN users see English labels in localized UI | `Triage.tsx:28-36` (`kindMeta()`) | 05a |
| P1-8 | **Tasks page still has 7-widget viewport sprawl** — TaskList + CalendarSidebarWidget + EfficiencyCard + AgentAllocation + HookTaskPipeline + ScheduleDAGView + TaskExecutionLog; widgets compress to <300px on 13" laptops | `Tasks.tsx:252-285` | 05a + 05b |
| P1-9 | **Extensions "Create Agent" CTA navigates to catalog, not a create form** — misleading button label | `Extensions.tsx:33-34` | 05a |
| P1-10 | **Custom `role="button"` divs have no visible focus ring** — keyboard users cannot see where they are (global CSS gap) | Chat session list, Sidebar sessions, Kanban cards | 05b |
| P1-11 | **No shared `Card` primitive** — 4 recipes (standard / glass / accent / gradient) with 4 hover elevations (`shadow-sm` → `shadow-xl`) | OPCTask, Settings, Extensions, Conversations, Memory, Goals | 05b |
| P1-12 | **Sidebar has no badges for actionable items** — pending HIL approvals, failed routines, unread triage are invisible until user visits each page | `Sidebar.tsx` global nav | 05a + 05b |

### P2 — Polish issues

| # | Finding | Pages affected | Source |
|----|---------|----------------|--------|
| P2-1 | `font-headline-sm` token referenced but undefined — silent fallback to base font | `Chat.tsx:413`, `Header.tsx:155` | 05b |
| P2-2 | Icon sizing is raw `text-[Npx]` (140+ occurrences) — no semantic scale; adjacent buttons render at different sizes | All pages, especially Header, Chat, Sidebar | 05b |
| P2-3 | Loading state is a centered spinner everywhere (causes layout shift); `SkeletonLoader` exists but only used in 2 pages | Chat, Extensions (5 tabs), Notifications, Routines, Hooks | 05b |
| P2-4 | Empty states hand-rolled on 5 pages — inconsistent icon sizes (32px / 48px / 28px), no CTA | Chat, Profiles, OPCTask, Memory, Hooks | 05b |
| P2-5 | Off-scale font sizes (`text-[10px]`, `text-[11px]`, `text-[13px]`) — 20+ occurrences below the 12px floor | Welcome, Hooks, Triage, Profiles, Tasks | 05b |
| P2-6 | Elevation has no token scale — `shadow-sm` through `shadow-2xl` used as raw utilities with no documented mapping | All card/modal/dropdown surfaces | 05b |
| P2-7 | Animations: 4 durations (200/300/500/700ms), 3 transition types (`-colors`/`-all`/`-transform`), 4 press-feedback recipes — no tokens | Global | 05b |
| P2-8 | Pinned chat sessions not persisted to backend — local state only, lost on reload | `Chat.tsx:103` (`pinnedIds`) | 05a |
| P2-9 | "Virtualized chat message list" claim is stale — `Chat.tsx:472` uses styled `ScrollArea`, not a real virtualizer library | `Chat.tsx:472` | 05a |
| P2-10 | Welcome Documents skills point to placeholder repo `shannon-agent/shannon-skills-docs` — install will fail | `Welcome.tsx:92, 619-688` | 05a |
| P2-11 | EfficiencyCard shows fabricated % from seeded demo tasks with no "demo" label | `Tasks.tsx:99` | 05a |
| P2-12 | "View System Logs" opens modal with placeholder help text, not actual logs | `AdvancedSettings.tsx:183-199` | 05a |
| P2-13 | Models Settings API key field shows fake masked value `sk-•••••`, not editable inline | `ModelsSettings.tsx:179` | 05a |
| P2-14 | Memory page exposes "confidence" slider and "category" concepts with no explanation — AI-engineering jargon | `MemoryPanel.tsx` | 05a |
| P2-15 | Two parallel session-list implementations (Chat sidebar vs. Sidebar sessions) with different padding/borders/active-state | `Chat.tsx:300-399`, `Sidebar.tsx:94-153` | 05b |
| P2-16 | Chat example prompts are 50% code-focused — non-code examples skew toward "recruiter persona" | `WelcomeState.tsx:7-12` | 05a |
| P2-17 | No warning when switching approval mode to "Full Auto" — border turns red but no confirmation | `ChatInput.tsx:265-284` | 05a |
| P2-18 | Model selector shows raw IDs (`claude-sonnet-4-6`) — no friendly names or "recommended" badges | `ChatInput.tsx:286-308` | 05a |
| P2-19 | "MCP" acronym used throughout with no tooltip or "What is MCP?" link | Extensions, Settings | 05a |
| P2-20 | Nav priority: Memory is top-level in Simple mode but Extensions is Dev-only — arguably inverted | `Sidebar.tsx` | 05a |
| P2-21 | Rename pass incomplete — sidebar says "Workspaces" but Tasks tab still says "Worktrees"; Extensions page title still says "Extensions" not "Integrations" | `Tasks.tsx:48`, `Extensions.tsx` | 05a |
| P2-22 | `EmptyState` action button overrides `Button` primitive — sixth primary-button recipe | `components/ui/empty-state.tsx:24` | 05b |

### P3 — Strategic / competitive feature gaps

| # | Finding | Competitor evidence | Source |
|----|---------|---------------------|--------|
| P3-1 | **No voice mode** — ChatGPT Desktop's defining feature; Claude has voice on mobile; Hermes has voice in CLI + desktop | [ChatGPT Desktop](https://chatgpt.com/features/desktop/) | 05c |
| P3-2 | **No artifact / visual builder** — Claude's Artifacts + Claude Design is a category of output Shannon cannot produce | [Claude Help Center — Artifacts](https://support.claude.com/en/articles/9487310) | 05c |
| P3-3 | **Thinner skills catalog** — Hermes ships 118+ skills + Skills Hub integrating 9 registries; WorkBuddy ships 100+ skills; Shannon's catalog is architecturally richer but content-thin | [Hermes Skills](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills) | 05c |
| P3-4 | **No self-improvement loop** — Hermes's closed learning loop delivers 40% efficiency gain on repetitive tasks (TokenMix benchmark) | [Medium/Tenten — Hermes deep-dive](https://medium.com/@tentenco/hermes-agent-desktop-app-everything-you-need-to-know-about-nous-researchs-self-improving-ai-agent-3cb59bd31e5f) | 05c |
| P3-5 | **Warmer brand polish needed** — Claude's terracotta/cream palette scores higher on "consumer-ready" feel; Shannon's material-blue reads developer-leaning | [PCMag Claude Review](https://me.pcmag.com/en/ai/30779/claude) | 05c |
| P3-6 | **Global shortcut overlay thinner than ChatGPT** — ChatGPT's Option+Space / Alt+Space summons the Chat Bar from anywhere; Shannon's global shortcut exists but no compact overlay window | [ChatGPT Desktop](https://chatgpt.com/features/desktop/) | 05c |

### Shannon's unique strengths (defend these)

From `05c` §"Unique differentiators":

1. **OPC (Operations Control Center)** — strategic-level dashboard for AI agent ops. No competitor has equivalent.
2. **Worktree-based parallel work isolation** — git-level isolation for unattended routines. Unique architecture.
3. **Simple / Advanced sidebar toggle** — dual-persona nav. No competitor does this.
4. **Curated bilingual i18n (English + Chinese)** — not machine-translated. Rare among AI desktops.
5. **Permission profiles as a security primitive** — proactive, pre-configurable tool-access modes. No competitor equivalent.
6. **First-class automations** (hooks + routines + permission profiles) — deeper than WorkBuddy/Hermes scheduling; nonexistent in Claude/ChatGPT Desktop.
7. **Multi-provider freedom with local-first architecture** — Tauri-native (not Electron); Anthropic/OpenAI/Ollama/DeepSeek switchable per-conversation.
8. **Inter-agent message visibility** — `commands_agents.rs` + Tasks Agent Messages panel. WorkBuddy runs parallel agents but they are opaque.

---

## Part 2 — Cross-audit insights

### Where the three audits reinforce each other

- **Header is a hotspot.** 05a flags 3 dead buttons; 05b flags 6 raw numeric
  paddings, 5 mixed icon sizes, undefined `font-headline-sm`. The Header
  deserves a dedicated redesign sprint.
- **Tasks page is overloaded.** 05a flags 7-widget sprawl; 05b flags 5
  competing CTAs in `TasksHeader`, off-scale `text-[13px]` tabs, and widget
  compression on small screens. Both audits agree: collapse secondary
  widgets into tabs.
- **Skills subsystem is the worst design-system offender.** 05b flags 26
  raw color violations; 05c notes Shannon's skills catalog is already thin.
  Breaking the theme on the few skills UIs we have compounds the problem.
- **OPC concept is valuable but opaque.** 05a flags HIL gating bug +
  fabricated efficiency metrics; 05b flags 3 sibling-styled HIL buttons,
  no focus ring on revision textarea, inconsistent `glass-card` usage;
  05c confirms OPC is a unique differentiator worth polishing.

### Where the audits diverge

- 05a says **Memory page is straightforward** with no major honesty gaps;
  05b says **Memory modal uses `bg-surface` (blends with page)** and
  hard-coded English `aria-label="Close"`. Synthesis: Memory is honest but
  has UI polish drift.
- 05a says **Sidebar simple/dev split is the biggest improvement since
  baseline**; 05b says **sidebar is state-blind** (no badges for pending
  approvals, failed routines, triage counts). Synthesis: the split is
  correct, but the sidebar needs state-aware badges to reach its potential.
- 05a flags **Extensions has 7 sub-tabs** as IA bloat; 05c says Shannon's
  Extensions Hub architecture is "technically superior" to Hermes.
  Synthesis: architecture is good, surface is cluttered — consolidate
  Featured + MCP Servers + Plugins into "All Integrations".

### Verified shipped vs. stale claims (from 05a appendix)

**Stale / unverified claims in `pm-audit-followups.md` (need doc update):**

| Claim | Status | Evidence |
|-------|--------|----------|
| Virtualized chat message list | **Stale** | `Chat.tsx:472` uses styled `ScrollArea`, not a virtualizer library |
| OPC HIL buttons gated on pending permission | **Partially fixed** | Gates on `hasRunningTasks`, not `pending_permission` |
| Focus trap on 18 modal dialogs | **Partially fixed** | Escape + click-outside only; no Tab cycling |
| 7 label renames (Extensions→Integrations, Worktrees→Workspaces, etc.) | **Partially fixed** | Sidebar says "Workspaces" but Tasks tab still says "Worktrees", Extensions page title unchanged |

**Verified fixed (confirmed in code):** Chat attach button, Skill cards clickable, `toastError()` surfaces real cause, 6 EmptyState CTAs, ConfirmDialog before destructive ops, Focus-visible rings on native buttons, Billing demo banner, Memory dirty guard, Mod+n keyboard shortcut, Brand icons + micro-interactions, ErrorState primitive.

---

## Part 3 — Phased improvement plan

### Week A — Honesty fixes (P0 + critical P1)

**Theme:** Close the dead-button regressions and the theme-bypass bug before
any more feature work. These are trust-breaking issues.

| # | Task | Estimate | Files |
|---|------|----------|-------|
| A1 | Wire Header notifications bell → dropdown panel showing recent triage items (or navigate to `/triage`) | 2h | `Header.tsx`, `Sidebar.tsx` (reuse triage polling) |
| A2 | Fix OPC HIL gate: change `hasRunningTasks` to `task.pending_permission === true` or active `permissionRequest` from `useApp()` | 1h | `OPCTask.tsx:141` |
| A3 | Skills subsystem theme sweep: swap all 26 `text-gray-*` / `bg-blue-600` / `bg-gray-50` to MD3 semantic tokens (`text-on-surface`, `bg-primary`, `bg-surface-container-lowest`) | 2h | `SkillProposalsToast.tsx`, `SkillProposalReviewPanel.tsx` |
| A4 | Hide 5 config-only Data Sources (Slack/Discord/Telegram/RSS/iCal) from the catalog, or promote the "Query coming soon" badge to a full-card overlay | 1h | `DataSources.tsx:399` |
| A5 | Wire Header OPC search input — either connect to task search or remove the input | 30m | `Header.tsx:81-87` |
| A6 | Fix Triage kind labels i18n — replace literal `kindMeta()` strings with `intl.formatMessage({ id: 'triage.kind.failedRun' })` etc. (update `en.json` + `zh-CN.json` together per convention) | 1h | `Triage.tsx:28-36`, `i18n/locales/*.json` |
| A7 | Remove "virtualized chat" claim from `pm-audit-followups.md` and any release notes (or actually adopt `@tanstack/react-virtual` — out of scope for Week A) | 15m | `docs/product-review/pm-audit-followups.md` |

**Week A exit criteria:** Every interactive-looking element in the Header
and OPC Task pages does what it promises. Every theme renders the Skills
subsystem correctly. Triage labels are localized in zh-CN.

### Week B — Design-system consolidation (P1 architecture)

**Theme:** Extract the primitives that should have existed before the
redesign sprint. Migrate the highest-leverage call sites.

| # | Task | Estimate | Notes |
|---|------|----------|-------|
| B1 | Add `--text-headline-sm: 20px` token to `index.css` (trivial; fixes silent fallback) | 5m | `ui/src/index.css` |
| B2 | Build `<Textarea>` primitive (sibling to `<Input>` with same focus ring) | 30m | `components/ui/textarea.tsx` (new) |
| B3 | Build `<Modal>` / `<Dialog>` primitive with `backdrop`, `size`, escape handling, focus trap (Tab cycling), scroll lock | 4h | `components/ui/modal.tsx` (new) |
| B4 | Build `<Card>` primitive with `variant="elevated" \| "outlined" \| "glass"` and `interactive` prop | 2h | `components/ui/card.tsx` (new) |
| B5 | Build `<ConfirmDialog>` specialized for destructive confirms | 1h | Wraps `<Modal>` |
| B6 | Build `<DropdownMenu>` primitive (accessible `role="menu"`, keyboard nav, consistent shadow) | 3h | `components/ui/dropdown-menu.tsx` (new) |
| B7 | Build `<Badge>` primitive for status pills | 30m | `components/ui/badge.tsx` (new) |
| B8 | Build `<Tooltip>` primitive with `aria-describedby` and delay | 2h | `components/ui/tooltip.tsx` (new) |
| B9 | Extend global focus-visible rule to `[role="button"]`, `[role="option"]`, `[role="tab"]`, `[role="listitem"]` | 15m | `ui/src/index.css:849-857` |
| B10 | Define elevation token scale `--shadow-e1` through `--shadow-e5` with documented mapping (e1=card-rest, e2=card-hover, e3=dropdown, e4=drawer, e5=modal) | 30m | `ui/src/index.css` |
| B11 | Define duration tokens (`--duration-fast/normal/slow/slower`) | 15m | `ui/src/index.css` |
| B12 | Define icon-size utilities (`icon-xs/sm/md/lg/xl/2xl` = 12/16/20/24/32/48px) | 30m | `ui/src/index.css` or Tailwind plugin |
| B13 | Migrate OPCTask revision textarea + Goals search to shared primitives (fixes P1-5 WCAG violation) | 30m | `OPCTask.tsx:179`, `Goals.tsx:36` |

**Week B exit criteria:** All 7 primitives exist with tests. OPCTask and
Goals focus rings are fixed. Global focus-visible rule covers custom ARIA
roles. Token gaps closed.

### Week C — Migration sweep (P1 + P2 cleanup)

**Theme:** Migrate the existing call sites to the new primitives. This is
mechanical work but high-volume.

| # | Task | Estimate |
|---|------|----------|
| C1 | Migrate ~15 hand-rolled primary buttons to `<Button>` — remove `className` overrides except layout | 4h |
| C2 | Migrate ~25 hand-rolled modals to `<Modal>` (Chat x3, Header x1, Triage x2, Settings x7, OPC x1, Memory x1, Extensions x2, Diff x1, others x7) | 6h |
| C3 | Migrate ~10 confirm modals to `<ConfirmDialog>` (Settings x4, Billing x3, Chat x1, Triage x2) | 2h |
| C4 | Migrate Header model selector + MyAgents menu to `<DropdownMenu>` | 2h |
| C5 | Route all form inputs through shared `<Input>` / `<Textarea>` (Chat, Welcome, Routines x7, Profiles, Hooks, MyAgents, QuickFix x5) | 4h |
| C6 | Migrate 5 hand-rolled empty states to `<EmptyState>` primitive (Chat session-list, Profiles custom, OPCTask no-task, Memory, Hooks no-results) | 2h |
| C7 | Replace 30+ status-pill class strings with `<Badge>` component | 2h |
| C8 | Replace 140+ `text-[Npx]` icon sizes with `icon-sm/md/lg` utilities | 3h |
| C9 | Off-scale font audit: replace `text-[10px]`/`text-[11px]`/`text-[13px]` with `label-xs` token (define `--text-label-xs: 11px`) or enforce 12px floor | 2h |
| C10 | Collapse Tasks secondary widgets (EfficiencyCard, AgentAllocation, HookTaskPipeline) into a tabbed "Insights" panel | 4h |
| C11 | Add sidebar badges for pending HIL approvals, failed routines, unread triage items | 3h |
| C12 | Complete rename pass: Tasks tab "Worktrees" → "Branches" / "Workspaces"; Extensions page title → "Integrations" (or commit to reverting the rename) | 1h |

**Week C exit criteria:** Button bypass list is empty. Modal z-index/backdrop
values are consistent. Tasks page fits on a 13" laptop. Sidebar communicates
state.

### Week D — Strategic feature work (P3 + polish)

**Theme:** Close the competitive gaps that materialize as user-facing
features, not just hygiene. Pick from the menu based on product priority.

| # | Task | Estimate | Competitor being chased |
|---|------|----------|-------------------------|
| D1 | **Voice mode** — `useVoice` hook + animated orb UI + provider-specific voice APIs (OpenAI Realtime, Anthropic voice beta) | 2-3 days | ChatGPT Desktop |
| D2 | **Artifact panel** — sandboxed iframe renderer for HTML/React, Mermaid renderer, SVG viewer; right-side panel opens on artifact-eligible output | 3-5 days | Claude Desktop |
| D3 | **Global shortcut overlay window** — compact Shannon chat bar floating above all apps via Tauri multi-window | 2 days | ChatGPT Desktop |
| D4 | **Plan Mode for chat** — agent produces implementation plan before execution; user approves then begins | 1-2 days | Cursor 3 |
| D5 | **Diff-preview-before-apply** — surface the existing `components/diff/` more prominently in chat when code edits are proposed | 1 day | Cursor 3 |
| D6 | **Self-improvement loop** — post-execution evaluation writes procedural skills to `~/.shannon/skills/` with optional write-approval gate | 3-5 days | Hermes Desktop |
| D7 | **Skills catalog growth** — integrate with skills.sh, well-known endpoints, GitHub taps (multi-registry Skills Hub) | 3-5 days | Hermes Desktop |
| D8 | **Companion window for IDE integration** — slim Shannon window that docks to VS Code; surface existing LSP integration | 2-3 days | ChatGPT Desktop |
| D9 | **Warmer visual design pass** — adopt repositioning doc §4.2 palette (terracotta #E8743C or lake green #2A9D8F + cream #F4F3EE) | 3-5 days | Claude Desktop |

**Week D exit criteria:** At least one of {voice, artifact panel, global
shortcut overlay} is shipped behind a feature flag. Plan Mode is available
as a chat toggle.

---

## Part 4 — Recommendation matrix

### If only 1 week is available

Do Week A (honesty fixes). The dead Header buttons and the Skills theme
bypass are the highest-trust-cost bugs. Users will notice and forgive
missing features; they will not forgive buttons that look interactive and
do nothing.

### If 2 weeks are available

Week A + Week B (primitives). Week B is cheap insurance: every week
without shared primitives accumulates more hand-rolled drift. Pay the
once.

### If 4 weeks are available (recommended)

Weeks A + B + C + pick 2-3 items from Week D. Recommended Week D picks:

1. **D4 Plan Mode** — lowest effort, directly improves the agent task UX,
   pairs naturally with Shannon's existing permission profiles.
2. **D5 Diff-preview-before-apply** — Shannon already has `components/diff/`
   and `commands_files.rs`; surfacing it is mostly a UX integration task.
3. **D3 Global shortcut overlay** — Shannon already registers global
   shortcuts via `tauri-plugin-global-shortcut`. The work is adding a
   compact multi-window overlay.

Defer D1 Voice, D2 Artifact panel, D6 Self-improvement, D7 Skills catalog
growth to a future sprint — each is a multi-day investment that deserves
its own discovery + design pass.

### If pursuing consumer positioning (vs. dev-tool positioning)

Add Week D items D1 (Voice), D2 (Artifact panel), D9 (Warmer design) to
the critical path. These are the three gaps that most directly affect
"would a non-technical user choose Shannon over Claude Desktop?"

### If pursuing power-user / dev-tool positioning

Add Week D items D5 (Diff preview), D6 (Self-improvement loop), D7
(Skills catalog growth), D8 (IDE companion window). These compound
Shannon's existing strengths (worktrees, permission profiles, MCP) rather
than chasing competitors on their home turf.

---

## Part 5 — Success metrics

Track these before/after each phase to verify impact:

| Metric | Baseline (2026-06-26) | Target (post Week C) |
|--------|------------------------|----------------------|
| Hand-rolled primary buttons (`grep -rn "bg-primary text-on-primary" ui/src`) | ~22 occurrences | 0 (all via `<Button>`) |
| Hand-rolled modals (`grep -rn 'bg-black/' ui/src`) | ~25 occurrences | 0 (all via `<Modal>`) |
| Off-grid icon sizes (`grep -rn 'text-\[[0-9]px\]' ui/src`) | 140+ | 0 (all via `icon-*` utilities) |
| Raw palette colors in Skills subsystem | 26 | 0 |
| Dead interactive Header elements | 3 | 0 |
| Pages with WCAG 1.4.11 focus-ring violations | 2 (OPCTask, Goals) | 0 |
| Tasks page widget count in default viewport | 7 | 4 (secondary widgets collapsed to tabs) |
| Sidebar items with state-aware badges | 1 (Triage) | 4 (Triage + HIL approvals + Failed routines + Unread agent messages) |
| Novice onboarding time (first-message success) | ~5 min (technical user), ~15 min (non-technical) | 3 min / 8 min |
| Monthly active users who discover Extensions Hub (Simple mode) | unknown | instrumented |

---

## Appendix — Source documents

- `docs/product-review/05a-novice-pm-audit-2026-06-26.md` — Dual-lens audit
  (first-time user + senior PM). 341 lines.
- `docs/product-review/05b-ui-design-audit-2026-06-26.md` — UI/UX designer
  audit with token scorecard, per-page review, top-15 ranked fixes. 927
  lines.
- `docs/product-review/05c-competitive-analysis-2026-06-26.md` — Competitive
  analysis of Claude / ChatGPT / WorkBuddy / Hermes / Cursor / Raycast /
  Supermaven with side-by-side comparison table. 675 lines.
- `docs/product-review/pm-audit-followups.md` — Pre-existing sprint audit
  tracker. Some claims now verified-stale (see Part 2 above).
- `docs/product-review/04-product-repositioning.md` — Earlier repositioning
  strategy doc; references warm palette, Simple/Advanced modes, voice mode
  aspirations.

## Appendix — Update pm-audit-followups.md

The following entries in `pm-audit-followups.md` should be updated to
reflect the 05a verification pass:

- **Virtualized chat message list** — mark STALE. `Chat.tsx:472` is a
  styled ScrollArea, not a virtualizer.
- **OPC HIL buttons gated on pending permission** — mark PARTIALLY FIXED.
  Gates on `hasRunningTasks`, not `pending_permission`. P0-2 in this doc.
- **Focus trap on 18 modal dialogs** — mark PARTIALLY FIXED. Escape +
  click-outside implemented; Tab cycling not verified.
- **7 label renames** — mark PARTIALLY FIXED. Sidebar updated; Tasks tab +
  Extensions page title not updated.

New entries to add to `pm-audit-followups.md` P1 section:
- Header notifications bell dead button (P0-1)
- Header OPC search input dead (P1-1)
- Header avatar non-interactive (P1-2)
- Skills subsystem raw color violations (P0-3)
- Config-only Data Sources shown as installable (P0-4)
- Triage kind labels bypass i18n (P1-7)
