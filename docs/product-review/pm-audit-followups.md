# PM Audit Follow-ups — Unresolved After PR #50

**Source audit:** `docs/product-review/03-senior-pm-audit.md` (2026-06-15)
**Cross-referenced against:** PR #50 commits (`git log dev..s2/ui-design-overhaul`)
**Date:** 2026-06-26

PR #50 (UI design overhaul) closed the P0 honesty pass and most cross-cutting
polish/a11y items. This doc lists audit findings **still unresolved** after
that PR. Grouped by audit severity, then by page.

## Still open — P0 (critical honesty bugs)

| # | Page | Finding | Status |
|---|------|---------|--------|
| 1 | Chat | **Attach button has no handler** (US-CHAT-08). Files can only be attached via undocumented drag-and-drop. | Not in PR #50 |
| 2 | Extensions → Skills | **Skill cards not clickable.** No detail drawer, no install flow. Hover state is purely decorative. | Not in PR #50 |

## Still open — P1 (per-page)

### Chat
- Drag-and-drop silent failure: rejected files clear `isDragging` without toast
- Pinned messages not persisted to session metadata (lost on reload)
- Token usage ring unlabeled — no legend for input vs. output vs. context limit
- Stop button state desync between header and input
- No conversation fork (ChatGPT/Claude/Codex all have "branch from message")

### Tasks
- Tab labels unclear: "Active / History / Worktrees" → recommend "Running / Past / Branches"
- 6-column viewport overstuffed on small laptops — collapse secondary widgets into tabs
- "Hook Task Pipeline" widget has no tooltip / explanation
- Two primary CTAs compete (New Background Task vs. calendar/list toggle)

### Goals / Agents
- Page named "Goals" but is actually "Agent Activity" — rename mismatch
- Approve/Adjust renders silently with no pending approvals (no CTA in empty state)
- Goal input at bottom has no contract — placeholder promises more than UI delivers

### OPC Task (`/opc/task/:id`)
- Revision note not validated (empty submit accepted)
- Efficiency metrics (cost/tokens/agents) appear fabricated — label as "Estimate" or wire backend

### Mission Control
- No filtering (team / assignee / due date)
- No "Open in Tasks" / "Open in OPC" jump from card drawer

### Extensions
- Sub-nav terminology confused: "Data Sources" should be "Connections (MCP)"
- No enable/disable state on skill cards

### Hooks
- 5 dead events still shown as configurable (Phase E E4 audit `03343c1`)
- Event names use developer jargon (`SubagentStart`, `PostCompact`)

### Routines
- No "Test run" button
- No per-routine execution history drawer

### Profiles
- Tool-name-based selection (`["read_file","bash"]`) — should be categories
- No conflict detection UI (4-tier precedence is invisible)

### Triage
- Sticky bulk-action bar positioning bug with virtualization
- No keyboard navigation (j/k/enter/a/d)
- No visible sort control

### Editor / QuickFix
- Diagnostics shown as raw JSON — should be sentence format
- Quick-fix applies without diff preview

### Settings
- Approval mode has no "recommended for new users" hint
- Model tab sliders don't reflect model capabilities (e.g. o1 ignores temperature)
- Theme grid: only 1 of 4 themes is polished
- Usage & Billing still demo data — needs "Demo" label or backend wiring

### Layout / Sidebar / Header
- Sidebar items don't reflect state (pending approvals, failed routines → badge/dot)
- Header provider chip is decoration-only (no quick-switcher)

## Still open — Cross-cutting

### A. Naming inconsistencies (rename pass)
The full table in `03-senior-pm-audit.md` §A is **not yet applied**:
- Extensions → Integrations
- Data Sources → Connections (MCP)
- Profiles → Approval Profiles
- Worktrees → Workspaces
- Hooks → Automations
- Routines → Schedules
- Strategic Focus → Mission

### B. State feedback
- 47 `catch (e) { console.warn(...) }` patterns still silent
- 23 `toast.error('Failed')` calls don't tell user why
- Loading spinners have no timeout

**Recommendation:** shared error boundary + retry pattern, user-visible error messages with actual cause.

### C. Empty states
Most empty states now have CTAs after PR #50 (ConfirmDialog + LoadingState primitives help), but audit identified ~32 empty states. Recommend a dedicated sweep to verify each one has a CTA.

### D. Mobile / small-screen
App is still desktop-only. No sidebar overlay pattern for <768px. Tauri targets desktop but Windows tablets and small laptops exist.

### E. Accessibility (partial — PR #50 closed some)
Closed by PR #50:
- Focus-visible rings on icon-only buttons
- Focus trap on 18 modal dialogs

Still open:
- No skip-to-main link
- Kanban cards not keyboard-focusable (only buttons)
- Drag-and-drop is mouse-only — no keyboard equivalent
- Priority badges use color alone — needs icon + text

## Recommended next sprint

**Week A — Finish P0 honesty**
1. Wire Chat attach button (`accept` filtering + thumbnail preview)
2. Make Extensions skill cards clickable → detail drawer + install flow

**Week B — Naming + state feedback**
3. Apply rename table (route aliases with redirects)
4. Add shared error boundary + replace silent catches
5. Add "Demo" labels to billing page

**Week C — Power-user page polish**
6. Routines: Test button + execution history
7. Triage: keyboard nav + sort dropdown + sticky fix
8. Profiles: category-based selection + conflict detection
9. Hooks: hide 5 dead events + plain-English names

**Week D — Remaining a11y + mobile**
10. Skip-to-main link
11. Keyboard dnd for kanban
12. Sidebar overlay for <768px
13. Icon + text on priority badges

## Reference: PR #50 closed items (for cross-check)

Confirmed shipped in PR #50 (audit → resolved):
- Welcome duplicate paragraph
- OPC HIL buttons gated on pending permission (ConfirmDialog + dirty guard)
- Billing demo mode disable (Change Plan / Cancel)
- Memory dirty guard
- ConfirmDialog before destructive ops (Extensions 4 tabs + OPC)
- Conversations click → load session
- InstallDialog floating-branch warning
- Datasources i18n
- Shared LoadingState / ErrorState / ConfirmDialog / SkeletonLoader primitives
- Focus-visible rings (icon-only buttons, chat/task action buttons, form fields)
- Focus trap on 18 modal dialogs
- Mod+n new chat session
- Virtualized chat message list
- Editor Ask AI button + Save flow
- Anthropic + Ollama provider presets
- Parallel Triage bulk ops
- StreamingResponse extracted from Chat.tsx
- Brand icons + micro-interactions
- Three-state migration sweep finished
- Tailwind 4 parser fix
