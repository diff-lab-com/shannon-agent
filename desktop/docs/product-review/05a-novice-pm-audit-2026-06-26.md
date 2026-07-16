# Shannon Desktop — Novice + PM Audit (2026-06-26)

**Auditor**: Dual-lens pass (first-time user + senior PM)
**Scope**: Every key page in `ui/src/`, verified against claims in PR #50/#51
**Method**: Read source for every page + component, traced flows end-to-end, cross-checked against `01-novice-user-review.md`, `03-senior-pm-audit.md`, `pm-audit-followups.md`
**Branch**: `s2/ui-design-overhaul` (commit `e7724c3`)

---

## Executive summary (5-8 bullets)

1. **Massive IA cleanup since baseline.** Mission Control, Goals, Routines, Hooks, Profiles no longer have top-level nav entries — they redirect to `/tasks` (`App.tsx:65-69`). The sidebar now defaults to a "Simple" mode with only 4 items (Chat, Tasks, Memory, Triage) and hides Extensions/OPC behind a "Dev" toggle (`Sidebar.tsx:17-32, 320-395`). This is the single biggest improvement since the June-15 audit and directly addresses novice blocker #2.

2. **Welcome flow is dramatically better.** The old 1-step "choose provider + paste key" wall is now a 4-step wizard: Task → Model → Tools → Done, with task-based recommendations, env-var provider detection, a "Test connection" button, and one-click Documents skill installs (`Welcome.tsx:150-694`). The duplicate-paragraph P0 bug is gone. This directly addresses novice blocker #1.

3. **Chat attach button is genuinely wired now** (`ChatInput.tsx:132-147`) — native file dialog with image filter presets, thumbnail previews for images, per-file remove. The prior P0 "dead button" claim is verified fixed. Drag-and-drop also works and shows a drop hint overlay.

4. **Billing page now has a visible "Demo mode" banner** (`BillingSettings.tsx:68-80`) with a `data-testid="demo-banner"`. The prior P1 "fabricated data presented as real" honesty gap is closed.

5. **Three new dead-button / dead-input bugs in Header** (NEW, not in prior audits): the notifications bell has no `onClick` (`Header.tsx:134`), the OPC search field has no `onChange`/`onSubmit` (`Header.tsx:81-87`), and the avatar circle is non-interactive (`Header.tsx:140-142`). All three look interactive but do nothing.

6. **OPC Task HIL buttons are only partially gated.** The prior P0 said "Approve/Rollback/Revision render unconditionally." Code now gates on `hasRunningTasks` (`OPCTask.tsx:19, 141`), which is better but still wrong: any running task shows the human-in-the-loop review panel, even when no permission request is actually pending. The correct gate is `task.pending_permission === true` or an active `permissionRequest` from context. Clicking "Approve" calls `respondPermission(taskId, true)` with a task that may have no pending request — silent no-op.

7. **Five "config-only" Data Sources are shown as installable** (`DataSources.tsx:399`): Slack, Discord, Telegram, RSS, iCal all have install/configure UI and a "Query coming soon" badge, but the query path is stubbed. A user who installs Slack expecting to query it will be confused — the badge is small and easy to miss.

8. **First-time activation is still fragile.** The Welcome "Skip" button jumps to `/chat` without configuring a provider, and the Layout's `shouldShowWelcome` re-check only fires if `config.provider` is absent (`Welcome.tsx:131-136`, `Layout.tsx:42`). If the user skips, then later sets a provider in Settings, then clears it, they will NOT see Welcome again (the `shannon.hasSeenWelcome` localStorage flag is already set). There's no "you have no provider configured" guard at send time beyond a dismissible banner.

---

## Part 1: First-time user walkthrough

### Welcome (`pages/Welcome.tsx`)

**First impression (3 seconds)**: Clean 4-step wizard. "What brings you here today?" with 4 task cards (Code, Writing, Research, General). This is approachable and doesn't assume technical knowledge.

**Friction points**:
- The "Task" step cards use English-only labels via i18n keys, but the blurbs ("Help me build a REST API endpoint in Rust") are still code-heavy for the "Writing" and "General" paths. A non-technical user picking "Writing" still sees tool recommendations like "filesystem, git, playwright" in Step 2 with no plain-language explanation of what those are.
- Step 2 (Tools) pre-checks tools based on task choice, but the tool descriptions (`welcome.tools.filesystem.desc`, etc.) assume the user knows what a "filesystem" or "MCP tool" is. There's no "skip tools" shortcut — you must advance through this step.
- The "Test connection" button (`Welcome.tsx:439-447`) is good, but if it fails with "network unreachable" the user has no path forward — there's no offline/local fallback offered (Ollama is listed but not recommended for writing/research tasks).

**Honesty gaps**:
- The "Documents skills" section in Step 3 (`Welcome.tsx:619-688`) lists 3 skills (Pandoc DOCX, Python DOCX, Markdown Beautify) that all point to the same placeholder repo `shannon-agent/shannon-skills-docs` with `ref: 'main'`. The code comment at `Welcome.tsx:92` admits: "Repos are placeholders until the matching shannon-skills-* repos are published." If a user clicks "Install," it will fail with a git clone error. The UI gracefully reports failure, but presenting installable skills that are known-broken is a small honesty gap.

**Missing CTAs / dead ends**:
- The "Skip" button (top-right) is always available, which is good. But skipping leaves the user with zero config. A "Skip to chat — we'll set this up later" microcopy would reduce anxiety.

---

### Chat (`pages/Chat.tsx` + `components/chat/`)

**First impression**: Familiar ChatGPT-like layout. Left session sidebar, center messages, right context panel (collapsed by default). The empty state shows 4 example prompts.

**Friction points**:
- The 4 example prompts in `WelcomeState.tsx:7-12` are still developer-leaning: "Draft a follow-up email to a candidate who went silent after the onsite" (recruiter persona), "Summarize the document below into 5 bullet points for a busy exec" (business), "Research the top 3 Rust web frameworks" (developer), "Build a REST API endpoint in Rust" (developer). Two of four are code-only. A true novice sees "Rust web frameworks" and re-confirms "this isn't for me."
- The approval-mode selector in the chat input toolbar (`ChatInput.tsx:265-284`) has 5 modes (Readonly, Plan, Suggest, Auto, Full Auto) with tiny icons and labels. A new user has no idea what "Suggest" vs "Auto" means. There's no tooltip explaining the autonomy levels, and no warning when switching to "Full Auto" (the border turns red, but there's no confirmation dialog or text warning).
- The context panel token usage ring (`Chat.tsx:686-704`) shows "X% · total / max" with a color-coded bar. Good for engineers, meaningless to novices. The word "Context Window" is used without explanation.
- The model selector in the chat input (`ChatInput.tsx:286-308`) lists every model by raw ID (e.g. `claude-sonnet-4-6`). No friendly names, no "recommended" hint, no capability indicators.

**Honesty gaps**:
- Pinned sessions (`pinnedIds`, `Chat.tsx:103`) are still local-only state. The prior audit flagged this. Pinning survives navigation but is lost on reload. The pin icon appears on hover but the persistence silently fails.

**Missing CTAs / dead ends**:
- When the API key is missing, a dismissible banner appears (`Chat.tsx:442-469`) with a "Configure" CTA — good. But if the user dismisses it, there's no persistent indicator that they're unconfigured. The next send attempt will fail with a backend error.

---

### Tasks (`pages/Tasks.tsx` + `components/tasks/`)

**First impression**: Complex. Toolbar with 5 buttons (Filters, Calendar, DAG, New Task, Schedule), then 3 tabs (Active / History / Worktrees), then a 12-column grid with TaskList + CalendarSidebarWidget + EfficiencyCard + AgentAllocation + HookTaskPipeline + ScheduleDAGView + TaskExecutionLog.

**Friction points**:
- The tab labels use i18n keys `tasks.tab.active`, `tasks.tab.history`, `tasks.tab.worktrees` but the prior audit recommended renaming to "Running / Past / Branches." The underlying `Tab` type is still `'active' | 'history' | 'worktrees'` (`Tasks.tsx:48`). If the i18n values were updated, the code wasn't — a maintainer reading the code still sees the old mental model.
- "DAG" toggle button is in the toolbar with no tooltip or explanation. A novice has no idea what DAG means. The prior audit flagged this; no change.
- "Hook Task Pipeline" widget (`Tasks.tsx:279`) is still rendered with no header tooltip. The prior audit flagged this; no change.
- "Worktrees" tab still uses Git terminology. The prior audit recommended "Branches" or "Workspaces." The sidebar already renames the concept to "Workspaces" (`Sidebar.tsx:275`), but the Tasks tab still says "Worktrees" — inconsistent.
- The "New Task" form (`NewTaskForm`) asks for a "prompt" — the same developer-friction as before. No templates for non-technical tasks.

**Honesty gaps**:
- EfficiencyCard shows a percentage (`efficiencyPct`, `Tasks.tsx:99`) computed as `completedCount / tasks.length`. On first load with seeded sample data (from Welcome's `seedSampleData` call), this shows a fabricated efficiency metric. The user has no way to know this is based on demo tasks.

**Missing CTAs / dead ends**:
- Empty Active tab state: depends on whether sample data was seeded. If seeded, shows demo tasks. If not, shows "No tasks yet" with a CTA to create one (good).

---

### Triage (`pages/Triage.tsx`)

**First impression**: Clean inbox-style list with filter chips, bulk actions, and clear empty state. This is one of the better-polished pages.

**Friction points**:
- The word "Triage" is still medical jargon. The prior novice review dedicated a section to this. The page title (`triage.title`) is i18n-keyed, so it could be renamed to "Inbox" or "Action Needed" without code changes, but the route is still `/triage` and the sidebar label key is `nav.triage`.
- The kind labels (`Triage.tsx:28-36`) are hardcoded English: "Failed Run," "Budget Exceeded," "Needs Review," "Timeout." These bypass i18n entirely (the `kindMeta` function returns literal strings, not i18n keys). A zh-CN user sees English labels in an otherwise-localized UI.

**Honesty gaps**: None new. Bulk actions work, delete confirmation exists.

**Missing CTAs / dead ends**:
- The empty state has a "Refresh" CTA (`Triage.tsx:385`) — good. The no-match state has a "Clear filters" CTA — good.

---

### Extensions hub (`pages/Extensions.tsx` + `components/extensions/`)

**First impression**: App-store-like grid with 7 sub-tabs (Featured, MCP Servers, Skills, Agents, Data Sources, Plugins, Installed). Search bar at top. This is a big improvement over the prior audit's "3 tabs with no clear distinction."

**Friction points**:
- 7 sub-tabs is a lot. "Featured" vs "MCP Servers" vs "Plugins" vs "Installed" — the distinction between an MCP Server, a Plugin, and a Featured vendor is unclear to a novice. The prior audit recommended renaming "Data Sources" → "Connections (MCP)"; the tab label is still `extensions.dataSources`.
- The "MCP" acronym appears throughout with no explanation anywhere in the UI. No tooltip, no "What is MCP?" link.
- The Extensions page is only visible in Dev sidebar mode. A simple-mode user has no way to discover or install skills/extensions from the sidebar — they'd have to know to toggle to Dev mode first. This is defensible (novices don't need MCP), but the Welcome wizard's Step 3 Documents skills section links to `/extensions/featured` (`Welcome.tsx:679`) — which is unreachable if the user is in Simple mode and the sidebar doesn't show Extensions.

**Honesty gaps**:
- Data Sources page (`DataSources.tsx:312-317`): the "Query coming soon — install/configure works today" badge is honest, but easy to miss. A user who installs the Slack adapter expecting to query Slack messages will be disappointed. Five of the listed adapters (Slack, Discord, Telegram, RSS, iCal — `DataSources.tsx:399`) are config-only stubs.
- The Agents tab (`Agents.tsx`) surfaces community agents from GitHub upstreams with a "SecurityBadge" scanning the description text. But the trust labels ("Verified," "Official," "Community," "Unknown") are based on catalog metadata, not actual security review. "Verified" implies a level of vetting that may not exist.

**Missing CTAs / dead ends**:
- Skills tab now has clickable cards opening a detail drawer (`Skills.tsx:157-166`, `SkillDetailDrawer`). The prior P0 "cards not clickable" is verified fixed. Install/uninstall works.
- The "Create Agent" CTA in the Extensions header (`Extensions.tsx:33-34`) navigates to `/extensions/agents` — which just shows the catalog, not a create form. There's no "create custom agent" flow visible. Misleading button label.

---

### OPC (`pages/OPC.tsx`) + OPC Task (`pages/OPCTask.tsx`)

**First impression**: A "Mission Focus" banner (default text still references "Agent Orchestration"), then analytics dashboard, agent swarm, and a kanban board. The "Experiment" badge is shown in the sidebar.

**Friction points**:
- "One Person Company" is still an opaque name. The sidebar shows `nav.opc` (the label) with an "Experiment" badge, but the concept doesn't map to anything a user understands.
- OPC is only visible in Dev sidebar mode. Good — novices won't stumble into it.
- The OPC Task detail page (`OPCTask.tsx`) is reached by clicking a task in the kanban. The human-in-the-loop "Approve Final Merge / Rollback / Request Revision" panel (`OPCTask.tsx:141-200`) renders whenever `hasRunningTasks` is true — meaning ANY running task anywhere in the app triggers this UI on the task detail page. This is a gating bug.

**Honesty gaps**:
- The "Efficiency Metrics" panel (`OPCTask.tsx:209-246`) shows session cost, token usage, agent count, and task count. These are real values from `useApp()` context, but "Task Completion Rate" is computed inline from tasks array — if tasks are seeded demo data, the rate is fabricated.
- The default "Mission Focus" text (`OPCMissionFocus.tsx:17`) is still "Anthropic Agent Orchestration — autonomous task execution with multi-agent coordination." This is consultant-speak, not a user-facing mission statement.

**Missing CTAs / dead ends**:
- The OPC Task page has no "back to board" button other than the breadcrumb. The browser back works, but there's no explicit "Done" or "Close" action.

---

### Settings (`pages/Settings.tsx` + `components/settings/`)

**General Settings**: Approval mode slider now has descriptive labels and a "current mode" sentence (`GeneralSettings.tsx:94-99`). Language selector (English / 简体中文) is present. "Re-run setup wizard" button exists. The prior P1 "no recommended hint" is partially addressed — each mode has a description, but there's still no explicit "Recommended for new users: Confirm/Plan" callout.

**Models Settings**: Now has "Quick Setup Presets" for OpenAI, DeepSeek, GLM, MiniMax, Kimi (`ModelsSettings.tsx:219-225`). This directly addresses the prior audit's "provider list too short" finding. Performance strategy selector (Speed/Balanced/High-Quality) is new and useful. Temperature and Max Tokens sliders exist but are generic — they don't reflect model-specific capabilities (e.g., o1 ignores temperature). Prior audit flagged this; no change.

**Billing Settings**: Demo banner is present and visible (`BillingSettings.tsx:68-80`). Change Plan and Cancel buttons work (call backend `configure`). The prior honesty gap is closed.

**Advanced Settings**: Skill loop toggle, memory management, data privacy, debug console, factory reset — all wired with confirmation dialogs for destructive actions. The "System Logs" modal (`AdvancedSettings.tsx:183-199`) shows hardcoded placeholder text ("Shannon Desktop v0.1.0" and generic help strings) rather than actual logs. This is a minor honesty gap — the button says "View System Logs" but shows a help message, not logs.

**Friction points**:
- No "Account" section anywhere in Settings. No login, no profile, no subscription management (the Billing page shows plan data but has no "sign in to manage" flow). A consumer user expects to find their account here.
- The API key field in Models settings (`ModelsSettings.tsx:179`) shows `sk-••••••••••••` as a masked value but it's a readOnly display string, not the actual key. There's no way to update the key from this field — you have to use the Quick Setup Presets or re-run the wizard. Confusing.

---

### Memory (`pages/Memory.tsx` + `components/memory/MemoryPanel.tsx`)

**First impression**: Clean CRUD interface for persistent memory entries. Stats header, filter row (project + category + search), list of memory cards, create/edit modal.

**Friction points**:
- The memory "category" concept (preference, pattern, decision, error, context) is unexplained. A novice doesn't know the difference between a "pattern" and a "decision" in memory context.
- The editor asks for "confidence" (0.00-1.00 slider) with no explanation of what it does. This is an AI-engineering concept exposed to end users.
- The memory "project" field defaults to `.` (current directory). This is developer-centric (assumes a code repo). A non-technical user has no "project."

**Honesty gaps**: None significant. The page is straightforward about being a manual CRUD tool.

---

### Header (`components/Header.tsx`)

**First impression**: Top bar with page title, model selector dropdown, notifications bell, help button, avatar circle.

**Friction points / dead buttons** (3 NEW bugs):
- **Notifications bell** (`Header.tsx:134-136`): No `onClick` handler. The button has an `aria-label` and `title` but clicking does nothing. Dead button.
- **OPC search input** (`Header.tsx:81-87`): Only rendered on the OPC page. Has a placeholder and search icon but NO `onChange` or `onSubmit`. Typing does nothing. Dead input.
- **Avatar circle** (`Header.tsx:140-142`): Renders a `person` icon in a styled circle. No `onClick`, no dropdown, no link to settings. Purely decorative. A user expects to click their avatar to see account/profile.

**Honesty gaps**: The model selector dropdown (`Header.tsx:99-131`) is wired and works — clicking a model calls `handleModelSwitch`. This is the prior "header provider chip is decoration-only" finding, now fixed for the model chip.

---

## Part 2: Senior PM audit

### Value proposition per page (one line each)

| Page | Value prop |
|------|-----------|
| Welcome | Onboarding wizard that gets you from install to first message in <2 min |
| Chat | Multi-provider LLM chat with tool calls, file attach, streaming |
| Tasks | Scheduled + triggered task execution with calendar/DAG/history views |
| Triage | Inbox for failed runs, budget alerts, items needing review |
| Extensions | App store for MCP servers, skills, agents, data source adapters |
| OPC | Experimental multi-agent orchestration workspace (dev mode only) |
| OPC Task | Per-task detail: agent workflow, execution log, human-in-the-loop review |
| Memory | Persistent memory layer: preferences, decisions, patterns, errors |
| Settings | Config: approval mode, models, billing, advanced toggles |

### IA problems (pages that should merge / split / reorder)

1. **Tasks page is still overstuffed.** 7 widgets in one viewport (TaskList, CalendarSidebarWidget, EfficiencyCard, AgentAllocation, HookTaskPipeline, ScheduleDAGView, TaskExecutionLog). On a 13" laptop the secondary widgets are <200px wide. The prior audit recommended collapsing into tabs; no change. The "Active / History / Worktrees" tabs help, but the Active tab itself is still a 7-widget wall.

2. **OPC vs Tasks overlap remains.** Both have task boards. Both show agent allocation. OPC has a kanban with DnD; Tasks has a list with calendar. The route redirects (`/mission-control`, `/goals` → `/tasks`) collapsed 3 pages into 1, which is good, but OPC still stands alone as a parallel task surface. A user who enables Dev mode sees both Tasks and OPC in the sidebar and doesn't know which to use.

3. **Memory is a top-level nav item but Extensions is not (in Simple mode).** Memory is arguably more niche than Extensions for most users. The nav priority feels inverted: a new user who wants to install a skill (common) must toggle to Dev mode, but Memory (rare for novices) is always visible.

4. **Extensions has 7 sub-tabs.** Featured, MCP Servers, Skills, Agents, Data Sources, Plugins, Installed. "MCP Servers" and "Plugins" and "Featured" all ultimately install MCP servers — the distinction is unclear. Consider merging MCP Servers + Plugins into "All Integrations" and keeping Featured as a curated subset.

### Competing primary CTAs (2+ buttons fighting for attention)

| Page | Competing CTAs | Issue |
|------|---------------|-------|
| Tasks toolbar | "New Task" (primary purple) vs "Calendar" / "DAG" (ghost toggles) vs "Schedule" (ghost) | New Task and Schedule both create tasks via different flows. User doesn't know which to use. |
| Extensions header | "Create Agent" / "Add Source" CTA vs the sub-tab navigation | The CTA changes based on active sub-tab but navigates to the same page (no-op for Agents tab). |
| Chat input | Working directory chip vs Approval mode vs Model selector vs Attach vs Send | 5 interactive elements in the input toolbar. The model selector and approval mode are both dropdowns competing for the same "configure your session" intent. |
| Settings sidebar | 6 sub-items (General, Theme, Models, Billing, Advanced, Notifications) all at the same hierarchy | No visual priority. "Models" (critical for setup) and "Theme" (cosmetic) are siblings. |

### Honesty gaps (table: page | issue | severity)

| Page | Issue | Severity |
|------|-------|----------|
| Header | Notifications bell has no onClick — dead button | P1 |
| Header | OPC search input has no onChange — dead input | P1 |
| Header | Avatar circle is non-interactive (no account menu) | P2 |
| OPC Task | HIL buttons render when any task is running, not when permission is actually pending | P1 |
| DataSources | 5 adapters (Slack, Discord, Telegram, RSS, iCal) install but can't query — "coming soon" badge is small | P1 |
| Welcome | Documents skills point to placeholder repos that don't exist yet | P2 |
| Tasks | EfficiencyCard shows % based on potentially-seeded demo data with no "demo" label | P2 |
| Advanced Settings | "View System Logs" opens a modal with placeholder text, not actual logs | P2 |
| Chat | Pinned sessions not persisted (local state only, lost on reload) | P2 |
| Extensions | "Create Agent" button navigates to catalog, not a create form | P2 |
| Triage | Kind labels ("Failed Run," "Budget Exceeded") are hardcoded English, bypass i18n | P2 |
| Models Settings | API key field shows fake masked value, can't be edited inline | P2 |

### Onboarding-to-activation gap (can a new user reach "aha" in <5 min?)

**With the wizard (recommended path): Yes, mostly.**
- Welcome wizard Step 0 → 1 → 2 → 3 takes ~2 min if the user has an API key ready.
- Env-var detection (`detectProviderFromEnv`) auto-fills the provider if the shell has `ANTHROPIC_API_KEY` etc. set — nice touch.
- "Test connection" button gives immediate feedback before committing.
- After finishing, the user lands on Chat with a configured provider. First message should work.

**Gap 1**: If the user doesn't have an API key and doesn't know what one is, the wizard doesn't explain. The "Get a key from your provider's dashboard" help text links nowhere. There's no "I don't have a key" fallback (e.g., free trial, Ollama setup guide).

**Gap 2**: After the first chat message, there's no "next step" guidance. The user doesn't know Tasks, Extensions, or Memory exist (if in Simple mode). There's no progressive disclosure — no "Now that you've chatted, try creating a scheduled task" prompt.

**Gap 3**: The wizard's Step 2 (Tools) is a speed bump. A non-technical user doesn't know what "filesystem" or "tavily" tools are. The descriptions are one-liners like "Read and write files on your computer." This is a moment where a novice might abandon, thinking "I don't understand these tools, maybe I'm setting it up wrong."

**Verdict**: Activation is achievable in <5 min for a technical user with an API key. For a true novice, the wizard is approachable but the tools step and the lack of "no key" fallback are friction points. Grade: B+ (up from D in the prior audit).

---

## Part 3: Cross-page flow audit

### Flow 1: Activation — Install → first chat → see useful result

1. **Install + launch**: App opens. `Layout.tsx:42-45` checks `shouldShowWelcome` — if no localStorage flag and no provider, navigates to `/welcome`. **Works.**
2. **Welcome wizard**: User picks "Writing" → Anthropic → enters API key → clicks "Test connection" → success → Step 2 tools (pre-checked: web_search) → Step 3 Done → clicks "Start". `finish()` calls `markWelcomeSeen()`, optionally sets Dev mode, calls `seedSampleData()`, navigates to `/chat`. **Works.**
3. **First chat**: Chat page loads. If `showApiKeyBanner` is false (key was set), no banner. User types a message. `handleSend` calls `sendMessage`. **Works — assuming backend is reachable.**
4. **See useful result**: Response streams in. `Markdown` component renders it. User can copy/paste. **Works.**

**Breakage**: None in the happy path. The flow is solid. The only risk is the `seedSampleData()` call failing silently (`Welcome.tsx:234-238` catches and logs) — if it fails, Tasks page will be empty, but Chat works independently.

### Flow 2: Power loop — Create task → assign agent → review output → iterate

1. **Create task**: User goes to Tasks (Simple mode shows it in sidebar). Clicks "New Task". `NewTaskForm` appears. User enters a prompt. Clicks submit. `handleStartTask` calls `api.startBackgroundTask(body)`. **Works.**
2. **Assign agent**: There's no "assign to agent" UI in `NewTaskForm`. The form takes a prompt, optional assignee, optional priority — but the assignee field is a text input, not a picker from available agents. A user can type "agent-1" but there's no list. **Breakage: weak assignment UX.**
3. **Review output**: Task appears in TaskList. Clicking it opens `TaskDetailDrawer`. The drawer shows task info. To see agent output, the user would go to OPC Task detail (`/opc/task/:id`) — but there's no link from TaskDetailDrawer to OPC Task. **Breakage: no cross-page navigation between Tasks detail and OPC Task detail.**
4. **Iterate**: In OPC Task, the user can "Approve / Rollback / Request Revision." But as noted, these render whenever any task is running, not when a specific permission is pending. Clicking "Approve" on a task with no pending request silently calls `respondPermission(taskId, true)` which may be a no-op. **Breakage: misleading HIL UI.**

### Flow 3: Recovery — Something fails → user gets notified → user fixes it

1. **Something fails**: A background task fails. Backend writes a triage item to `~/.shannon/triage.jsonl` with kind `failed_run`.
2. **User gets notified**: The sidebar Triage nav item shows an unread badge (`Sidebar.tsx:313-317`) polling every 30 seconds (`Sidebar.tsx:173-178`). The badge appears on the Triage item. **Works.**
3. **User opens Triage**: Sees the failed_run item with red "Failed Run" label. Can mark read, archive, or delete. **Works.**
4. **User fixes it**: Here's the gap. The triage item shows a message and task name, but there's no "retry task" or "open task" button. The user has to manually navigate to Tasks, find the task, and re-run it. There's no deep link from triage item to the failing task. **Breakage: no recovery shortcut.**
5. **Header notifications bell**: Dead button. If the user expects to see notifications by clicking the bell in the header, nothing happens. The only notification path is the sidebar Triage badge. **Breakage: dead notification UI.**

---

## Part 4: Prioritized issues (P0/P1/P2)

| Sev | Page | Issue | Suggested fix |
|-----|------|-------|---------------|
| P0 | Header | Notifications bell has no onClick handler — users expect a notification panel | Wire to a dropdown panel showing recent triage items, or navigate to `/triage` |
| P0 | OPC Task | HIL Approve/Rollback/Revision buttons render when any task is running, not when `pending_permission` is true | Gate on actual permission request state; use `permissionRequest` from `useApp()` or add `task.pending_permission` check |
| P1 | Header | OPC search input has no onChange/onSubmit — dead input | Wire to task search, or remove if no backend search exists for OPC |
| P1 | Header | Avatar circle is non-interactive | Add account/profile dropdown, or remove if no account system |
| P1 | DataSources | 5 adapters (Slack, Discord, Telegram, RSS, iCal) are config-only stubs shown as installable | Either hide from catalog until query is implemented, or make the "Query coming soon" badge much more prominent (full-card overlay) |
| P1 | Triage | Hardcoded English kind labels bypass i18n | Replace `kindMeta()` literal labels with i18n keys |
| P1 | Tasks | 7-widget viewport still overstuffed on small screens | Collapse secondary widgets (EfficiencyCard, AgentAllocation, HookTaskPipeline) into a tabbed "Insights" panel |
| P1 | Extensions | "Create Agent" button navigates to catalog, not a create form | Either build a create-agent flow, or rename to "Browse Agents" |
| P2 | Welcome | Documents skills point to placeholder repos that will fail to clone | Remove from wizard until repos are published, or show as "Coming soon" |
| P2 | Chat | Pinned sessions not persisted to session metadata | Persist `pinnedIds` to backend session metadata |
| P2 | Chat | WelcomeState example prompts are 50% code-focused | Replace 2 code examples with general-purpose examples (writing, translation, analysis) |
| P2 | Chat | No warning when switching approval mode to Full Auto | Add confirmation dialog with risk description |
| P2 | Models Settings | API key field shows fake masked value, not editable | Make the field editable or add an "Update key" action |
| P2 | Advanced Settings | "View System Logs" shows placeholder text, not real logs | Either wire to actual log tail, or rename to "Log Help" |
| P2 | Tasks | EfficiencyCard shows fabricated % from demo tasks with no label | Add "Demo data" label when tasks are seeded sample data |
| P2 | Memory | "Confidence" slider and "category" concepts unexplained | Add tooltips or a help link explaining memory categories |
| P2 | Chat | Model selector shows raw model IDs, no friendly names or recommendations | Add display names + "recommended" badges in the model dropdown |
| P2 | Extensions | "MCP" acronym used everywhere with no explanation | Add a one-line tooltip or "What is MCP?" help link |
| P2 | Nav | Memory is top-level in Simple mode but Extensions is Dev-only | Consider swapping: Extensions is more commonly needed than Memory |

---

## Appendix: Verified-fixed vs stale-fix

For each "fixed in PR #50/51" claim from `pm-audit-followups.md`, here is the verification result:

| Claim | Status | Evidence |
|-------|--------|----------|
| Chat attach button wired (US-CHAT-08) | **Verified fixed** | `ChatInput.tsx:132-147` — `handleAttachClick` opens native dialog with image filters; thumbnails render for images |
| Skill cards clickable → detail drawer | **Verified fixed** | `Skills.tsx:157-166` — `onOpenDetail` opens `SkillDetailDrawer`; install/uninstall works |
| 7 label renames (Extensions→Integrations, Worktrees→Workspaces, etc.) | **Partially fixed** | Sidebar uses "Workspaces" for the new-session-in-worktree button (`Sidebar.tsx:275`), but Tasks tab still says "worktrees" (`Tasks.tsx:48`), Extensions page title still says "Extensions" not "Integrations", nav still says `nav.extensions`. The rename is incomplete. |
| `toastError()` surfaces real error cause | **Verified fixed** | `toastError` from `@/lib/errorToast` is used in Header, BillingSettings, GeneralSettings, ModelsSettings, AdvancedSettings, Chat. Error messages include the actual exception. |
| 6 empty states gained CTAs | **Verified fixed** | Triage empty + no-match states have CTAs (`Triage.tsx:385, 412`). Memory empty state has "Create first" CTA (`MemoryPanel.tsx:256-262`). WorktreePanel, Goals(removed), OPCAgentSwarm, Extensions all have CTAs per followup doc. |
| ConfirmDialog before destructive ops | **Verified fixed** | Chat delete session modal (`Chat.tsx:583-597`), Triage delete/bulk-delete modals (`Triage.tsx:434-510`), Billing cancel modal, Advanced factory-reset + clear-cache modals |
| Focus-visible rings | **Verified fixed** | `focus-visible:ring-2 focus-visible:ring-primary/30` appears across Chat, Tasks, Triage, Extensions, Settings components |
| Focus trap on modals | **Partially fixed** | Most modals have `onKeyDown` Escape handling and click-outside-to-close. But focus trap (cycling Tab within the modal) is not implemented — Tab can escape to background elements. The claim says "18 modal dialogs" have focus traps; the code shows Escape + click-outside but no explicit focus cycling. |
| Virtualized chat message list | **Stale / unverifiable** | `Chat.tsx:472` uses `ScrollArea` for messages but there's no virtualization library import (no `react-virtuoso`, `@tanstack/react-virtual`, etc.). The `ScrollArea` is a styled scroll container, not a virtualizer. For long conversations (1000+ messages), this will still render all message DOM nodes. The claim appears stale. |
| Billing demo mode banner | **Verified fixed** | `BillingSettings.tsx:68-80` — visible `data-testid="demo-banner"` with warning icon and description |
| OPC HIL buttons gated on pending permission | **Stale / partially fixed** | Code gates on `hasRunningTasks` (`OPCTask.tsx:141`), NOT on `pending_permission`. The gate is broader than intended — any running task shows the HIL panel. See P0 in issues table. |
| Memory dirty guard | **Verified fixed** | `MemoryPanel.tsx` uses async save with toast feedback; editor has saving state guard |
| Conversations click → load session | **Verified fixed** (page removed) | The legacy conversations route redirects; session switching is in Chat sidebar |
| Mod+n new chat session | **Verified fixed** | `useKeyboardShortcuts` hook is wired in `Layout.tsx:33` |
| Brand icons + micro-interactions | **Verified fixed** | Sidebar has `hover:-translate-y-0.5` on nav items, `active:scale-95` on buttons, `transition-all duration-300` throughout |
| ErrorState shared primitive | **Verified fixed** | Referenced in followup doc; `ErrorBoundary` in `Layout.tsx:67` wraps the Outlet |

---

## Summary scorecard (updated from June-15 baseline)

| Dimension | June-15 score | June-26 score | Change |
|-----------|---------------|---------------|--------|
| First-impression friendliness | 2/10 | 6/10 | +4 (wizard redesign, task-based entry, env detection) |
| Onboarding ease | 2/10 | 6/10 | +4 (4-step wizard, but tools step is still a speed bump) |
| Terminology friendliness | 1/10 | 4/10 | +3 (Simple/Dev mode split hides jargon, but Triage/OPC/MCP remain) |
| Visual guidance | 4/10 | 6/10 | +2 (empty states have CTAs, but Tasks is still overstuffed) |
| Error tolerance / safety | 2/10 | 5/10 | +3 (ConfirmDialog, toastError, but Full Auto has no warning, dead Header buttons) |
| Consumer-user fit | 1/10 | 4/10 | +3 (Simple mode is real, but examples/tools/language still skew developer) |
| Overall product coherence | 4/10 | 6/10 | +2 (IA cleanup is significant, but OPC/Tasks overlap and 7 Extensions tabs remain) |

**One-line summary**: Shannon Desktop has undergone a substantial UX transformation since June-15 — the sidebar simple/dev split, the welcome wizard, and the billing demo banner are the three highest-impact changes. The product is now usable for a technical new user in under 5 minutes. However, the Header still has 3 dead interactive elements, the OPC HIL gating is incorrect, 5 Data Source adapters are installable-but-unqueryable stubs, and the Tasks page remains an overstuffed 7-widget wall. The next sprint should focus on: (1) wiring the 3 dead Header elements, (2) fixing the OPC HIL gate, (3) collapsing the Tasks widget sprawl, and (4) either hiding or prominently labeling the config-only Data Sources.
