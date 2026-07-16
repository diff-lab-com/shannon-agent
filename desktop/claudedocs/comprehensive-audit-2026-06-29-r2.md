# Shannon Desktop — Comprehensive Audit R2 (PM + Architect)

**Date:** 2026-06-29 · **Scope:** all pages, features, components, i18n · **Lens:** senior PM (10yr) + architect
**Basis:** `main`/`dev` state after PRs #94–#97. Follows R1 (`comprehensive-audit-2026-06-29.md`, shipped as PR #97).

## TL;DR

The shell is in **good shape**. After R1 + #96 (tasks i18n) + #97 (P1/P2/P3): **zero** silent
failures, **zero** `console.log`/`TODO`/`window.confirm`, **zero** i18n key-parity gaps. The one
substantive issue found this round was a **residual i18n translation gap** in the Tasks module's
deep sub-components (drawers, forms, panels, DAG views) that #96's board-level pass didn't reach —
**237 keys translated in this round, gap now closed**. Remaining items are polish/consistency, not
bugs; they are presented as a prioritized plan for review.

---

## 1. i18n — completeness (FIXED this round)

**Parity:** `en=2283 / zh=2283`, CI parity gate passes. **No missing keys.**

**Translation gap found & fixed:** #96 translated the Tasks *board*; the deeper Tasks
sub-components were still English (`zh === en`). Grouped counts of the gap:

| Sub-component | Keys | | Sub-component | Keys |
|---|---|---|---|---|
| scheduleForm | 38 | | resultRoutingEditor | 11 |
| agentMessagesPanel | 22 | | scheduleDAGView | 10 |
| taskDetailDrawer | 20 | | hookTaskPipeline | 9 |
| hookRoutineCreateDialog | 19 | | taskDAGView | 8 |
| routineDetailDrawer | 15 | | + 11 smaller groups | 57 |
| tasksHeader | 11 | | **total** | **~220** |

**Action taken:** translated **237** keys (200 plain-prose + 37 including ICU placeholder/prose and
plural forms). ICU placeholders (`{name}`, `{count, plural, …}`) verified preserved post-write.
Covered: Tasks module fully, plus the Extensions Hub prose keys (Install/Cancel/Submit/Skills/
Agents/search/noInstalled).

**Remaining 50 `zh === en` — all legitimate** (verified): brand names (Shannon, OpenAI, Anthropic,
Slack, GitHub, Playwright), code/path/regex placeholders (`sk-…`, `0 9 * * *`, `lint-after-edit`,
`/abs/path/to/src/lib.rs`, `regex, e.g. \.rs$`), perf range labels (`<10ms`, `10ms–100ms`), locale
self-names (English / 中文（简体）), and OAuth/webhook field labels (Bot Token, Webhook URL — kept as
convention; see D5).

---

## 2. Code-health scan (clean)

| Check | Result |
|---|---|
| Empty `catch {}` (silent failures) | **0** |
| `console.log`/`console.debug` | **0** |
| `TODO`/`FIXME`/`HACK`/`XXX` | **0** |
| Native `window.confirm/alert/prompt` | **0** (the one grep hit is a comment in ModelsSettings) |
| `any` usage | 8 (low; see F3) |

This is notably clean — no manufactured work here.

---

## 3. Findings (for the plan)

### F2 — 7 hand-rolled modal overlays instead of the shared `Modal` primitive *(architecture/consistency)*
`BillingSettings` (3) and `AdvancedSettings` (4) each render a bespoke
`<div className="fixed inset-0 bg-black/30 …" role="dialog" aria-modal onClick onKeyDown={Escape}>`
with copy-pasted Escape + backdrop-close logic. The shared `Modal` primitive (Week B, PR #53) and
`ConfirmDialog` already do this **plus** focus-trap and consistent aria. 7 duplicated overlay
implementations = drift risk + missing focus management.

### F3 — `any` in the `t()` i18n helper *(type-safety)*
5 copies of `const t = (id: string, values?: any) => intl.formatMessage({ id }, values)` (Header,
Triage ×2, Editor, OPCTask). Plus `Chat.tsx:716 (usage as any).max_tokens` and 2 mock-only `any`.
The helper should type `values?: Record<string, unknown> | readonly unknown[]`; `usage` needs a
proper type field.

### F4 — Large component files *(maintainability)*
7 files >600 lines: `Chat.tsx` (775), `Welcome.tsx` (742), `McpAddServerDialog.tsx` (762),
`ModelsSettings.tsx` (683), `NotificationsSettings.tsx` (674), `MemoryPanel.tsx` (611),
`Editor.tsx` (573), `InstallDialog.tsx` (547). Not bugs; ROI on splitting is low unless a file is
actively being changed.

### F6 — Voice feature: wired in but not validated against product intent *(product)*
`useVoice` + `MicButton` + `VoiceOrb` are imported and rendered in `ChatInput.tsx` (not a stub —
has `transcribing` state). But product positioning ([[project_positioning_general_user]]) lists
Voice as "规划中" (planning). Needs a decision: is voice shipping, experimental, or to be gated?

### F5 — `onClick` on `<div>` backdrops *(a11y)*
15 hits; almost all are modal/drawer backdrops (legit click-to-close). Migrating F2 to the shared
`Modal` resolves the modal ones. The remaining (Layout sidebar scrim, KeyboardShortcutsHelp) are
acceptable.

### F7 — Optional: translate remaining OAuth/webhook terms *(i18n polish)*
"Bot Token", "Webhook URL", "Bot Token (xoxb-)" could become "机器人令牌"/"Webhook 地址". Borderline
— many products keep OAuth terminology in English.

---

## 4. Prioritized plan + decision points (for your review)

| ID | Item | Effort | Recommend |
|---|---|---|---|
| **D1** | F2: migrate the 7 hand-rolled overlays → shared `Modal`/`ConfirmDialog` (Billing 3 + Advanced 4) | M | **Yes** — consistency + free focus-trap/aria/Esc, removes 7× duplication |
| **D2** | F3: type the `t()` helper (`values?: Record<string,unknown> \| readonly unknown[]`); type `usage.max_tokens` | S | **Yes** — low-risk type tightening |
| **D3** | F4: break up the 600+ line files | L | **Defer** — works fine; only split opportunistically when editing |
| **D4** | F6: decide Voice status (ship / gate behind flag / verify-then-decide) | S–M | **Verify-then-decide**: smoke-test voice in chat; if solid, ship; if rough, gate behind a flag until polished |
| **D5** | F7: translate "Bot Token"/"Webhook URL" etc. | S | **No** — keep OAuth/webhook terms in English (convention); defer |

**Already shipped this round:** the i18n Tasks translation (237 keys) — PR pending.

## 5. What I did NOT find
No dead routes, no orphan components (conversations/ + voice/ are both consumed by `ChatInput`),
no broken legacy redirects (all map to live routes), no accessibility landmines beyond the modal
focus-management covered by D1, no empty-state gaps (R1 G3 closed those).

---
*Refs: [[project_pm_audit_p1p2p3_pr97]] · [[project_tasks_i18n_pr96]] · R1 = `comprehensive-audit-2026-06-29.md`*
