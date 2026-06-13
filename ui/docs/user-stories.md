# Shannon Desktop UI - User Stories (Complete)

## Chat Page

### US-CHAT-01: Send Messages
As a user, I want to type and send messages to the AI so I can get coding assistance.
- Type in input, press Enter to send
- Shift+Enter for multi-line
- Empty messages not sent
- Input disabled during query
- Send button disabled when input empty
- Stop button shown during query, cancels on click
**Test coverage**: 25 tests in Chat.test.tsx ✅

### US-CHAT-02: Session Management
As a user, I want to manage chat sessions so I can organize conversations.
- Create new session via "New Chat" button
- Switch between sessions by clicking
- Search sessions by name (with highlight)
- Rename session inline (double-click → edit → Enter/blur)
- Delete session with right-click confirmation
- Show message count and time per session
**Test coverage**: Covered in Chat.test.tsx ✅

### US-CHAT-03: Streaming Responses
As a user, I want to see AI responses stream in real-time.
- Streaming text appears with cursor animation
- Thinking indicator shows during processing
- Tool calls display with status icons (running/error/done)
- Tool calls expandable/collapsible
- Auto-scroll to bottom on new content
**Test coverage**: Covered in Chat.test.tsx ✅

### US-CHAT-04: Message Actions
As a user, I want to interact with AI responses.
- Copy message to clipboard (via navigator.clipboard)
- Regenerate AI response (sends "Regenerate" message)
- Like/unlike messages (local state toggle)
**Test coverage**: Covered in Chat.test.tsx ✅

### US-CHAT-05: Context Panel
As a user, I want to see context about the current session.
- Token usage (input/output/cost) in right sidebar
- Active tool calls with status badges
- File context list (path, language, lines)
**Test coverage**: Covered in Chat.test.tsx ✅

### US-CHAT-06: Cancel Query
As a user, I want to cancel an ongoing query.
- Press Escape during query to cancel
- Stop button visible during processing
**Test coverage**: Covered in Chat.test.tsx ✅

### US-CHAT-07: Permission Handling
As a user, I want to approve/deny tool execution.
- Modal shows in Header when permission requested
- Allow/Deny buttons
- Auto-dismisses after response
**Test coverage**: Covered in Header.test.tsx ✅

### US-CHAT-08: Attach File ⚠️ ISSUE
As a user, I want to attach files to my message.
- Attach file button in input bar
- Opens file picker dialog
- Selected files included with message
**Status**: Button exists but has NO onClick handler. Non-functional.
**Test coverage**: NONE

### US-CHAT-09: Message Recall
As a user, I want to recall my last message.
- Alt+Up arrow when input is empty fills last user message
**Test coverage**: Covered in Chat.test.tsx ✅

### US-CHAT-10: Error Display
As a user, I want to see error messages when things go wrong.
- Error banner displayed in chat area
- Red styling for visibility
**Test coverage**: Covered in Chat.test.tsx ✅

---

## Scheduled (Tasks) Page

### US-TASK-01: View Tasks
As a user, I want to see all my scheduled tasks.
- Task list with status badges (Completed, Running, Failed, Pending)
- Task details (title, description, assignee, priority)
- Empty state when no tasks
- Bento card layout
**Test coverage**: 5 + 29 enhanced = 34 tests ✅

### US-TASK-02: Create Background Task
As a user, I want to create new background tasks.
- "New Background Task" button opens prompt dialog
- Enter task prompt and submit
- Task appears in execution log
**Test coverage**: Covered in TasksEnhanced.test.tsx ✅

### US-TASK-03: Cancel Tasks
As a user, I want to cancel running tasks.
- Cancel button on running tasks
- Cancel button on running background tasks
**Test coverage**: Covered in TasksEnhanced.test.tsx ✅

### US-TASK-04: Filter Tasks
As a user, I want to filter tasks by status.
- Filter buttons (All, Pending, Running, Completed)
- Clicking a filter shows only matching tasks
- Active filter highlighted
**Test coverage**: 7 filter tests in TasksEnhanced.test.tsx ✅

### US-TASK-05: Calendar View
As a user, I want to see tasks on a calendar.
- Toggle between calendar and list view
- Month navigation (prev/next)
- Days with tasks shown
- Today highlighted
- Click day to see tasks for that day
**Test coverage**: Calendar structure tests in TasksEnhanced.test.tsx ✅

### US-TASK-06: Task Detail
As a user, I want to see full task details.
- Click task card to open detail drawer
- Shows title, status, description, priority, assignee
- Close button to dismiss
**Test coverage**: Partial - needs detail drawer click tests

### US-TASK-07: AI Efficiency
As a user, I want to see task completion efficiency.
- Percentage of completed tasks
- Progress bar visualization
**Test coverage**: Covered in TasksEnhanced.test.tsx ✅

### US-TASK-08: Agent Allocation
As a user, I want to see how agents are allocated to tasks.
- Agent names with distribution bars
- Shows when agents exist
**Test coverage**: Covered in TasksEnhanced.test.tsx ✅

### US-TASK-09: Run Now
As a user, I want to immediately execute a task.
- "Run Now" button on each task
- Calls startBackgroundTask API
- Refreshes task list after execution
**Test coverage**: Covered in TasksEnhanced.test.tsx ✅

### US-TASK-10: Error Feedback
As a user, I want to see error messages when task operations fail.
- Error banner at top of page
- Dismissible
**Test coverage**: Needs test

---

## Goals Page

### US-GOAL-01: Task Tree View
As a user, I want to see all tasks in a tree structure.
- Active, pending, completed tasks with status indicators
- Progress bars for tasks with progress
- Color-coded status (primary for active, green for done)
- Search to filter tasks in sidebar
**Test coverage**: 4 + 14 enhanced = 18 tests ✅

### US-GOAL-02: Agent Pipeline
As a user, I want to see active agents.
- Agent timeline with names and status
- Agent count in header
- Dashed connection line between agents
**Test coverage**: Covered in GoalsEnhanced.test.tsx ✅

### US-GOAL-03: Human-in-the-Loop
As a user, I want to approve or adjust agent actions.
- Approve button → respondPermission(id, true)
- Adjust button → respondPermission(id, false)
- Only shows when active tasks exist
**Test coverage**: Covered in GoalsEnhanced.test.tsx ✅

### US-GOAL-04: Task Summary
As a user, I want to see task summary counts.
- Active, pending, completed counts in right sidebar
**Test coverage**: Covered in GoalsEnhanced.test.tsx ✅

### US-GOAL-05: Goal Input
As a user, I want to ask questions about my goals.
- Text input with Enter to send
- Send button disabled when empty
- Sends via sendMessage()
**Test coverage**: Covered in GoalsEnhanced.test.tsx ✅

### US-GOAL-06: File Attachment
As a user, I want to attach files to my goal query.
- Attach file button opens file picker
- Selected files included with message
**Test coverage**: Needs test

### US-GOAL-07: AI Assistant Suggestion
As a user, I want AI to suggest next steps.
- AI sparkle button sends suggestion prompt
- Auto-triggers message send
**Test coverage**: Needs test

---

## Extensions Page

### US-EXT-01: Browse Skills
As a user, I want to browse available skills.
- Skills grouped by category (Coding, Research, etc.)
- Color-coded icons per category
- Skill cards with name, description, trigger
- Loaded from API (listSkills)
**Test coverage**: 3 in ExtensionsHub.test.tsx - MINIMAL

### US-EXT-02: Filter Skills
As a user, I want to toggle between trending and recent skills.
- Trending/Recent toggle buttons
- Clicking changes sort order (reverses within categories)
**Test coverage**: Covered in ExtensionsHub.test.tsx ✅

### US-EXT-03: Manage Data Sources
As a user, I want to manage MCP server connections.
- Add server (name, command, args) via form
- Remove server with confirmation
- Restart server (with spinner feedback)
- View connection status and tool count
**Test coverage**: 11 tests in DataSources.test.tsx ✅

### US-EXT-04: View Agents
As a user, I want to see running agents.
- Agent cards with status, model, task, progress
- Performance metrics section
- Task completion percentage
**Test coverage**: 24 tests in MyAgents.test.tsx ✅

### US-EXT-05: Search Extensions
As a user, I want to search extensions.
- Search input with dynamic placeholder
- Filters displayed results
- Context passed via Outlet
**Test coverage**: 10 tests in Extensions.test.tsx ✅

### US-EXT-06: Agent Management Actions
As a user, I want to manage individual agents.
- Configure button shows agent config panel
- More Options dropdown (View Status, Stop Agent)
- View All Tasks navigates to /tasks
**Test coverage**: Needs tests for new handlers

### US-EXT-07: Create New Agent
As a user, I want to create a new agent.
- Click "New Specialization" card → shows creation form
- Textarea for agent description
- Create Agent button sends creation message
- Cancel dismisses form
**Test coverage**: Needs tests

### US-EXT-08: Navigate Extensions
As a user, I want to navigate between extension sub-pages.
- CTA button navigates to correct sub-route
- Search input updates across views
**Test coverage**: 4 tests in Extensions.test.tsx ✅

---

## OPC Page

### US-OPC-01: Kanban Board
As a user, I want to see tasks in a kanban board.
- 5 columns: To Do, Pending, Doing, Done, Deprecated
- Color-coded cards by status
- Priority badges on task cards (Critical for high)
- Progress bars for in-progress tasks
- Done cards link to task detail page
**Test coverage**: 19 tests in OPC.test.tsx ✅

### US-OPC-02: Agent Swarm
As a user, I want to see active agents in sidebar.
- Agent cards with name, model, status indicator
- Active count badge
- Empty state when no agents
- Green pulse for active agents
**Test coverage**: Covered in OPC.test.tsx ✅

### US-OPC-03: Quick Task
As a user, I want to quickly create a task from the OPC board.
- Text input with add button
- Submit creates background task
- Input clears after submit
- Empty input ignored
**Test coverage**: Covered in OPC.test.tsx ✅

### US-OPC-04: Strategic Focus
As a user, I want to see and edit the project's mission statement.
- Display provider-specific or default mission
- Visual banner with label
- Edit button toggles inline editing
- Textarea for editing
- Save Focus persists via api.configure()
**Test coverage**: Needs test for editing flow

### US-OPC-05: OPC Task Detail
As a user, I want to view individual OPC task details.
- Agent workflow visualization
- Task description
- Execution log timeline
- Human-in-the-loop review (Approve/Rollback/Revision)
- Revision note textarea with submit
- Efficiency metrics (cost, tokens, agents)
**Test coverage**: 16 tests in OPCTask.test.tsx ✅

---

## Settings Page

### US-SET-01: Approval Mode
As a user, I want to configure my approval mode.
- Slider with 5 modes (Suggest, Confirm, Plan, Auto Edit, Full Auto)
- Current mode description
- Persisted to backend via api.configure()
- Saving indicator
**Test coverage**: 6 tests in GeneralSettings.test.tsx ✅

### US-SET-02: Model Configuration
As a user, I want to configure AI models.
- Performance strategy toggle (Speed/Balanced/High Quality)
- Provider tabs with model list
- Switch active model (with spinner feedback)
- API key input with visibility toggle
- Temperature slider (persisted via configKey)
- Max tokens slider (persisted via configKey)
- Refresh models button
**Test coverage**: 5 tests in ModelsSettings.test.tsx - MINIMAL

### US-SET-03: Theme Selection
As a user, I want to change the app theme.
- Theme grid with preview cards (4 themes)
- Active theme highlighted with ring
- Click to switch via setTheme()
- Color swatches for active theme
**Test coverage**: 4 tests in ThemeSettings.test.tsx ✅

### US-SET-04: Advanced Configuration
As a user, I want to configure advanced settings.
- Memory management toggle (persisted)
- Clear session cache (with spinner)
- Data privacy toggles (telemetry, encryption)
- Debug console toggle
- View System Logs → modal dialog
- Manage API Keys → modal dialog
- Factory reset with confirmation dialog
**Test coverage**: 9 tests in AdvancedSettings.test.tsx ✅

### US-SET-05: Usage & Billing
As a user, I want to view my usage and billing info.
- Current plan display (name, price, token limit)
- Token usage ring chart
- Cache hit rate ring chart
- Cost analysis bar chart (30 days)
- Billing history table
**Test coverage**: 8 tests in BillingSettings.test.tsx ✅

### US-SET-06: Change Plan
As a user, I want to change my subscription plan.
- Change Plan button opens modal
- Plan picker with Free/Pro/Enterprise options
- Current plan highlighted
- Selection saves via api.configure()
**Test coverage**: Needs test

### US-SET-07: Cancel Subscription
As a user, I want to cancel my subscription.
- Cancel button shows confirmation dialog
- Confirms before canceling
- Calls api.configure() with cancel action
**Test coverage**: Needs test

### US-SET-08: Legal & Privacy
As a user, I want to view legal information.
- Legal & Terms link opens modal
- Privacy Policy link opens same modal
- Shows data handling description
**Test coverage**: Needs test

---

## Audit Summary

### Issues Found

| ID | Priority | Page | Issue |
|----|----------|------|-------|
| BUG-01 | P0 | Chat | Attach file button has no handler (US-CHAT-08) |
| BUG-02 | P1 | ExtensionsHub | Skill cards not clickable - no detail view (US-EXT-01) |
| GAP-01 | P1 | Tasks | No test for task detail drawer click (US-TASK-06) |
| GAP-02 | P1 | Tasks | No test for error feedback banner (US-TASK-10) |
| GAP-03 | P1 | Goals | No test for file attachment (US-GOAL-06) |
| GAP-04 | P1 | Goals | No test for AI assistant button (US-GOAL-07) |
| GAP-05 | P1 | Extensions | No test for agent configure/menu/create (US-EXT-06, US-EXT-07) |
| GAP-06 | P1 | OPC | No test for strategic focus editing (US-OPC-04) |
| GAP-07 | P2 | Settings | No test for change plan modal (US-SET-06) |
| GAP-08 | P2 | Settings | No test for cancel subscription (US-SET-07) |
| GAP-09 | P2 | Settings | No test for legal modal (US-SET-08) |
| GAP-10 | P2 | Settings | No test for models slider persistence (US-SET-02) |
| GAP-11 | P2 | Advanced | No test for system logs / API keys modals (US-SET-04) |

### Test Coverage Gaps
- ExtensionsHub.test.tsx: 3 tests — needs skill card interaction tests
- ModelsSettings.test.tsx: 5 tests — needs slider persistence, model switching tests
- BillingSettings.test.tsx: 8 tests — needs modal interaction tests
- AdvancedSettings.test.tsx: 9 tests — needs modal interaction tests
