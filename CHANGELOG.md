# Changelog

All notable changes to Shannon Desktop are documented here. Entries are grouped by sprint and category.

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
