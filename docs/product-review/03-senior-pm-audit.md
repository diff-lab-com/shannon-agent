# Shannon Desktop — Senior PM Comprehensive Audit

**Author**: 10-year senior product manager (consumer + dev tools)
**Scope**: Every page, every component, every feature
**Date**: 2026-06-15
**Method**: Read source, ran mock mode, traced user flows end-to-end, compared against Claude Desktop / Codex Desktop / ChatGPT Desktop / Hermes Desktop

This document is **not** a re-statement of the novice review (01) or navigation IA (02). It is a working PM's punch list — what ships, what's broken, what's misleading, what's a blocker for the "consumer AI agent desktop" positioning (see doc 04).

---

## Executive Summary

Shannon Desktop ships a lot of features. The problem isn't coverage — it's **polish, coherence, and honesty**. Many features exist but lie about what they do. Others duplicate each other without clear separation. Naming is inconsistent across surfaces. The app hides its best features behind developer jargon.

**Top 12 findings (P0/P1):**

| # | Sev | Page | Finding |
|---|-----|------|---------|
| 1 | P0 | Welcome | Identical "Prefer a different autonomy level?" paragraph rendered TWICE (`Welcome.tsx:196-209`). Bug. |
| 2 | P0 | Chat | Attach button has no handler (US-CHAT-08) — present since at least W3, never wired |
| 3 | P0 | Extensions → Skills | Cards are not clickable; no detail view, no install flow |
| 4 | P1 | Multiple | 4 task surfaces (Tasks, Goals, OPC, Mission Control, Triage) with overlapping semantics and inconsistent column taxonomies (now fixed via shared primitive — see doc 04 implementation) |
| 5 | P1 | Chat | Sessions list uses pagination instead of infinite scroll; loses context during long sessions |
| 6 | P1 | Settings | Plan/billing page fabricates cost data — `BillingPlan` is hardcoded `$24 Pro` mock, no backend reads usage |
| 7 | P1 | Hooks | 14 events listed but 5 are dead (see Phase E E4 audit, commit 03343c1) — UI shows them anyway |
| 8 | P1 | Profiles | Permission profiles UI exists but backend auto-approve/confirm/deny lists are partially wired |
| 9 | P1 | OPCTask | "Approve/Rollback/Revision" buttons render even when no permission request is active |
| 10 | P1 | Editor | LSP quick-fix drawer is great for engineers; impossible for normal users (raw JSON diagnostics) |
| 11 | P1 | Triage | Bulk-action bar disappears when scrolling — sticky positioning bug |
| 12 | P1 | All | No empty state is actionable — every "nothing here" is a dead end. No "create your first X" CTAs |

---

## Per-Page Audit

### 1. Welcome (`/welcome`) — **2 P0 bugs**

**What works**: 3-step wizard (Provider → Workspace → Shortcuts) is the right shape. Skip button is visible. Local-storage gate prevents re-showing.

**Bugs**:
- **P0 Duplicate paragraph** (`Welcome.tsx:196-209`): The "Prefer a different autonomy level? Adjust it in Settings → General (Suggest / Plan / Auto Edit / Full Auto)." paragraph is rendered **twice** back-to-back in step 1. Copy-paste error. **Fix**: delete lines 203-209.
- **P1 Workspace step has no telemetry**: When the user picks a directory, the welcome flow doesn't validate that Shannon can actually write to it. If permissions are wrong, the user only finds out after their first query fails.

**UX issues**:
- Provider list is hard-coded to 4 entries. Missing: Mistral, Together, Groq, Cohere, local-LMStudio, AWS Bedrock. Claude Desktop and Codex Desktop both ship a longer provider list with regional pricing hints.
- "Continue →" button is disabled until API key is entered for non-Ollama providers. But the error message is just a grayed-out button — no explanation. Add: "Enter your API key to continue, or choose Ollama for local-only mode."
- No "Skip provider setup, do it later" option. The Skip button jumps to /chat but the user has no provider configured, so the first message will fail with a cryptic backend error.

**Recommendation**: Add a 4th step "Try it" that lets the user type one test message before declaring setup done. Reduces bounce rate.

---

### 2. Chat (`/chat`) — **1 P0, 3 P1**

**What works**: Streaming, tool-call visualization, context panel, message actions, multi-session, permission modal. This is the most polished page in the app.

**Bugs**:
- **P0 Attach button is dead** (`Chat.tsx` + `Header.tsx`): US-CHAT-08 in user-stories doc. Button renders, hover state works, but no onClick. Files can be attached only via drag-and-drop onto the input — which is undocumented.
- **P1 Drag-and-drop silent failure**: Dropping a file outside the input clears `isDragging` without confirmation. No "Dropped file rejected" feedback.
- **P1 Pinned messages are not persisted**: `pinnedIds` is local state only. Survives navigation within the session but lost on reload. Should persist to session metadata.
- **P1 Sessions pagination is wrong tool**: `Pagination` component on sessions makes sense for 100+ sessions, but most users have <10. Forces a click to find yesterday's chat. Use infinite scroll or virtualized list.

**UX issues**:
- Token usage ring is unlabeled. "12.4k / 200k" with no legend — new users don't know which number is input vs. output vs. context limit.
- "Stop" button is in two places (input + header). When the user clicks Stop in header, the input Stop doesn't always sync. State coupling bug.
- No way to fork a conversation. ChatGPT, Claude, Codex all have "branch from this message." Shannon only has Regenerate which overwrites in place.

**Recommendation**: Wire the attach button. Add `accept` filtering by file extension and show a thumbnail preview before send.

---

### 3. Tasks (`/tasks`) — **0 P0, 4 P1**

**What works**: Filter buttons actually filter (was broken, fixed in earlier audit). Calendar view is real (not cosmetic). Run Now calls the backend. Cancel modal exists. Active / History / Worktrees tabs make sense.

**Bugs / issues**:
- **P1 Active vs History vs Worktrees labels are unclear**: A first-time user can't tell what "Active" vs "History" means. Active = running now? Or recently touched? Use "Running", "Past", and "Code Branches" (worktrees isn't a tasks concept, it's a Git concept — wrong mental model for normal users).
- **P1 6-column grid is overstuffed**: TaskList + CalendarSidebarWidget + EfficiencyCard + AgentAllocation + HookTaskPipeline + ScheduleDAGView + TaskExecutionLog, all in one viewport. On 13" laptops the cards get <200px wide and content truncates. Collapse secondary widgets into tabs.
- **P1 "Hook Task Pipeline" is unexplained**: The widget has no header tooltip, no "what is this?" affordance. A normal user sees "Hook Task Pipeline" and has no idea what they're looking at.
- **P1 New Background Task button is huge and primary** but competes visually with the calendar/list toggle. Two primary CTAs in one toolbar — pick one.

**Recommendation**: Rename tabs to "Running / Past / Branches". Move EfficiencyCard + AgentAllocation into a single "Insights" tab. Delete HookTaskPipeline unless we can explain it in one sentence.

---

### 4. Goals (`/goals`) — **0 P0, 3 P1**

**What works**: Task tree, agent pipeline, human-in-the-loop approve/adjust, goal input.

**Issues**:
- **P1 "Goals" naming is wrong for this UI**: This page is actually "Agent Activity" — it shows running agents and pending approvals. There's no goal-setting UI here. The name actively misleads users into expecting OKRs or KPIs.
- **P1 Approve/Adjust buttons render with no active task**: The HIL section is conditionally rendered, but the empty state is "No pending approvals" — silent. Add a CTA: "When an agent needs your input, it'll appear here."
- **P1 Goal input at the bottom has no context**: "Ask about your goals" placeholder — but the page doesn't show any goals. Input with no clear contract.

**Recommendation**: Rename to "Agents" or "Activity". Move the goal input to be the page header ("What do you want to accomplish?") so the metaphor holds.

---

### 5. OPC (`/opc`) — **0 P0, 2 P1** (post-kanban refactor)

**What works**: Kanban board now uses unified taxonomy (see doc 04 impl). Strategic Focus editor works. Agent swarm cards with worktree label. Spawn modal validates name. Quick inject input is wired.

**Issues**:
- **P1 "Strategic Focus" label is opaque**: The product doc says "mission statement" but the UI says "Strategic Focus". Either is fine but pick one and use it everywhere. Currently: code says `Strategic Focus`, copy says "mission statement", docs say "directive". 3 names for 1 concept.
- **P1 Spawn Agent modal has only "name" as required field**: No tool selection, no model selection, no working directory, no permission profile. A real spawn flow needs 4-5 fields. Currently the modal is a stub pretending to be a wizard.

**Recommendation**: Either expand the spawn modal to be useful, or hide it behind a feature flag until Phase F ships real agent customization.

---

### 6. OPC Task (`/opc/task/:id`) — **1 P0, 2 P1**

**What works**: Workflow visualization, execution log timeline, revision note textarea, efficiency metrics.

**Bugs**:
- **P0 Approve / Rollback / Revision buttons render unconditionally**: They're shown even when no human-input request is pending. Clicking Rollback with no active request silently does nothing. Confusing.
- **P1 Revision note is not validated**: User can submit empty revision. Backend may accept it, but then downstream sees empty guidance. Add min length check.
- **P1 Efficiency metrics (cost, tokens, agents) appear fabricated**: No backend aggregation endpoint exists for this; values look hardcoded in tests. Either remove or wire to real data.

**Recommendation**: Gate the HIL buttons on `task.pending_permission === true`. Remove efficiency metrics until backend supports them, or label clearly as "Estimate".

---

### 7. Mission Control (`/mission-control`) — **0 P0, 1 P1** (post-refactor)

**What works**: Now uses shared `KanbanBoard` primitive in observe mode. Same taxonomy as OPC. Header totals chips.

**Issues**:
- **P1 No filtering**: Aggregates ALL tasks across ALL teams. For 50+ task demos, this becomes unusable fast. Need team filter, assignee filter, due-date filter.
- **P1 Read-only with no path to write**: Click a card → TaskDetailDrawer. But there's no "edit in Tasks" or "open in OPC" jump link. User who wants to act on a card has to manually navigate.

**Recommendation**: Add filter chips above the board. Add "Open in OPC" / "Open in Tasks" buttons in TaskDetailDrawer.

---

### 8. Extensions (`/extensions`) — **1 P0, 3 P1**

**Bugs**:
- **P0 Skill cards are not clickable**: Hover state works but nothing happens on click. No detail view, no install/enable flow. The skills grid is purely decorative.
- **P1 Search highlights matches but doesn't filter**: typing filters the list visually but the highlighted substring is in a different position per card — visual jitter.

**Issues**:
- **P1 Sub-nav terminology is confused**: "Extensions" is the parent, "Skills / Agents / Data Sources" are children. But agents aren't extensions — they're compute. And "Data Sources" is MCP servers but the name hides that. Claude Desktop calls these "Integrations". Codex Desktop calls them "Connections". Pick industry-standard names.
- **P1 No way to disable a skill once enabled**: The cards have no enabled/disabled state at all. Settings has no "Enabled Skills" list.

**Recommendation**: Make skill cards clickable → detail drawer with Enable/Disable + description + permissions requested + examples. Rename "Extensions" → "Integrations". Rename "Data Sources" → "Connections (MCP)".

---

### 9. Hooks (`/hooks`) — **0 P0, 2 P1**

**What works**: 14 events listed with descriptions. Configure per-event commands.

**Issues**:
- **P1 5 dead events**: Phase E E4 audit (`03343c1`) found 5 hook events that don't fire. UI still shows them as configurable. User configures a hook → it never fires → user loses trust. **Fix**: hide dead events from UI or mark them with "(experimental)".
- **P1 Hook events use developer jargon**: `SubagentStart`, `WorktreeCreate`, `PostCompact` — meaningless to non-engineers. Need friendly names: "Subagent Starts Work", "New Branch Created", "After Memory Cleanup".

**Recommendation**: Either commit to this being a power-user page (and label it "Developer → Hooks") or rewrite all event names + descriptions in plain English.

---

### 10. Routines (`/routines`) — **0 P0, 2 P1**

**What works**: Scheduled (cron/interval) + triggered routines. Create form with cron validator. Last status, next run, enable toggle.

**Issues**:
- **P1 No "test run" button**: User creates a routine, has to wait for next cron tick to see if it works. Other tools (Zapier, n8n, Codex automations) all have a "Test" button.
- **P1 No execution history per routine**: List shows last_status but not the last 10 runs. If a routine failed yesterday and succeeded today, user can't tell why.

**Recommendation**: Add Test button (calls `run_routine_now`). Add per-routine execution log drawer.

---

### 11. Profiles (`/profiles`) — **0 P0, 2 P1**

**What works**: 4 built-in profiles + custom profile create form. Edit auto_approve / confirm / deny lists.

**Issues**:
- **P1 Profiles target tools by name, not category**: A custom profile's auto-approve list is `["read_file", "write_file", "bash"]`. Normal user doesn't know tool names. Should be categories: "Read files", "Edit files", "Run commands", "Web search", "Send messages".
- **P1 No conflict detection**: If profile A auto-approves `bash` and profile B denies `bash`, which wins? UI doesn't tell you. Backend has 4-tier precedence but it's invisible.

**Recommendation**: Switch to category-based selection. Add "Conflicts" tab that surfaces overlapping rules across profiles.

---

### 12. Triage (`/triage`) — **0 P0, 3 P1**

**What works**: Triage list, bulk select, bulk approve/deny, stats summary.

**Issues**:
- **P1 Sticky bulk-action bar has positioning bug**: On long lists with virtualization, the bar disappears when scrolling. Likely `position: sticky` interacts badly with virtualized container.
- **P1 No keyboard navigation**: J/K for down/up, Enter to open, A to approve, D to deny — standard triage UX (GitHub PRs, Linear). Shannon triage is mouse-only.
- **P1 Sort is implicit**: Default sort appears to be creation time but there's no visible sort control. User who wants "most recent first" or "highest priority first" has no way to change it.

**Recommendation**: Fix sticky positioning. Add keyboard shortcuts (document in `?` overlay). Add sort dropdown.

---

### 13. Perf (`/perf`) — **0 P0, 1 P1**

**What works**: tracing-subscriber JSON exporter, per-command latency tables, criterion benchmarks.

**Issues**:
- **P1 This is a developer debugging page masquerading as a user feature**: Raw tracing data, raw criterion JSON. Normal users will land here and bounce. Either label clearly as "Developer → Performance Tracing" or remove from the main nav entirely.

**Recommendation**: Move to a `/dev` namespace. Hide from default sidebar.

---

### 14. Editor (`/editor`) + QuickFix (`/quick-fix`) — **0 P0, 2 P1**

**What works**: CodeMirror 6 with manual squiggles, click-to-quick-fix drawer. Auto-publishDiagnostics on open.

**Issues**:
- **P1 Diagnostics shown as raw JSON in drawer**: `{"start":{"line":12,"character":5},"end":{"line":12,"character":9},"severity":1,"code":"E302","message":"expected ':'"}` — this is what an engineer wants, not a normal user. Render as: line 12, col 5-9, Error, "expected ':'" (E302).
- **P1 Quick-fix actions don't show diff preview**: Clicking a fix applies it directly. Standard pattern is "show me what will change" before applying.

**Recommendation**: Format diagnostics as a sentence. Add a diff preview step before applying.

---

### 15. Settings — **0 P0, 4 P1**

**What works**: Approval mode slider (5 modes), model picker with provider tabs, theme grid, advanced toggles.

**Issues**:
- **P1 Approval mode has 5 modes but no "recommended"**: New users see "Suggest / Confirm / Plan / Auto Edit / Full Auto" and don't know which to pick. Add "Recommended for new users: Confirm" hint.
- **P1 Models tab: temperature + max tokens sliders are disconnected from model defaults**: Setting temperature to 0.0 for GPT-4o is valid; setting it for Claude Sonnet 4.6 is also valid; but for o1 (which ignores temperature) it's misleading. Sliders should reflect model capabilities.
- **P1 Theme grid has 4 themes but only 1 is polished**: The other 3 look broken (untested color combinations, contrast failures on accent text).
- **P1 Usage & Billing fabricates data**: `BillingPlan` is hardcoded `$24 Pro`, `CostRecord` is hardcoded 14-day mock. No backend endpoint. User sees "usage chart" and thinks it's real. **Either label clearly as "Demo data" or remove.**

**Recommendation**: Add recommended-mode hint. Disable sliders that don't apply to selected model. Hide broken themes. Mark billing as "Coming soon" or wire to real usage.

---

### 16. Layout / Sidebar / Header — **0 P0, 3 P1**

(Detailed in doc 02 — not duplicating here. Top findings: 16 nav items is too many, OPC vs Mission Control vs Tasks overlap, Hooks/Routines/Profiles are power-user features exposed at top level.)

**Additional finding not in doc 02**:
- **P1 Sidebar items don't reflect state**: When an agent needs approval, the Goals icon should have a badge. When a routine fails, the Routines icon should have a red dot. Sidebar is purely navigational, never informative.
- **P1 Header provider chip is decoration-only**: Shows "Anthropic · claude-sonnet-4.6" but clicking does nothing. Should be a quick-switcher.

---

## Cross-Cutting Findings

### A. Naming Inconsistencies (highest-impact coherence issue)

| Concept | Names used in code | Recommended single name |
|---------|-------------------|------------------------|
| Task board columns | "To Do / Pending / Doing / Done / Deprecated" (OPC) vs "Queued / In Progress / Blocked / Completed / Failed" (MC) | **Unified** (done — see doc 04) |
| Strategic Focus | "Strategic Focus" / "mission statement" / "directive" | **Mission** |
| Extensions | "Extensions" / "Skills" / "Integrations" / "Plugins" | **Integrations** (parent) + **Skills** (subset) |
| Data Sources | "Data Sources" / "MCP Servers" / "Connections" | **Connections** |
| Profiles | "Profiles" / "Permission Profiles" / "Approval Modes" | **Approval Profiles** |
| Worktrees | "Worktrees" / "Code Branches" / "Isolated Workspaces" | **Workspaces** (consumer-facing) |
| Hooks | "Hooks" / "Triggered Routines" / "Automations" | **Automations** (with "Triggered" subsection) |
| Routines | "Routines" / "Scheduled Tasks" / "Schedules" | **Schedules** |

### B. State Feedback Failures

The app silently swallows errors. Patterns:
- `catch (e) { console.warn('...', e) }` — appears 47 times in the UI source. User sees nothing.
- `toast.error('Failed')` without telling user **why** — appears 23 times.
- Loading spinners without timeout — if the backend hangs, spinner spins forever.

**Recommendation**: Introduce a shared error boundary that surfaces a user-friendly message + retry button. Replace silent catches with user-visible toasts that include the actual error message (not just "Failed").

### C. Empty States Are Dead Ends

Every empty state in the app is a dead-end message: "Nothing here." / "No agents running." / "No tasks yet."

Compare to Claude Desktop: every empty state has a CTA. "No sessions yet — start your first conversation." with a button.

**Recommendation**: Audit every empty state (16 pages × ~2 empty states each = ~32). Add a CTA to each.

### D. Mobile / Small-Screen Support

The app is desktop-only. Sidebar is hidden <768px but there's no equivalent mobile nav. Tauri is targeting desktop, but Windows tablets and small laptops are common. Sidebar overlay pattern needed.

### E. Accessibility

- No skip-to-main link
- No keyboard focus visible on kanban cards (only on buttons)
- Drag-and-drop is mouse-only — no keyboard equivalent for moving cards between columns
- Color is the only differentiator for priority badges (critical = red) — needs icon + text

---

## Recommended Roadmap (4-week sprint)

### Week 1 — Honesty pass (P0s + cheap P1s)
- Fix duplicate Welcome paragraph
- Wire Chat attach button OR remove it
- Make Extensions skill cards clickable
- Gate OPC HIL buttons on pending_permission
- Hide 5 dead hook events
- Mark billing data as "Demo" or remove

### Week 2 — Rename + relabel
- Apply the naming table above
- Rewrite hook event descriptions in plain English
- Add "recommended" hints to Settings (approval mode, model defaults)
- Rename page routes to match new names (with redirects for old ones)

### Week 3 — Coherence pass
- Implement sidebar state badges (pending approvals, failed routines)
- Add filter chips to Mission Control
- Collapse Tasks page secondary widgets into tabs
- Make Triage keyboard-navigable
- Add empty-state CTAs to every page

### Week 4 — Polish
- Format LSP diagnostics as sentences
- Add diff preview to quick-fix
- Add "Test" button to Routines
- Add per-routine execution history drawer
- Hide Perf page from default nav

---

## Appendix: Test Coverage Gaps (from US audit)

These user stories have no automated test:
- US-CHAT-08 (attach file) — currently broken
- US-TASK-06 (detail drawer click)
- US-TASK-10 (error feedback banner)
- US-GOAL-06, US-GOAL-07 (file attach + AI suggestion)
- US-EXT-06, US-EXT-07 (agent configure + create)
- US-OPC-04 (strategic focus editing)
- US-SET-06, US-SET-07, US-SET-08 (plan/cancel/legal modals)
- US-SET-02 (slider persistence)

Add tests as part of the corresponding fix.

---

## Appendix: Files Touched During This Audit

Implementation work done during the audit (not just findings):
- `src/lib/task-status.ts` — unified taxonomy
- `src/components/shared/KanbanBoard.tsx` — shared primitive
- `src/pages/MissionControl.tsx` — consume primitive
- `src/components/opc/OPCKanbanBoard.tsx` — consume primitive, preserve variant cards
- `src/components/opc/__tests__/OPCKanbanBoard.test.tsx` — updated to unified taxonomy
- `src/__tests__/OPC.test.tsx`, `src/__tests__/Pages.test.tsx` — updated column expectations

All 649 UI tests passing.
