# Shannon Desktop — Competitive UI Analysis & Improvement Plan

## 1. Current Shannon Desktop Stack

- **Framework**: Tauri v2 + React 18 + TypeScript
- **UI Library**: shadcn/ui (9 primitives: button, input, tabs, dialog, badge, scroll-area, separator, textarea, toast)
- **Dependencies**: @radix-ui/* (dialog, scroll-area, separator, slot, tabs, toggle, toggle-group, tooltip), class-variance-authority, tailwind-merge, lucide-react
- **Styling**: Tailwind CSS with CSS variable theming
- **Testing**: Vitest (364 tests, 36 files)

## 2. Competitor UI Comparison

### Claude Desktop (Anthropic)

| Aspect | Details |
|--------|---------|
| **Layout** | 3-tab system: Chat, Cowork (dispatch), Code (dev). Multi-panel drag-and-drop workspace. |
| **Design Language** | Warm terracotta orange (#C15F3C), cream backgrounds (#F4F3EE). Deliberately "human, warm" feel vs cold blue AI interfaces. |
| **Key Features** | Session sidebar (mission control), split-view (Cmd+click), integrated terminal, file editor, preview pane (embedded browser), diff viewer with inline comments, context usage tracking, model+effort selector, permission mode selector, tasks pane (subagents), side chat (Ctrl+;) |
| **Navigation** | Tab-based primary + filtered sidebar. Cmd+Tab cycles sessions. Command palette. |
| **Unique** | 3 permission modes (Ask/Auto-accept/Plan), compact command for context compression, diff inline comments, GitHub PR monitoring, preview pane for live app testing, drag-drop panel arrangement |

### Codex Desktop (OpenAI)

| Aspect | Details |
|--------|---------|
| **Layout** | 3-panel: Project sidebar (left), Thread list (center), Review pane (right). |
| **Design Language** | Professional, minimal, dark-first. High contrast code display. Tailwind CSS. |
| **Tech Stack** | Electron + React + Radix UI + Framer Motion + TanStack Store |
| **Key Features** | Multi-thread parallel agents, diff viewer (side-by-side/unified), browser panel (in-app browser), skills library, automation scheduler, handoff mode, Figma/Linear integration, appshots capture |
| **Unique** | Computer Use interface (macOS Accessibility API), side-by-side threads, worktree-aware UI, MCP server management, PDF preview, cloud deploy (one-click Vercel/Cloudflare), Chrome extension integration |

### OpenClaw

| Aspect | Details |
|--------|---------|
| **Layout** | Left sidebar (chat history) + main feed + right preview rail |
| **Design Language** | Dark-first, crimson red (#ff5c5c) as primary accent. Inter + JetBrains Mono. Radius scale: 6/10/14/20px. Spring animations. |
| **Key Features** | Real-time tool activity visualization, canvas/webview overlay, screen capture, multi-channel dashboard (Discord/Telegram/Slack/WhatsApp), cron scheduling, session history search |
| **Unique** | 15+ messaging platform integrations, gateway control dashboard (localhost:18789), Android companion app, screen capture/recording in browser panel |

### Hermes

| Aspect | Details |
|--------|---------|
| **Layout** | Left sidebar (sessions/agents/skills) + main chat + right rail (files/terminal/preview) |
| **Design Language** | 6 built-in themes: Nous (blue), Midnight (purple), Ember (orange), Mono (gray), Cyberpunk (green), Slate (GitHub-style). Scalable radius system. |
| **Tech Stack** | Electron + React + Tailwind CSS |
| **Key Features** | Model picker overlay, 100+ skills browser, agent/profile switcher, integrated terminal, sticky human messages (2-line clamp), thinking blocks, onboarding wizard |
| **Unique** | Arc border animation (rotating gradient for loading), composer focus glow effect, scalable radius system, theme asset system (custom backgrounds), 6 built-in themes out of the box |

## 3. Shannon Desktop Improvement Plan

### P0 — Must Have (Phase 7)

| # | Feature | Competitor Reference | Scope |
|---|---------|---------------------|-------|
| 1 | **Theme System** (multi-theme: dark, light, + 2 alternates) | Hermes 6 themes | New ThemeProvider, CSS variable sets, theme switcher in settings |
| 2 | **Signature Accent Color** — define Shannon's brand identity | Claude terracotta, OpenClaw crimson | Update CSS variables, add brand color |
| 3 | **Context Window Usage Indicator** | Claude + Codex | Visual bar in StatusBar showing context usage % |
| 4 | **Onboarding/Welcome Page** | Hermes wizard, Claude welcome cards | First-run experience with provider setup |
| 5 | **Design Token System** (radius scale, spacing scale, animation tokens) | OpenClaw/Hermes | CSS custom properties for consistent design |

### P1 — Should Have (Phase 8)

| # | Feature | Competitor Reference | Scope |
|---|---------|---------------------|-------|
| 6 | **Command Palette** (Cmd+K) | Hermes + Codex | Searchable command/action overlay |
| 7 | **Session Filtering** (by status, project) | Claude mission control | Filter controls in SessionList |
| 8 | **Model + Effort Selector** (combined dropdown) | Claude bottom bar | Merge ModelSelector with effort level |
| 9 | **Side Chat** (branch conversations) | Claude Ctrl+; | Secondary chat panel for quick questions |
| 10 | **Micro-interactions** (animations, transitions) | Hermes springs, Codex Framer Motion | Loading spinners, send button, panel transitions |

### P2 — Nice to Have (Phase 9)

| # | Feature | Competitor Reference | Scope |
|---|---------|---------------------|-------|
| 11 | **Embedded Preview Pane** (dev server browser) | Claude + Codex | WebView pane for running app preview |
| 12 | **Drag-and-Drop Panel Arrangement** | Claude + Codex | User-customizable workspace layout |
| 13 | **GitHub PR Integration** (monitor/auto-merge) | Claude + Codex | PR list, diff review, merge actions |
| 14 | **Automation/Scheduler UI** | OpenClaw + Codex | Cron-like task scheduling interface |
| 15 | **WebView/Canvas Overlay** | OpenClaw | Agent-driven browser overlay |

### P3 — Future

| # | Feature | Competitor Reference |
|---|---------|---------------------|
| 16 | Mobile companion app | OpenClaw Android |
| 17 | Computer Use (desktop automation) | Codex macOS Accessibility API |
| 18 | Cloud deploy integration | Codex one-click Vercel/Cloudflare |
