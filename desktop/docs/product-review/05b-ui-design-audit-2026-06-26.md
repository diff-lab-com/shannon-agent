# Shannon Desktop — UI/UX Design Audit (2026-06-26)

**Auditor:** UI/UX designer lens (consumer + dev-tool products).
**Scope:** every page under `ui/src/pages/` and every reusable component under
`ui/src/components/` (primitives in `ui/`, shared, chat, opc, tasks, extensions,
settings, memory, skills, conversations, routines).
**Baseline read:** `docs/product-review/pm-audit-followups.md` — items already
shipped in PR #50/51 are NOT re-flagged here.
**Token reference:** `ui/src/index.css` (MD3 semantic tokens, 14 themes).
**Convention:** all citations are `file:line` against the current branch
`s2/ui-design-overhaul`.

---

## Executive summary (5-8 bullets)

1. **The MD3 token system is real and mostly adopted, but two subsystems never
   got the memo.** The Skills feature
   (`components/skills/SkillProposalsToast.tsx`,
   `components/skills/SkillProposalReviewPanel.tsx`) is written entirely in raw
   `text-gray-*` / `bg-gray-*` / `bg-blue-600` Tailwind — 26 violations across 2
   files — and will look broken on every non-default theme. This is the single
   worst design-system regression in the app.

2. **The shared `Button` primitive exists but is bypassed on roughly half the
   primary CTAs in the product.** Pages hand-roll `<button>` with bespoke
   `bg-primary text-on-primary rounded-lg font-bold` stacks (Chat, Profiles,
   Welcome, Routines, Hooks, OPC Task, extensions). The result is 6+ different
   "primary button" heights/radii/paddings. The primitive's variants
   (`default`, `outline`, `ghost`, `secondary`, `destructive`) are correct but
   underused; `EmptyState` even re-implements a button inside the button file.

3. **Modal/drawer/dialog code is duplicated ~25 times with three incompatible
   recipes.** Recipes: (a) `fixed inset-0 bg-black/30 backdrop-blur-sm` (Header,
   Triage, AdvancedSettings), (b) `fixed inset-0 bg-black/40 backdrop-blur-sm`
   (Chat, McpAddServerDialog, MemoryPanel, Layout sidebar overlay),
   (c) `fixed inset-0 bg-black/70 backdrop-blur-sm` (ResearchReportModal,
   MessageBubble image viewer). Backdrop opacity ranges 30/40/70 — the same
   interaction looks different depending on which page launched it. z-index
   values are scattered (`z-50`, `z-[60]`, `z-[80]`, `z-[85]`, `z-[100]`,
   `z-[200]`) with no documented stacking contract.

4. **Card surfaces are consistent in tokens (`bg-surface-container-lowest
   border-outline-variant/30 rounded-2xl shadow-sm`) but inconsistent in
   elevation.** Cards hover between `shadow-sm`, `shadow-md`, `shadow-lg`,
   `shadow-xl`, `shadow-2xl` with no rule. OPCTask alone uses `shadow-sm` on 7
   sibling cards that all live in the same column — fine — but ConversationsToday
   uses `border-primary/30` accent, MyAgents uses `glass-card`, OPCMissionFocus
   uses `backdrop-blur-md`. There is no "this is a card" primitive.

5. **Icon sizing is raw `text-[Npx]` everywhere (140+ occurrences).** The
   Material Symbols font defaults to 24px; the codebase uses `text-[18px]`,
   `text-[20px]`, `text-[14px]`, `text-[12px]`, `text-[48px]`, `text-[32px]`,
   `text-[16px]`, `text-[24px]`, `text-[28px]` with no semantic mapping.
   Matching icons render at different sizes on adjacent buttons.

6. **Loading state is "a spinning `progress_activity` glyph" everywhere —
   including places where a skeleton is correct.** Chat session list, Hooks,
   Extensions (all 5 tabs), Notifications, Routines all show a centered spinner
   inside already-laid-out chrome, causing layout shift. SkeletonLoader exists
   but is only used in Goals and Triage. The spinner itself is inconsistent:
   some spinners are `text-primary`, some inherit, some are `text-[32px]`,
   some `text-[16px]`.

7. **Input fields have 4 incompatible focus-ring implementations.** The shared
   `Input` primitive (`components/ui/input.tsx:12`) uses
   `focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50`.
   Hand-rolled inputs use `focus:ring-2 focus:ring-primary/30` (Welcome,
   MyAgents), `focus:ring-1 focus:ring-primary/30` (Chat search),
   `focus:border-primary` with no ring (OPCTask revision textarea, Goals
   search), and `focus-visible:ring-2 focus-visible:ring-primary/30` (QuickFix,
   Editor). Four rings, four radii, four colors.

8. **Cross-page flow has two real breaks:** (a) the Header model selector is a
   bespoke dropdown re-implemented per page rather than a shared
   `ModelSelector`, so OPC's search bar and the model chip fight for the same
   header slot; (b) the sidebar badge/dot for pending approvals, failed
   routines, and triage counts is absent — navigation is state-blind, so users
   must manually visit Triage/Tasks/OPC to discover what needs attention.

---

## Design system health

### Token usage scorecard (per page: % semantic vs hard-coded)

Methodology: grep for semantic classes (`bg-surface-*`, `text-on-surface*`,
`text-primary`, `bg-primary`, `border-outline-variant`, etc.) vs hard-coded
Tailwind palette (`text-gray-*`, `bg-blue-*`, raw `#hex`, `from-violet-*`,
`to-sky-*`, etc.). Raw `text-[Npx]` is counted separately under typography.

| Page / Component | Semantic | Hard-coded palette | Score | Worst offender |
|------------------|---------|--------------------|-------|----------------|
| `components/skills/SkillProposalsToast.tsx` | 0 | 5 | **F** | `bg-blue-600`, `text-gray-900` |
| `components/skills/SkillProposalReviewPanel.tsx` | 0 | 21 | **F** | `bg-blue-600`, `bg-gray-50`, `text-blue-700` |
| `components/extensions/Featured.tsx` | mixed | 9 (gradient palette) | **C** | `from-pink-600 to-orange-500` brand swatches — defensible |
| `components/extensions/DataSources.tsx` | mixed | 11 (gradient palette) | **C** | `from-violet-600 to-purple-500` brand swatches — defensible |
| `components/tasks/TaskDAGView.tsx` | low | 5 raw hex | **D** | `'#10b981'`, `'#6366f1'`, `'#ef4444'` |
| `components/tasks/ScheduleDAGView.tsx` | low | 9 raw hex | **D** | `'#94a3b8'`, `'#0f172a'`, `'#10b981'` |
| `components/chat/Chart.tsx` | low | 1 raw hex array | **C** | `PIE_COLORS` palette — acceptable for charts |
| `components/chat/ResearchReportModal.tsx` | low | 8 raw hex (in `<style>`) | **C** | `color: #111`, `background: #f5f5f5` — print/export CSS |
| `pages/Chat.tsx` | high | 5 raw hex (in `<style>`) | **B** | Same print-CSS pattern as ResearchReportModal |
| `pages/Editor.tsx` | high | 2 raw hex with `var()` fallback | **B+** | `'var(--color-error, #b3261e)'` — acceptable |
| `components/settings/notifications/SlackWizard.tsx` | high | 1 raw hex | **A-** | `background_color: '#2C2D33'` — Slack API payload, not styling |
| All other pages | high | 0 | **A** | — |

**Verdict:** Outside of the Skills subsystem and the two DAG canvas views,
token adoption is strong. The Skills files are a P0 regression — they will
break on every dark/theme variant because `bg-white dark:bg-gray-800` is the
only dark-aware pattern and it ignores the app's theme engine entirely.

**Brand-gradient exception (judgment call):** `Featured.tsx:242-254` and
`DataSources.tsx:402-417` define per-brand gradient palettes
(`from-orange-600 to-amber-500` for GitLab, `from-indigo-600 to-violet-500`
for Linear, etc.). This is defensible — brand identity colors cannot be
mapped to MD3 semantic tokens without losing recognizability. Document this
as an intentional exception in a header comment so future contributors do
not "fix" it.

### Spacing consistency

The token scale (`p-xs` / `p-sm` / `p-md` / `p-lg` / `p-xl` / `p-gutter`) is
the dominant pattern and is used correctly in most pages. Drift:

- **`EmptyState` action button** (`components/ui/empty-state.tsx:24`) uses
  `px-lg py-sm` — fine, but it is a one-off primary-button recipe that does
  not match any other primary button.
- **Chat sidebar session rows** (`pages/Chat.tsx:333`) use `p-sm` while
  **Sidebar sessions list** (`components/Sidebar.tsx:134`) uses `px-3 py-2`
  (Tailwind numeric) for the same component. Two sidebars, two paddings.
- **Settings tabs** (`components/ui/tabs.tsx`) and **Extensions sub-tabs**
  (`pages/Extensions.tsx:55`) both use `px-md py-xs` — consistent.
- **Tasks tab switcher** (`pages/Tasks.tsx:189`) uses `px-md py-sm` — one step
  larger than the other two tab implementations.
- **OPCTask cards** universally use `p-xl` — internally consistent.
- **Raw numeric padding** (`p-2`, `py-2`, `px-3`, `py-3`) appears in
  `Header.tsx:66,84,91,134,137`, `BillingSettings.tsx:117-118,176-198`,
  `MyAgents.tsx:142-145`, `Sidebar.tsx:96-110`. These are all in
  hand-rolled-button contexts where the author reached for Tailwind defaults
  instead of the spacing scale.

**Verdict:** Spacing scale is healthy at the page level. The drift is
concentrated in hand-rolled buttons and the two parallel session sidebars.

### Typography scale adherence

The token scale defines `font-body-md`, `font-label-sm`, `font-label-md`,
`font-headline-md`, `font-headline-lg`, `font-display-lg` with matching
`text-*` size tokens. Adoption is good but leaky:

- **`font-bold` / `font-semibold` / `font-medium`** appear ~60 times as raw
  utilities instead of being baked into the label/headline token (the tokens
  already specify `--text-label-sm--font-weight: 500` etc., so adding
  `font-bold` on top is redundant or contradictory). Examples:
  `pages/Tasks.tsx:189`, `pages/Goals.tsx:55-56,208,265-267`,
  `pages/Chat.tsx:303,369,413`, `pages/Triage.tsx:68,269,281,297,319,334`.
- **`text-[Npx]` for icons** is the dominant icon-sizing method (140+
  occurrences across the codebase). There is no `icon-sm` / `icon-md` /
  `icon-lg` semantic. The result: on `pages/Chat.tsx:370-381`, four icon
  buttons in the same row use `text-[14px]`, `text-[14px]`, `text-[14px]`,
  `text-[14px]` — consistent here, but the Header (`Header.tsx:67,86,108,
  135,138,141`) mixes `text-[24px]`, `text-[20px]`, `text-[16px]`,
  `text-[12px]`, `text-[18px]` with no rule.
- **Headlines are mostly tokenized** (`font-headline-md`, `font-headline-lg`)
  — this is the strongest part of the typography system.
- **`font-headline-sm` is referenced but not defined** in `index.css`. The
  `@theme` block defines `headline-md` and `headline-lg` only.
  `pages/Chat.tsx:413` uses `font-headline-sm` and `Header.tsx:155` uses
  `font-headline-sm` — these silently fall back to the base font with no
  size/weight applied.

**Verdict:** Typography is 80% there. The missing `--text-headline-sm` token
is a latent bug. Icon sizing needs a semantic layer.

### Border-radius / elevation consistency

**Border radius:** MD3 recognizes `rounded-md` / `rounded-lg` / `rounded-xl`
/ `rounded-full` at the token level (`index.css:174-180` defines
`--radius-sm` through `--radius-4xl`). The codebase overwhelmingly uses
`rounded-xl` for cards and `rounded-lg` for buttons/inputs — good. Outliers:

- **`rounded-2xl`** appears ~80 times (cards, modals, drawers, message
  bubbles, kanban columns). This is the de-facto card radius even though
  `rounded-xl` is the "documented" one. Pick one.
- **`rounded-3xl`** appears once: `components/extensions/Featured.tsx:150`
  (featured card) — inconsistent with the `rounded-2xl` used on every other
  extensions card.
- **`rounded-full`** is used correctly for pills/chips/avatars.
- **`rounded-md`** is rare and mostly in the shadcn primitives
  (`button.tsx`, `input.tsx`) and the Sidebar sessions search
  (`Sidebar.tsx:110`).

**Elevation (shadow):** No documented elevation scale. The codebase uses
`shadow-sm`, `shadow-md`, `shadow-lg`, `shadow-xl`, `shadow-2xl` as raw
Tailwind utilities with no semantic mapping. Patterns observed:

- Cards at rest: `shadow-sm` (universal).
- Cards on hover: `shadow-md` (Settings, Extensions) or `shadow-lg`
  (DataSources) or `shadow-xl` (Featured) — three different hover elevations
  for the same interaction.
- Modals/dialogs: `shadow-2xl` (universal — good).
- Dropdowns/popovers: `shadow-xl` (Header model selector) or `shadow-lg`
  (MyAgents menu) — inconsistent.
- `glass-card` / `glass-panel` utilities (`index.css:136-140`) apply
  `backdrop-filter: blur()` with no shadow token, so glass cards have zero
  elevation and visually merge with the background on light themes.

**Verdict:** Radius is binary (`rounded-xl` for buttons, `rounded-2xl` for
cards — document this). Elevation needs a 5-level token scale
(`shadow-e1` through `shadow-e5`) or at minimum a documented mapping
(resting=sm, hover=md, dropdown=lg, modal=2xl).

---

## Component audit

### Button hierarchy (find pages with competing primaries)

The shared `Button` (`components/ui/button.tsx`) defines 6 variants
(`default`, `outline`, `ghost`, `secondary`, `destructive`, `link`) and 7
sizes. The `default` variant is the primary. Problems:

1. **Bypass rate is ~50%.** Primary CTAs are hand-rolled as raw `<button>` in:
   - `pages/Chat.tsx:303` — "New Chat" (`py-2 bg-primary text-on-primary
     rounded-lg font-bold`, no size token).
   - `pages/Profiles.tsx:126-133` — "New Profile" (`px-lg py-sm bg-primary
     text-on-primary rounded-lg font-label-md`).
   - `pages/Welcome.tsx:335,388` — "Continue"/"Test Connection"
     (`px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md`).
   - `pages/Hooks.tsx:118` — (toggle create form).
   - `pages/Routines.tsx:118` — (toggle create form).
   - `pages/OPCTask.tsx:154-160` — "Approve Final Merge"
     (`px-md py-sm rounded-xl bg-primary text-on-primary`).
   - `pages/Extensions.tsx:79-83` — CTA (`px-lg py-sm rounded-full`).
   - `pages/Tasks.tsx` — via `TasksHeader` (not visible in slice, but the
     page has both "New Background Task" and calendar/list/DAG toggles
     competing for attention — see issue 2 below).
   - `components/extensions/Featured.tsx:200` — gradient CTA.
   - `components/extensions/MyAgents.tsx:226` — "Spawn".
   - `components/settings/BillingSettings.tsx:117` — "Change Plan"
     (`py-3 px-4 ... rounded-xl font-bold`).

   Each of these re-specifies the primary button recipe. The result: primary
   buttons are `py-2`, `py-sm`, `py-md`, `py-3`, `px-lg`, `px-md`, `px-4`,
   `rounded-lg`, `rounded-xl`, `rounded-full` — no two match.

2. **Tasks page has two competing primary CTAs.** `TasksHeader` renders
   "New Background Task" (primary) alongside calendar/list/DAG view toggles.
   When `showNewTask` is also true, the page shows the primary CTA, the view
   toggles, the tab switcher, the filter toggle, and the new-task form all
   above the fold. There is no single obvious next action.

3. **OPCTask has three primary-styled buttons in the HIL block**
   (`pages/OPCTask.tsx:154-174`): "Approve Final Merge" (filled primary),
   "Rollback" (outlined error), "Request Revision" (outlined neutral).
   Approve is correctly the strongest, but all three use `w-full px-md py-sm
   rounded-xl font-label-md` — they look like siblings, not a primary + two
   secondaries.

4. **`EmptyState` re-implements a primary button**
   (`components/ui/empty-state.tsx:24`): `<Button className="mt-lg bg-primary
   text-on-primary px-lg py-sm rounded-xl font-label-md cursor-pointer">`.
   This overrides the `Button` primitive's own `default` variant styling,
   producing a button that does not match any other primary in the app.

**Fix:** Migrate all hand-rolled primary buttons to `<Button>` (no className
override except layout). Add a `size="lg"` variant if the current `lg`
(`h-9`) is too short. Make `EmptyState` use `<Button size="lg"
className="mt-lg">` with no color override.

### Form fields (consistency scorecard)

| Field type | Where used | Focus ring | Height | Verdict |
|-----------|-----------|------------|--------|---------|
| Shared `Input` (`ui/input.tsx`) | Extensions search, Header search, Goals search, Chat session search | `focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50` | `h-8` | **A** (but `h-8` is short for a search field) |
| Chat session search (`Chat.tsx:312`) | Chat sidebar | `focus:ring-1 focus:ring-primary/30` | `py-xs` | **C** (ring-1, different color) |
| Header OPC search (`Header.tsx:84`) | Header | `focus:ring-2 focus:ring-primary/20` | `py-2` | **C** (ring-2, /20 opacity) |
| Welcome inputs (`Welcome.tsx:431`) | Onboarding | `focus:ring-2 focus:ring-primary outline-none` | `py-sm` | **C** (ring-2, no /opacity) |
| Routines/Profiles/Hooks inputs | Forms | `focus:ring-2 focus:ring-primary outline-none` | `py-sm` | **C** |
| MyAgents create form (`MyAgents.tsx:180`) | Agent create | `focus:ring-2 focus:ring-primary/30` | `p-sm` | **C** |
| OPCTask revision textarea (`OPCTask.tsx:179`) | HIL revision | `focus:border-primary` (no ring) | `py-sm` | **D** (no ring at all) |
| Goals search (`Goals.tsx:36`) | Goals sidebar | `focus:border-primary` (no ring) | `py-2` | **D** |
| QuickFix inputs (`QuickFix.tsx:58-105`) | Quick fix form | `focus-visible:ring-2 focus-visible:ring-primary/30` | `py-xs` | **B** (closest to primitive) |

**Four focus-ring recipes, four heights.** The shared `Input` is correct but
underused — most pages reach for a raw `<input>` or `<textarea>` with a
hand-rolled ring. The OPCTask revision textarea and Goals search have **no
focus ring at all**, only a border color change — this is an accessibility
regression (WCAG 2.1 SC 1.4.11: focus must be visible).

**Fix:** Route all text inputs through the shared `Input` primitive (or a
`Textarea` sibling that does not exist yet). Delete the hand-rolled
`focus:ring-*` classes.

### Cards / modals / empty states (pattern drift list)

**Cards — 4 incompatible recipes:**

| Recipe | Where | Tokens |
|--------|-------|--------|
| Standard card | OPCTask, Editor, QuickFix, Settings sections, OPC analytics | `bg-surface-container-lowest border border-outline-variant/30 rounded-2xl shadow-sm` |
| Glass card | OPCMissionFocus, MyAgents, ConversationsToday hero, Chat context panel | `glass-card bg-surface-container-lowest/70-80 backdrop-blur-md` |
| Accent card | Extensions (Plugins, DataSources, Featured) | `bg-surface-container-lowest hover:border-primary/40 hover:shadow-lg` |
| Gradient hero | ConversationsToday, EfficiencyCard | `bg-gradient-to-br from-primary-container/40 ...` or `bg-primary text-on-primary` |

Glass cards have no shadow and merge into the background on the default light
theme. The standard card is correct and should be the primitive.

**Modals — 3 backdrop recipes (as noted in summary):**

| Backdrop | z-index | Where |
|----------|---------|-------|
| `bg-black/30 backdrop-blur-sm` | `z-50` to `z-[200]` | Header perm, Triage, AdvancedSettings, BillingSettings, CancelTaskModal, OPCAgentSwarm, DiffDialog |
| `bg-black/40 backdrop-blur-sm` | `z-[60]` to `z-[85]` | Layout sidebar overlay, Chat delete/export, McpAddServerDialog, MemoryPanel, HookRoutineCreateDialog |
| `bg-black/70 backdrop-blur-sm` | `z-50` | ResearchReportModal, MessageBubble image viewer |

A `Modal` / `Dialog` primitive that takes `backdrop` and `z` props would
eliminate this. The `Drawer` primitive (`components/ui/drawer.tsx`) exists
but uses `bg-black/50` (a fourth value) and `p-6` (raw numeric) and is only
used by `SkillDetailDrawer` — every other "drawer" is hand-rolled.

**Empty states — pattern is good but leaky:**

The `EmptyState` primitive (`components/ui/empty-state.tsx`) renders
`icon (48px) + title (font-body-lg font-bold) + description + action button`.
It is used on 6+ pages (Goals, Triage, WorktreePanel, OPCAgentSwarm,
ExtensionsHub, MyAgents). Leaks:

- `pages/Chat.tsx:322-325` — session-list empty state is hand-rolled
  (`text-[32px]` icon, `text-body-sm` text, no title, no CTA).
- `pages/Profiles.tsx:186-190` — custom-profiles empty state is hand-rolled
  (`text-[48px]` icon, `font-headline-md` title, `font-body-sm` description,
  no CTA).
- `pages/OPCTask.tsx:93-97` — "no task selected" empty state is hand-rolled
  (icon + text, no title hierarchy, no CTA).
- `components/memory/MemoryPanel.tsx:252` — memory empty state is hand-rolled.
- `pages/Hooks.tsx:109` — hooks no-results is hand-rolled.

Five hand-rolled empty states alongside six that use the primitive. The
primitive's icon size is `text-[48px]`; the hand-rolled ones use `text-[32px]`
(Chat), `text-[48px]` (Profiles, Memory), `text-[28px]` (Goals sidebar) —
three icon sizes for the same pattern.

### Loading / error states (inconsistency list)

**Loading:**

- **Spinner glyph** (`material-symbols-outlined animate-spin
  progress_activity`) is the universal loading indicator. Used ~30 times.
  Inconsistencies:
  - Size: `text-[32px]` (most pages), `text-[16px]` (DiffDialog apply,
    McpAddServerDialog), `text-[14px]` (Welcome skill install, AdvancedSettings
    clear), `text-[20px]` (ModelsSettings), `text-[18px]` (AdvancedSettings
    clear button inline).
  - Color: `text-primary` (most), inherited (DiffDialog, MyAgents,
    DataSources, Skills, Agents, Plugins), `text-[32px] text-primary`
    (Hooks, Plugins, Extensions, Notifications).
  - Container: some pages center it in a `py-xl` div (correct); others inline
    it next to a button label (AdvancedSettings clear, ModelsSettings test).
- **SkeletonLoader** (`components/SkeletonLoader.tsx`) exists with a
  `CardSkeleton` export. Used only in Goals (`pages/Goals.tsx:98`) and Triage.
  Chat session list, Extensions tabs, Notifications, Routines, Hooks all show
  a centered spinner inside already-rendered chrome — causing the surrounding
  cards to collapse and re-expand when data arrives.
- **No loading timeout.** Every spinner spins forever if the backend hangs.
  No "this is taking longer than expected" fallback.

**Error states:**

- **Inline error banner** (`pages/Tasks.tsx:206-216`): `bg-error/10 border
  border-error/20 text-error rounded-xl` with a close button — good pattern,
  used once.
- **Toast** (`sonner`): used everywhere via `toast.error()` and the
  `toastError()` helper. 23 calls pass generic strings like `'Failed'`
  (per PM audit §B).
- **ErrorBoundary** (`components/ErrorBoundary.tsx`): renders a full-page
  `text-error` icon + message. Good.
- **Inline form errors**: MyAgents (`MyAgents.tsx:185`) uses
  `text-error text-label-sm` below the field — correct but unique; no other
  form does this.
- **Chat error retry** (`pages/Chat.tsx:526`): `variant="ghost"
  text-error hover:bg-error/10` — a ghost-styled error button, not a primary
  retry CTA. Weak affordance for "try again".

**Verdict:** Loading needs a `LoadingState` primitive that renders either a
spinner (for inline/button contexts) or a skeleton (for list/card contexts)
based on a `variant` prop. The skeleton variant should be the default for
any page that renders into a card grid. Error states need a shared
`ErrorBanner` primitive (the Tasks pattern is good, use it everywhere).

---

## Per-page visual review

### Chat (`pages/Chat.tsx`)

**Strengths:**
- Three-pane layout (sessions / messages / context) is well-structured.
- Message bubbles use correct MD3 tokens (`bg-primary-fixed` for user,
  `bg-surface-container-lowest` for assistant).
- Pinned-message UI, export/print actions, and session rename are thoughtful.
- Streaming indicator (`w-2 h-5 bg-primary/60 animate-pulse`) is a nice
  typing-cursor affordance.

**Issues:**
- **P1** `Chat.tsx:303` — "New Chat" button bypasses `<Button>` primitive;
  uses `py-2 ... rounded-lg font-bold` (raw numeric padding, wrong radius
  for a primary — should be `rounded-xl` to match other primaries).
- **P1** `Chat.tsx:312` — session search uses `focus:ring-1
  focus:ring-primary/30` (ring-1 is too thin; primitive uses ring-3).
- **P1** `Chat.tsx:333-337` — session row uses `p-sm rounded-lg ... border-l-2`
  while the Sidebar's equivalent (`Sidebar.tsx:134`) uses `px-3 py-2
  rounded-lg`. Two session-list components, two paddings, two borders
  (left-border active indicator vs. background-tint active indicator).
- **P2** `Chat.tsx:370-382` — four icon buttons (pin, export, print, more) all
  at `text-[14px]` — consistent, but there is no visible separator or
  group affordance; they appear on hover with `opacity-0 group-hover:opacity-100`.
- **P2** `Chat.tsx:413` — uses `font-headline-sm` which is not defined in
  `index.css`. Silently falls back.
- **P2** `Chat.tsx:585,610,639` — three modals with three different max-widths
  (`max-w-sm`, `max-w-3xl`, `max-w-5xl`) and two different backdrops
  (`bg-black/30`, `bg-black/40`).
- **P2** `Chat.tsx:660` — context panel uses `glass-panel` with
  `bg-surface-container-lowest/50` — on the default light theme this is
  nearly invisible against the `bg-background`.

### OPC Task (`pages/OPCTask.tsx`)

**Strengths:**
- Card layout is token-consistent (`bg-surface-container-lowest
  rounded-2xl border-outline-variant/30 shadow-sm` on every card).
- Agent workflow stepper with active/inactive states is clear.
- Efficiency metrics grid is well-organized.
- Breadcrumb nav at top is correct.

**Issues:**
- **P1** `OPCTask.tsx:154-174` — three HIL buttons all use `w-full px-md py-sm
  rounded-xl font-label-md`. Approve is filled-primary (correct as strongest),
  but Rollback and Request Revision look like equal-weight siblings. Rollback
  should be `variant="outline"` with error styling; Request Revision should be
  `variant="ghost"`.
- **P1** `OPCTask.tsx:179` — revision textarea has `focus:outline-none
  focus:border-primary` with **no focus ring** — WCAG 1.4.11 violation.
- **P2** `OPCTask.tsx:37,75,104,145` — section headings use
  `font-headline-md text-[20px] font-bold` — the `text-[20px]` overrides the
  token's `--text-headline-md: 24px`. Either use the token or define a
  `headline-sm` token at 20px.
- **P2** `OPCTask.tsx:142` — HIL card uses `glass-card` while every other card
  on the page uses the standard card recipe. Inconsistent within the same
  page.
- **P2** `OPCTask.tsx:220-223` — progress bar gradient
  (`from-primary/60 to-primary`) is the only gradient progress bar in the app;
  Goals and Tasks use flat `bg-primary`.

### Tasks (`pages/Tasks.tsx`)

**Strengths:**
- Tab switcher (Active/History/Worktrees) is clear.
- Error banner pattern (`bg-error/10 border-error/20`) is the best in the app
  — should be extracted to a primitive.
- Calendar/DAG/List view toggle gives users power-user control.

**Issues:**
- **P0/P1** Competing primary CTAs: `TasksHeader` renders "New Background
  Task" (primary) + "New Schedule" + calendar toggle + DAG toggle + filter
  toggle. When the new-task form is open, the header has 5 interactive
  controls above the task list. No single obvious next action.
- **P1** `Tasks.tsx:189` — tab buttons use `text-[13px] font-bold` — `13px`
  is not in the type scale (scale jumps 12 -> 14). Off-grid.
- **P2** `Tasks.tsx:252-285` — the right-rail widget stack
  (CalendarSidebarWidget + EfficiencyCard + AgentAllocation + HookTaskPipeline)
  is dense. On a 1280px laptop with the sidebar open, these widgets compress
  to <300px wide and become unreadable. PM audit §Tasks already flags this.

### Goals (`pages/Goals.tsx`)

**Strengths:**
- Task-tree timeline with status dots is a nice visual.
- Agent sidebar with connector line is polished.
- Empty state uses the `EmptyState` primitive correctly.

**Issues:**
- **P1** `Goals.tsx:36` — search input has `focus:border-primary` with no ring
  — WCAG 1.4.11 violation (same as OPCTask).
- **P1** `Goals.tsx:55-56,208,265-267` — raw `font-bold` on top of
  `font-label-md` (which already sets `font-weight: 500`). The `font-bold`
  overrides to 700, making these labels heavier than the design system
  specifies.
- **P2** `Goals.tsx:53` — active task card uses `bg-primary/10 border-primary/20`
  while the pending card uses `hover:bg-surface-container-high/60`. Two
  different active/hover recipes on the same list.
- **P2** `Goals.tsx:162` — task card uses `glass-card` + `ring-1 ring-primary/10`
  — the only place `ring` is used for card emphasis. Everywhere else uses
  `border` or `shadow`.

### Triage (`pages/Triage.tsx`)

**Strengths:**
- Filter chips (kind/read/archived) are consistent and tokenized.
- Bulk-action bar with `backdrop-blur-md` is a good sticky pattern.
- Uses `EmptyState` and `CardSkeleton` primitives correctly.
- Severity color mapping (`text-error`, `text-primary`, `text-secondary`) is
  semantic.

**Issues:**
- **P1** `Triage.tsx:68` — kind label uses `text-[11px] font-bold uppercase
  tracking-wider` — `11px` is off the type scale (12 -> 14). Also `uppercase
  tracking-wider` is applied per-element rather than via a token.
- **P2** `Triage.tsx:52` — card uses `glass-panel` + `bg-surface-container-lowest/80`
  — glass on a list item causes readability issues when the list scrolls
  behind it.
- **P2** `Triage.tsx:438,477` — two confirm modals, both `bg-black/30`, both
  `rounded-2xl shadow-xl` — consistent with each other but not with Chat's
  modals (`bg-black/40`).

### Extensions (`pages/Extensions.tsx` + `components/extensions/*`)

**Strengths:**
- Sub-tab nav is clean and tokenized.
- Search + CTA layout is responsive (stacks on narrow widths).
- Brand-gradient icon system (Featured, DataSources) is visually
  distinctive and defensible.
- `SkillDetailDrawer` uses the `Drawer` primitive correctly.

**Issues:**
- **P1** `Extensions.tsx:79` — CTA button uses `rounded-full` while every other
  primary in the app uses `rounded-lg` or `rounded-xl`. Pill-shaped primary
  in a rectangular UI.
- **P1** `MyAgents.tsx:142` — "Configure" button uses
  `bg-surface-variant/50` — `surface-variant` is not a background token (it
  is `on-surface-variant`, a text color). This renders as a transparent
  muddy color on most themes.
- **P2** `Featured.tsx:150` — featured card uses `rounded-3xl` while
  Plugins/DataSources/Agents cards use `rounded-2xl`. One step larger radius
  on one card type.
- **P2** `Featured.tsx:200` — CTA uses a gradient (`bg-gradient-to-r
  ${accent.button}`) — beautiful but it is a sixth primary-button recipe.
- **P2** `MyAgents.tsx:148` — dropdown menu uses `shadow-lg` while Header's
  model selector dropdown uses `shadow-xl`. Two dropdown elevations.

### Settings (`pages/Settings.tsx` + `components/settings/*`)

**Strengths:**
- Section-card pattern (`bg-surface-container-lowest rounded-xl border
  outline-variant/30 p-xl shadow-sm`) is the most consistent in the app.
- Theme picker with live preview swatches is excellent.
- Font-size picker with live preview is excellent.
- Confirm modals are consistent within Settings (all `bg-black/30`).

**Issues:**
- **P1** `BillingSettings.tsx:117-118` — "Change Plan" and "Cancel" buttons use
  `py-3 px-4` (raw numeric) and `font-bold` — bypass the primitive and use
  off-grid spacing.
- **P1** `AdvancedSettings.tsx:62,80,108,135,167` — section headings use
  `font-headline-md text-[24px] font-bold` — the `text-[24px]` is redundant
  (token already sets 24px) and `font-bold` overrides the token's 600 to 700.
- **P2** `AdvancedSettings.tsx:172` — destructive "Reset" button uses
  `px-xl py-md bg-error ... rounded-xl font-bold shadow-md` — should be
  `variant="destructive"`. Currently it is a hand-rolled seventh primary
  recipe.
- **P2** `ModelsSettings.tsx:59` — model tab uses `ring-1 ring-black/5
  font-bold` — `ring-black/5` is a hard-coded color, not semantic. Breaks
  on dark themes.
- **P2** `ThemeSettings.tsx:25,70,97` — sections use `rounded-xl` while
  BillingSettings sections use `rounded-2xl`. Two card radii in the same
  Settings shell.

### Welcome / Onboarding (`pages/Welcome.tsx`)

**Strengths:**
- Multi-step wizard with progress indicator is well-structured.
- Provider preset cards (Anthropic, Ollama) with test-connection feedback.
- Skill-install step with per-skill progress is delightful.
- Keyboard shortcut reference at the end is a nice touch.

**Issues:**
- **P1** `Welcome.tsx:335,388` — primary buttons use `px-lg py-sm bg-primary
  text-on-primary rounded-lg font-label-md` — bypass primitive, wrong radius
  (`rounded-lg` vs `rounded-xl` elsewhere).
- **P1** `Welcome.tsx:431` — API key input uses `focus:ring-2 focus:ring-primary
  outline-none` — `ring-primary` at full opacity (no `/30`) is heavier than
  every other input in the app.
- **P2** `Welcome.tsx:497` — "RECOMMENDED" badge uses `text-[10px]` — off the
  type scale (smallest token is 12px).
- **P2** `Welcome.tsx:587,596` — `<kbd>` elements use `text-[11px]` — off the
  scale.

### Profiles (`pages/Profiles.tsx`)

**Strengths:**
- Builtin-vs-custom split is clear.
- Tool-capability badges with auto/needs-approval states are informative.
- Custom-profile create form is compact.

**Issues:**
- **P1** `Profiles.tsx:126-133` — "New Profile" button bypasses primitive.
- **P1** `Profiles.tsx:273-309` — all form inputs use `focus:ring-2
  focus:ring-primary outline-none` — does not match the shared `Input`
  primitive's ring.
- **P2** `Profiles.tsx:186-190` — empty state for custom profiles is
  hand-rolled instead of using `EmptyState`.
- **P2** `Profiles.tsx:160` — capability badge uses `text-[10px]` — off scale.

### Hooks (`pages/Hooks.tsx`)

**Strengths:**
- Category filter chips are tokenized and accessible.
- Event-card layout with code-formatted field names is dev-tool-appropriate.

**Issues:**
- **P1** `Hooks.tsx:88` — search input uses `focus:ring-2 focus:ring-primary
  outline-none` — does not match primitive.
- **P2** `Hooks.tsx:109` — no-results empty state is hand-rolled.
- **P2** `Hooks.tsx:122,127` — category badge and field code use `text-[10px]`
  and `text-[11px]` — off scale.

### Routines (`pages/Routines.tsx`)

**Strengths:**
- Routine list with enable/disable toggles is clear.
- Create form is compact.

**Issues:**
- **P1** `Routines.tsx:118` — toggle-create button bypasses primitive.
- **P1** `Routines.tsx:217-263` — seven form inputs all use `focus:ring-2
  focus:ring-primary outline-none` — does not match primitive. Seven
  opportunities to migrate.
- **P2** `Routines.tsx:110` — subtitle code element uses `text-[12px]` — on
  scale (label-sm) but applied as raw px.

### Editor / QuickFix (`pages/Editor.tsx`, `pages/QuickFix.tsx`)

**Strengths:**
- Three-pane Editor (file tree / editor / diagnostics) is well-structured.
- QuickFix form layout is compact and labeled.
- Diff dialog is polished.

**Issues:**
- **P1** `Editor.tsx:376-378` — diagnostic severity color uses raw hex with
  `var()` fallback (`'var(--color-error, #b3261e)'`) — acceptable pattern but
  the `--color-warning` fallback (`#7c5800`) does not match the token
  (`--color-tertiary: #855000`). Two different "warning" colors.
- **P2** `QuickFix.tsx:58-105` — five inputs all use `focus:outline-none
  focus-visible:ring-2 focus-visible:ring-primary/30` — closest to the
  primitive but still hand-rolled.
- **P2** `QuickFix.tsx:118` — submit button uses `rounded-full` — pill shape,
  inconsistent with the `rounded-xl` / `rounded-lg` used elsewhere.

### Memory (`components/memory/MemoryPanel.tsx`)

**Strengths:**
- Category-color icon system is a nice visual mnemonic.
- Add/edit/delete flow is compact.

**Issues:**
- **P1** `MemoryPanel.tsx:435` — modal uses `bg-surface rounded-2xl border
  border-outline-variant shadow-2xl` — `bg-surface` (not
  `bg-surface-container-lowest`) makes the modal blend with the page
  background. `border-outline-variant` at full opacity (no `/30`) is heavier
  than other modals.
- **P2** `MemoryPanel.tsx:445` — close button has `aria-label="Close"` (hard-coded
  English) instead of using i18n.

### Conversations (`components/conversations/*`)

**Strengths:**
- "Today" hero card with gradient is visually appealing.
- List + detail layout is standard.

**Issues:**
- **P2** `ConversationsToday.tsx:92` — hero uses `bg-gradient-to-br
  from-primary-container/40 via-primary/10 to-transparent` — the only
  multi-stop gradient in the app. Beautiful but unique.
- **P2** `ConversationsToday.tsx:94` — icon container uses `rounded-2xl`
  while Header avatar (`Header.tsx:140`) uses `rounded-full`. Two avatar
  shapes.

### Mission Control (`pages/MissionControl.tsx`)

**Strengths:**
- Uses the shared `KanbanBoard` primitive correctly.
- Summary header with totals is clear.

**Issues:**
- **P2** `MissionControl.tsx:97` — total uses raw `font-bold` on top of
  `text-on-surface`.
- **P2** No filtering (team / assignee / due date) — already flagged in PM
  audit; from a design perspective, the lack of filter chips makes the
  header look empty.

---

## Cross-page flow coherence

### Navigation state persistence

- **Sidebar session list** persists order via `localStorage`
  (`Sidebar.tsx:48-53`) — good.
- **Tasks tab** (Active/History/Worktrees) does NOT persist — switching away
  and back resets to "Active". `Tasks.tsx:182` uses local `useState`.
- **Triage filters** (kind/read/archived/sort) do NOT persist — `Triage.tsx`
  uses local state.
- **Extensions sub-tab** persists via URL (`NavLink` to `/extensions/...`) —
  correct.
- **Chat context panel** open/close state does NOT persist —
  `Chat.tsx:contextPanelOpen` is local state.

**Verdict:** URL-backed state (Extensions) survives navigation; local state
(Tasks, Triage, Chat context) does not. Users lose their place when they
tab-switch.

### Shared component reuse (model selector, toasts, etc.)

- **Model selector:** Implemented once in `Header.tsx:99-131` as an inline
  dropdown. Not reusable — it reads from `useApp()` directly. Other pages
  cannot embed it. This is fine as long as the Header is always present, but
  the Welcome page (`/welcome`) renders without the standard Header, so
  onboarding users have no model selector.
- **Toasts:** `sonner` is used app-wide via `toast.success` / `toast.error` /
  `toastError()` — consistent.
- **Empty states:** `EmptyState` primitive exists but is used on only ~50%
  of empty-state opportunities (see above).
- **Confirm dialogs:** No shared `ConfirmDialog` primitive at the UI layer
  (PR #50 added one per the PM audit, but it is not in `components/ui/` — it
  appears to be logic-only or was inlined). Every confirm is hand-rolled.
- **Dropdown menu:** No shared primitive. `MyAgents.tsx:148` and the Header
  model selector are both hand-rolled `absolute right-0 top-full` divs with
  different shadows and borders.

### Flow breakage points

1. **Welcome -> Chat:** Welcome renders without Header/Sidebar. After
   onboarding completes, the app navigates to `/chat` and the full chrome
   appears. The transition is abrupt — no animation, no "welcome to your
   workspace" moment.
2. **OPC -> OPC Task:** The OPC board (`/opc`) and task detail (`/opc/task/:id`)
   share the Header but the OPC board has a search bar in the header while
   the task detail has a "sync status" pill. The header changes content
   between sibling pages — disorienting.
3. **Chat -> QuickFix / Editor:** Launched as modals from Chat
   (`Chat.tsx:602,631`). The modals are `max-w-3xl` and `max-w-5xl`
   respectively — nearly full-screen. They feel like page navigations trapped
   in a modal. No breadcrumb back to Chat.
4. **Sidebar badges:** The sidebar shows session count and triage stats
   (polled every 30s, `Sidebar.tsx:174-178`) but does NOT show badges for:
   pending approvals (HIL), failed routines, or unread triage items. Users
   must manually visit each page to discover actionable items.

---

## Micro-interaction audit

### Hover/focus/active/disabled states coverage

| Element | Hover | Focus | Active | Disabled | Verdict |
|---------|-------|-------|--------|----------|---------|
| Shared `Button` | `hover:bg-primary/80` (default variant) | `focus-visible:ring-3` | `active:translate-y-px` | `disabled:opacity-50` | **A** |
| Hand-rolled primary buttons | `hover:bg-primary/90` or `hover:shadow-md` or `hover:brightness-110` | inconsistent | `active:scale-95` or `active:scale-[0.98]` or none | `disabled:opacity-50` (mostly) | **D** (3 different hover recipes, 3 different active recipes) |
| Cards | `hover:shadow-md` or `hover:shadow-lg` or `hover:border-primary/40` or `hover:-translate-y-1` | none | none | n/a | **C** (4 hover recipes) |
| Icon buttons (ghost) | `hover:bg-surface-container-low hover:text-primary` | `focus-visible:ring-2 ring-primary/30` (most) | none | none | **B** |
| List rows (sessions, triage) | `hover:bg-surface-container-high/40-60` | none on most | none | n/a | **C** (no focus ring on clickable rows — `role="button" tabIndex={0}` without focus styles) |
| Tab buttons | `hover:text-on-surface` | `focus-visible:ring-2` (Tasks) or `focus:outline-none` (Hooks) | none | n/a | **C** |

**Key gap:** Clickable `div`/`li` elements with `role="button"` (Chat session
rows `Chat.tsx:328`, Sidebar sessions `Sidebar.tsx:122`) have keyboard
support (`tabIndex={0}`, `onKeyDown`) but **no visible focus ring**. The
global `button:focus-visible` rule in `index.css:849-857` only applies to
native `<button>` / `<a>` / `<input>`. These custom role=button divs are
invisible to keyboard users.

### Animation consistency

- **`transition-colors`** vs **`transition-all`** vs **`transition-transform`**:
  all three are used. `transition-all` is the most common (Cards, buttons) —
  it is the most expensive (animates every property including layout).
  Should prefer `transition-colors` for hover states and `transition-transform`
  for translate/scale.
- **`duration-200`** / **`duration-300`** / **`duration-500`** / **`duration-700`**:
  four durations with no token. Progress bars use `duration-500`/`duration-700`;
  hover states use `duration-200`/`duration-300`.
- **`active:scale-*`**: `active:scale-95` (Chat, Extensions), `active:scale-[0.98]`
  (AdvancedSettings), `active:scale-[0.99]` (AdvancedSettings clear),
  `active:translate-y-px` (shared Button). Four press-feedback recipes.
- **`animate-pulse`**: used for status dots (Header, footer, Chat streaming).
  Consistent.
- **`animate-spin`**: used for `progress_activity` loading glyph. Consistent.
- **`animate-in fade-in duration-700`**: used once (`pages/Settings.tsx:9`).
  No other page uses an entrance animation.

**Verdict:** Animations need a duration token scale
(`--duration-fast: 150ms`, `--duration-normal: 200ms`, `--duration-slow: 300ms`)
and a press-feedback token. The global focus-visible rule needs to extend to
`[role="button"]` and `[role="option"]`.

---

## Top 15 fixes ranked by impact

| Rank | Issue | Pages affected | Fix complexity |
|------|-------|---------------|----------------|
| 1 | **Skills subsystem uses raw `text-gray-*`/`bg-blue-600` (26 violations)** — breaks on every non-default theme | `SkillProposalsToast.tsx`, `SkillProposalReviewPanel.tsx` | Low (mechanical token swap) |
| 2 | **No shared `Modal`/`Dialog` primitive** — 25+ hand-rolled modals with 3 backdrop opacities, 6 z-index values, 4 max-widths | Chat, Header, Triage, Settings (x7), OPC, Memory, Extensions, Diff | Medium (build primitive, migrate ~25 call sites) |
| 3 | **Primary buttons bypass the `<Button>` primitive on ~15 pages** — 7+ different primary recipes | Chat, Profiles, Welcome, OPCTask, Extensions, Tasks, Settings, Hooks, Routines | Medium (migrate each to `<Button>`, remove className overrides) |
| 4 | **No focus ring on OPCTask revision textarea and Goals search** — WCAG 1.4.11 violation | OPCTask, Goals | Low (swap to shared `Input`/`Textarea`) |
| 5 | **Four incompatible input focus-ring recipes** — ring-1/ring-2/ring-3, primary vs ring, /20 vs /30 vs full opacity | Chat, Welcome, Routines, Profiles, Hooks, MyAgents, QuickFix, Editor | Medium (route all through shared `Input`) |
| 6 | **`font-headline-sm` token is referenced but undefined** — silent fallback to base font | Chat (`:413`), Header (`:155`) | Trivial (add `--text-headline-sm: 20px` to `index.css`) |
| 7 | **Icon sizing is raw `text-[Npx]` (140+ occurrences)** — no semantic scale, adjacent buttons render at different sizes | All pages, especially Header, Chat, Sidebar | Medium (add `icon-sm/md/lg` utilities or a wrapper component) |
| 8 | **No shared `Card` primitive** — 4 card recipes (standard, glass, accent, gradient) with 4 hover elevations | OPCTask, Settings, Extensions, Conversations, Memory, Goals | Medium (extract `<Card>` with `variant` prop) |
| 9 | **Custom `role="button"` divs have no visible focus ring** — keyboard users cannot see where they are | Chat session list, Sidebar sessions, Kanban cards | Low (extend global focus-visible rule to `[role="button"]`) |
| 10 | **Loading state is a centered spinner everywhere** — causes layout shift in card grids; SkeletonLoader exists but is unused | Chat, Extensions (5 tabs), Notifications, Routines, Hooks | Medium (default to skeleton in list/grid contexts) |
| 11 | **Sidebar has no badges for actionable items** — HIL approvals, failed routines, unread triage are invisible until user visits the page | Sidebar (global nav) | Medium (add badge/dot to nav items, wire to existing stats) |
| 12 | **Empty states are hand-rolled on 5 pages** — inconsistent icon size, hierarchy, CTA presence | Chat, Profiles, OPCTask, Memory, Hooks | Low (migrate to `EmptyState` primitive) |
| 13 | **Off-scale font sizes (`text-[10px]`, `text-[11px]`, `text-[13px]`)** — 20+ occurrences, sub-token sizes that fall below the 12px floor | Welcome, Hooks, Triage, Profiles, Tasks | Low (define `--text-label-xs: 11px` or enforce 12px floor) |
| 14 | **Elevation has no token scale** — `shadow-sm` through `shadow-2xl` used as raw utilities with no documented mapping | All card/modal/dropdown surfaces | Medium (define `shadow-e1`-`e5` tokens or document mapping) |
| 15 | **Two parallel session-list implementations** (Chat sidebar vs. Sidebar sessions) with different padding, borders, and active-state recipes | `Chat.tsx:300-399`, `Sidebar.tsx:94-153` | Medium (merge into one shared component) |

---

## Proposed design system additions

### Missing primitives

1. **`<Modal>`** (or `<Dialog>`) — backdrop (`bg-black/40`), z-index (`z-[100]`),
   max-width (`sm`/`md`/`lg`/`xl`/`full`), escape handling, focus trap, scroll
   lock. Replaces ~25 hand-rolled modals. Props: `open`, `onClose`, `size`,
   `children`.

2. **`<Card>`** — `variant="elevated" | "outlined" | "glass"`, `interactive`
   (adds hover elevation). Replaces 4 card recipes. Enforces
   `bg-surface-container-lowest rounded-2xl border-outline-variant/30`.

3. **`<ConfirmDialog>`** — specialized `<Modal>` for destructive confirms.
   Props: `title`, `description`, `confirmLabel`, `destructive`, `onConfirm`.
   Replaces ~10 inline confirm modals (Settings x4, Billing x3, Chat x1,
   Triage x2).

4. **`<DropdownMenu>`** — accessible menu with `role="menu"`, keyboard nav,
   consistent shadow/border. Replaces Header model selector and MyAgents
   action menu.

5. **`<Textarea>`** — sibling to `<Input>` with the same focus ring, same
   height scale, `resize` prop. Currently every textarea is hand-rolled.

6. **`<Badge>`** — for status pills (`bg-primary/10 text-primary font-bold
  uppercase tracking-wider`). Currently every status pill is a 5-class string
  duplicated 30+ times.

7. **`<Tooltip>`** — does not exist. Many icon buttons use `title=` (native
   browser tooltip) which is inconsistent and inaccessible. A tooltip
   primitive with proper `aria-describedby` and delay would improve the
   icon-heavy Header and Chat toolbars.

8. **`<Banner>`** — for the "API key missing" / "Demo mode" / "Connection
   lost" dismissible banners. Currently the Chat API-key banner
   (`Chat.tsx:448-466`) and the Tasks error banner (`Tasks.tsx:206-216`) are
   the only two; both are hand-rolled with different tokens.

### Missing tokens

9. **`--text-headline-sm: 20px`** — referenced by `font-headline-sm` class,
   undefined in `@theme`. Latent bug.

10. **`--text-label-xs: 11px`** (or enforce 12px floor) — `text-[10px]` and
    `text-[11px]` appear 20+ times. Either bless 11px as a token or ban it.

11. **Elevation scale** — `--shadow-e1` through `--shadow-e5` mapped to
    `shadow-sm` / `shadow-md` / `shadow-lg` / `shadow-xl` / `shadow-2xl`.
    Document: e1 = card resting, e2 = card hover, e3 = dropdown, e4 =
    drawer, e5 = modal.

12. **Duration scale** — `--duration-fast: 150ms`, `--duration-normal: 200ms`,
    `--duration-slow: 300ms`, `--duration-slower: 500ms`.

13. **Icon-size utilities** — `icon-xs` (12px), `icon-sm` (16px), `icon-md`
    (20px), `icon-lg` (24px), `icon-xl` (32px), `icon-2xl` (48px). Applied as
    a wrapper or as `text-icon-md` on the Material Symbols span.

### Global CSS fixes

14. **Extend focus-visible to custom roles:**

    ```css
    [role="button"]:focus-visible,
    [role="option"]:focus-visible,
    [role="tab"]:focus-visible,
    [role="listitem"]:focus-visible {
      outline: 2px solid var(--color-primary);
      outline-offset: 2px;
      border-radius: var(--radius-md);
    }
    ```

    This closes the keyboard-navigation gap on Chat session rows, Sidebar
    sessions, and Kanban cards.

15. **Document the card-radius decision.** Either:
    - `rounded-xl` for cards (align with the `--radius-xl` token), or
    - `rounded-2xl` for cards (the de-facto standard with 80+ usages).

    Pick one and add a comment in `index.css`. The current split
    (`rounded-xl` in Settings/Theme, `rounded-2xl` everywhere else) is the
    most common source of "why does this card look different" questions.

---

## Appendix: brand-gradient exception (judgment call)

`components/extensions/Featured.tsx:242-254` and
`components/extensions/DataSources.tsx:402-417` define per-brand gradient
palettes:

```
gitlab: { icon: "from-orange-600 to-amber-500", ... }
linear: { icon: "from-indigo-600 to-violet-500", ... }
slack:   { icon: "from-purple-600 to-rose-500", ... }
figma:   { icon: "from-pink-600 to-orange-500", ... }
```

These use raw Tailwind palette colors (`from-orange-600`, `to-violet-500`,
etc.) which bypass the MD3 token system. **This is intentional and
correct** — brand identity colors cannot be mapped to semantic tokens
without losing recognizability (Slack is not "the secondary color", it is
purple-to-rose). The audit does NOT flag these as violations.

**Recommendation:** Add a header comment in both files documenting the
exception so future contributors do not "fix" the gradients by mapping them
to `from-primary to-secondary` (which would make every brand look identical).
