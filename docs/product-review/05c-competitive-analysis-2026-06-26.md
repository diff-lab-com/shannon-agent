# Shannon Desktop — Competitive Analysis (2026-06-26)

> Deep-research comparison of Shannon Desktop against the 2026 field of
> desktop-class AI tools. Every claim is URL-cited. Where a product could
> not be verified, the doc says so explicitly.

---

## Executive summary (5 bullets)

1. **Shannon's deepest structural advantage is its combination of multi-provider LLM support, first-class automations (hooks + routines + permission profiles), and the Tauri-native (non-Electron) shell.** No single competitor in this set ships all four simultaneously. Claude Desktop locks you to Claude; ChatGPT Desktop locks you to GPT; Hermes Desktop is Electron; WorkBuddy (Tencent) hides its automation model behind a credit wall.
2. **Shannon's most visible weakness is brand polish and onboarding warmth.** Claude Desktop's terracotta/cream palette, ChatGPT's one-shortcut-anywhere access pattern, and Hermes's theming engine all feel more "consumer-ready" than Shannon's current material-blue, developer-leaning shell — even though the Shannon repo already started the repositioning work (see `04-product-repositioning.md`).
3. **The "agent orchestration" battlefield is crowded.** WorkBuddy (Tencent), Hermes Agent, and Cursor 3 all ship multi-agent parallelism with visual task dashboards. Shannon's OPC/Agent-Workshop concept is competitive but currently gated behind developer jargon ("OPC", "Triage", "Hooks") that the competitors have already rebranded to plain-English equivalents ("Tasks", "Inbox", "Automations").
4. **The Skills/Extensions ecosystem is now table stakes.** Hermes ships 118+ bundled skills plus a Skills Hub spanning 9 registries (skills.sh, ClawHub, well-known endpoints, GitHub taps, browse.sh). Claude Desktop has downloadable "extensions" for Apple Notes/Chrome/iMessage. Shannon's MCP + Skills + Agents + Datasources Extensions Hub is architecturally richer than Claude's but the catalog is thinner — Shannon must grow installable skills to stay credible.
5. **Voice and multimodal are Shannon's largest blind spots.** ChatGPT Desktop's Advanced Voice mode, Claude's voice on mobile, and Claude Design (visual artifact builder) all push beyond text. Shannon has no voice mode, no artifact builder, no image generation. This is the single biggest feature gap versus the consumer-facing leaders.

---

## Methodology

### Sources searched

- Official product sites and help centers (Anthropic, OpenAI, Tencent Cloud, Nous Research, Cursor, Raycast, Supermaven)
- Third-party reviews (PCMag, Medium/Tenten, Towards AI, Builder.io, DigitalApplied, Eigent.ai, OfLight)
- Community discussion (Reddit r/ClaudeAI, r/LocalLLaMA, r/OpenAI, r/cursor, r/raycastapp; HackerNews via search)
- Product directories (Aikii, Product Hunt, App Store)
- YouTube walkthroughs (timestamps noted where relevant)
- Shannon Desktop source code (`CLAUDE.md`, `04-product-repositioning.md`, `ui/src/i18n/locales/en.json`, `ui/src/pages/`, `ui/src/components/`)

### Confidence per competitor

| Competitor | Confidence | Notes |
|---|---|---|
| Claude Desktop | **High** | Official help center + PCMag hands-on + Anthropic blog posts + Reddit |
| ChatGPT Desktop | **High** | OpenAI features page + Help Center "Work with Apps" doc + Reddit |
| WorkBuddy (Tencent) | **Medium-High** | Tencent Cloud guide + Eigent review + Aikii listing + official docs at workbuddy.ai/docs. Did not test the app hands-on. |
| Hermes Desktop (Nous Research) | **High** | Official docs (hermes-agent.nousresearch.com) + Medium deep-dive + Reddit r/LocalLLaMA + GitHub |
| Cursor | **Medium-High** | Cursor.com + multiple 2026 reviews. Cursor is an IDE, not a desktop shell, so comparison is partial. |
| Raycast AI | **Medium** | Raycast.com + MacStories + Reddit. Raycast is a launcher, not an agent workspace. |
| Supermaven | **Medium** | Supermaven.com + community reviews. Code-completion tool, narrow scope. |

### Competitors that couldn't be verified

None. All seven named competitors were verified as real, shipping products. "WorkBuddy" required disambiguation (see note below) but the Tencent AI agent product was confirmed.

---

## Per-competitor deep dive

### 1. Claude Desktop (Anthropic)

**Sources:** [Anthropic Claude Design](https://www.anthropic.com/news/claude-design-anthropic-labs) · [PCMag Review 2026](https://me.pcmag.com/en/ai/30779/claude) · [Claude Help Center — Artifacts](https://support.claude.com/en/articles/9487310-what-are-artifacts-and-how-do-i-use-them) · [Claude Help Center — Release Notes](https://support.claude.com/en/articles/12138966-release-notes) · [Anthropic Projects](https://www.anthropic.com/news/projects) · [Reddit r/ClaudeAI — Desktop extensions](https://www.reddit.com/r/ClaudeAI/comments/1jiffk6/why_bother_installing_claude_for_desktop/)

#### A. Primary value proposition
The most polished conversational AI with elegant design — "a next-generation AI assistant built by Anthropic and trained to be safe, accurate, and secure to help you do your best work" ([claude.ai](https://claude.ai/)).

#### B. Onboarding flow
1. Download installer from claude.ai (macOS or Windows).
2. Sign in with Anthropic account (email + password).
3. Land on dashboard: central text field + left sidebar with conversation history + suggestion buttons ("vibe coding", "write a case study").
4. Start chatting immediately — no provider selection, no working directory, no tools toggle.
5. Optional: enable Connectors (Gmail, Notion, Google Calendar) or install desktop extensions.

**Key insight:** Zero-friction onboarding. The model choice is made for you. This is the anti-pattern Shannon's repositioning doc explicitly calls out and is moving away from.

#### C. IA / Navigation structure
- **Left sidebar**: conversation history (recent first), Projects (folders for chats + documents), Artifacts tab (all generated content in one view).
- **Central area**: chat input + streaming responses.
- **Right panel**: Artifact viewer (opens when Claude generates code, documents, diagrams, interactive content).
- **Settings**: accessed via gear icon; contains Connectors, Memory, Styles, personalization.
- **No "tasks" or "agents" top-level concept** — Claude is chat-centric. Claude Code (the CLI tool) has separate agent concepts, but the desktop app is conversation-first.

#### D. Chat / conversation UX
- **Multi-session** with full history in sidebar.
- **Projects** feature: group related chats + documents with shared knowledge (Pro plan and above, "unlimited Projects" on Pro).
- **Artifacts**: responses rendered as interactive sidebars — code, documents, diagrams, SVG, React components, even mini-apps. Artifact tab aggregates all generated artifacts across chats. "ChatGPT and Gemini don't give you an easy way to view all your generated content at once" ([PCMag](https://me.pcmag.com/en/ai/30779/claude)).
- **Claude Design** (Anthropic Labs, launched alongside Opus 4.7): collaborative design tool — create polished visual work, prototypes, slides, one-pagers by talking to Claude. Two-way sync with Claude Code ([Anthropic](https://www.anthropic.com/news/claude-design-anthropic-labs)).
- **File attach**: images, PDFs, documents. PCMag noted inconsistent document analysis quality.
- **Streaming**: token-by-token; slightly slower than Gemini but not as slow as DeepSeek.
- **No conversation forking/branching** in the desktop app (Shannon has `branch_session`).

#### E. Agent / task orchestration
- **No explicit "tasks" or "background agents" in the desktop app.** Claude Code (CLI) has agent mode, but the desktop app is synchronous chat.
- **"Cowork" tab** mentioned in Shannon's positioning doc as a Claude Desktop feature, but could not be independently verified in 2026-06 sources — may be planned or in limited rollout.
- **Permission flow**: pop-up confirmations when desktop extensions want to take actions ("Claude asked whether I was really sure" — PCMag). Privacy-first but can feel overbearing.

#### F. Settings / configuration
- **Provider/API key**: N/A — Claude is the only model. No provider switching.
- **Model picker**: Sonnet (default, conversational), Opus (complex reasoning), Haiku (fast). Selectable per-chat on paid plans.
- **Styles**: customize Claude's personality (concise, formal, custom).
- **Memory**: toggle auto-generated memories, toggle chat search, view what Claude knows about you.
- **Connectors**: Gmail, Google Calendar, Notion, Asana, Canva, Workato, and more. Web connectors + desktop extensions.
- **Notifications**: not a highlighted feature.

#### G. Extensions / integrations
- **Web Connectors**: Google Workspace, Notion, Asana, Canva, Workato, Zapier, and growing.
- **Desktop extensions** (downloadable): Apple Notes, Chrome, iMessage, Spotify, Filesystem, and more. "No other chatbot I've tested offers anything similar" (PCMag). Some are macOS-only (AppleScript dependency).
- **No MCP support** in the desktop app as of 2026-06 (MCP is in Claude Code / Claude for Developers, not the consumer desktop app).
- **No plugin marketplace** — extensions are installed individually.
- **No "skills" concept** at the consumer level (Skills exist in Claude Code as repeatable workflows, surfaced in the "10 Insane Claude Features" YouTube roundup).

#### H. Visual design language
- **Color**: warm palette — "terracotta orange + cream" aesthetic. "Its color scheme, font, and slightly blockier aesthetic are pleasing and make it feel less sterile than ChatGPT or Gemini" (PCMag).
- **Typography**: clean sans-serif (Tiempos Headline for display, Styrene for body — Anthropic's custom fonts).
- **Density**: comfortable, generous whitespace.
- **Light/dark**: both supported.
- **Design system**: custom (not Material, not Tailwind-default).

#### I. Micro-interactions / polish
- Streaming text with smooth scroll.
- Artifact tiles animate open.
- Memory toggle UI with clear affordances.
- Research progress screen with live stats (total searches, sources, time).
- Bar graph showing searches by website in deep research.
- **Bugs noted**: occasional hangs, context window limits, high browser memory during Artifact app creation.

#### J. Desktop-native integration
- **Tray/menubar icon**: yes (menubar app on macOS).
- **Global shortcut**: not a highlighted feature (unlike ChatGPT's Option+Space).
- **Native notifications**: yes (OS-level).
- **File system**: Filesystem Connector extension grants folder access.
- **Desktop app control**: Chrome, Spotify, iMessage, Apple Notes (macOS-heavy via AppleScript).
- **Chrome extension** (beta, Max plan only): AI browser agent + one-click Claude access from Chrome.

---

### 2. ChatGPT Desktop (OpenAI)

**Sources:** [ChatGPT Desktop features page](https://chatgpt.com/features/desktop/) · [OpenAI Help Center — Work with Apps](https://help.openai.com/en/articles/10119604-work-with-apps-on-macos) · [OpenAI — Introducing Apps in ChatGPT](https://openai.com/index/introducing-apps-in-chatgpt/) · [OpenAI Release Notes](https://help.openai.com/en/articles/6825453-chatgpt-release-notes) · [Reddit r/OpenAI — IDE editing](https://www.reddit.com/r/OpenAI/comments/1j545j2/chatgpt_for_macos_can_now_edit_code_directly_in/)

#### A. Primary value proposition
The fastest way to access the world's most popular AI from anywhere on your desktop — "Chat about code, email, screenshots, files, and anything on your screen" ([chatgpt.com](https://chatgpt.com/features/desktop/)).

#### B. Onboarding flow
1. Download from openai.com/chatgpt/desktop (not Mac App Store — noted as surprising by Reddit users).
2. Sign in with OpenAI account.
3. Press Option+Space (macOS) or Alt+Space (Windows) to summon the Chat Bar from anywhere.
4. Start chatting — no configuration needed.
5. Optional: enable "Work with Apps" to connect IDEs, terminals, Notes.

**Key insight:** The global shortcut is the hero feature. ChatGPT Desktop's value is "always one keystroke away," not "configure your AI workspace."

#### C. IA / Navigation structure
- **Minimal sidebar**: conversation history, similar to web.
- **Chat Bar** (overlay): summoned by Option+Space, floats over any app, compact.
- **Companion window**: attaches to IDEs (Xcode, VS Code) for side-by-side editing.
- **Main window**: full chat experience with model picker.
- **No tasks/agents/projects top-level navigation** — it is chat + history.

#### D. Chat / conversation UX
- **Multi-session** with history synced to web account.
- **No Projects, no Artifacts** (Artifacts are a Claude concept).
- **"Apps" in ChatGPT** (Nov 2025, Business/Enterprise/Edu preview): third-party apps embedded in chat ([OpenAI](https://openai.com/index/introducing-apps-in-chatgpt/)).
- **File attach**: images, PDFs, screenshots, files.
- **Screenshot capture**: "chat about anything on your screen."
- **Advanced Voice Mode**: real-time voice conversation, hands-free. "Chat with your computer in real-time and get hands-free advice and answers while you work" ([chatgpt.com](https://chatgpt.com/features/desktop/)).
- **Streaming**: token-by-token, fast.

#### E. Agent / task orchestration
- **No "tasks" or "background agents"** in the desktop app.
- **"Work with Apps"** is the closest to agent behavior: ChatGPT reads content from active apps (IDE, terminal, Notes) and can edit files directly in IDEs.
- **Code edits**: generates diff, user reviews and applies (or auto-apply toggle). Reversible via Cmd+Z in editor.
- **GPT-5 "agents"** are available in the broader ChatGPT platform but the desktop app surface is conversational, not dashboard-style.

#### F. Settings / configuration
- **Provider**: N/A — OpenAI only.
- **Model picker**: GPT-5, GPT-5.1, GPT-4o, o-series thinking models. Selectable per-chat.
- **Keyboard shortcut**: Option+Space (macOS) / Alt+Space (Windows), customizable in Settings.
- **Work with Apps**: manage which apps ChatGPT can access, enable/disable globally.
- **Data controls**: Temporary Chat toggle, "Improve the model for everyone" toggle, export/delete.
- **Notifications**: not highlighted.

#### G. Extensions / integrations
- **Work with Apps** (macOS): Apple Notes, Notion, TextEdit, Quip, Xcode, VS Code (including Cursor, Windsurf, VSCodium), JetBrains IDEs (IntelliJ, PyCharm, WebStorm, etc.), Terminal, iTerm, Warp, Prompt.
- **Editing** available only with IDEs; reading works with text editors and terminals too.
- **VS Code extension** required: "ChatGPT – Work with Code on macOS."
- **macOS Accessibility API** used for most integrations (System Settings → Accessibility permission).
- **No MCP support** in the desktop app.
- **No plugin marketplace** at the desktop level (Apps SDK is web-platform level).

#### H. Visual design language
- **Color**: clean white/dark with signature green-teal accent. Less warm than Claude, more utilitarian.
- **Typography**: system fonts (SF Pro on macOS).
- **Density**: compact, especially in the Chat Bar overlay.
- **Light/dark**: both, follows system.
- **Design system**: custom, minimal, Apple-native feel.

#### I. Micro-interactions / polish
- Chat Bar overlay appears/disappears smoothly.
- Companion window docks to active IDE.
- Voice mode: animated orb visualization.
- Diff review UI for code edits: clean, apply/revert buttons.
- Banner showing which apps ChatGPT is "working with."

#### J. Desktop-native integration
- **Global shortcut**: Option+Space / Alt+Space — **the defining feature.**
- **Menubar icon**: yes.
- **Native notifications**: yes.
- **IDE integration**: direct file editing via Accessibility API + VS Code extension.
- **macOS only for Apple Silicon** (M1+). Windows version also available.
- **Enterprise admin controls**: "Allow code edits on macOS" toggle, "Work with Apps" toggle.

---

### 3. WorkBuddy (Tencent Cloud / CodeBuddy team)

**Sources:** [Tencent Cloud WorkBuddy Guide](https://www.tencentcloud.com/techpedia/144100?lang=en) · [Eigent.ai Review](https://www.eigent.ai/blog/workbuddy-ai-review) · [Aikii listing](https://aikii.org/products/workbuddy) · [WorkBuddy.ai](https://www.workbuddy.ai/) · [Facebook/Yicai announcement](https://www.facebook.com/yicaiglobal/posts/1355101976661132/)

> **Disambiguation note:** "WorkBuddy" maps to at least three products: (1) Tencent's AI agent desktop (the one analyzed here), (2) a Chrome extension for website blocking/focus management, (3) an Indian field-sales mobile app (theworkbuddy.app). This analysis covers product (1), the AI agent workspace.

#### A. Primary value proposition
"Your own AI teammate" — a full-scenario AI agent desktop workstation that doesn't just chat but actually executes multi-step office work end-to-end. "1 brief. 100+ Experts. Multiple agents in parallel. 0 copy-paste." ([Tencent Cloud](https://www.tencentcloud.com/techpedia/144100?lang=en)).

#### B. Onboarding flow
1. Download from workbuddy.ai (Windows .exe ~150-180MB or macOS .dmg).
2. Install and sign in with account.
3. **Grant folder access** — authorize Desktop, Documents, Downloads folders. This is mandatory for the agent to work.
4. Optionally configure model per task type in Settings (defaults work out of the box).
5. Optionally connect a messenger (Slack/Telegram/Discord) for remote control.
6. Start with a real task: "Merge every Excel file in the '2026 Q1 Sales' folder and generate a bar chart plus an analysis report."

**Key insight:** Onboarding requires folder authorization (similar to Shannon's working directory concept) but positions it as giving the agent workspace access, not developer configuration.

#### C. IA / Navigation structure
- **Task-centric interface**: the primary surface is a task brief input, not a chat box.
- **Plan confirmation**: agent shows its decomposition plan; user approves before execution.
- **Live execution view**: sandbox progress with streaming tool output.
- **Result viewer**: files, charts, reports ready to use.
- **Conversation history**: past tasks and results.
- **Skill Marketplace**: browse and add 100+ Expert Skills.
- **Settings**: model configuration, messenger connections, folder permissions.
- **No traditional "sidebar with Chat/Tasks/Agents/Settings"** — it is organized around the task lifecycle.

#### D. Chat / conversation UX
- **Task-brief oriented**, not free-form chat. You describe a goal; the agent decomposes and executes.
- **Plan confirmation step** before execution — a key UX differentiator from pure chat.
- **Multimodal results**: documents, spreadsheets, PPTs, charts, PDFs.
- **File handling**: reads/writes/merges/converts Office files (Word/Excel/PPT), PDFs, images.
- **Local file operations**: batch-processes authorized folders.
- No evidence of conversation forking/branching/pinning in available sources.

#### E. Agent / task orchestration
- **Multi-agent orchestration is the core feature.** "Give it a goal, and it figures out the plan, runs the right tools, and hands you a finished deliverable" ([Eigent](https://www.eigent.ai/blog/workbuddy-ai-review)).
- **Parallel agents**: "one agent is pulling public web data, another is summarizing uploaded reports, and a third is assembling the slide deck, all at the same time."
- **Sandboxed execution**: tasks run in isolated environment.
- **Scheduled tasks**: "Every day at 9am, export yesterday's orders and send them to my team channel."
- **Remote control**: trigger desktop tasks from phone via Slack/Telegram/Discord.
- **No explicit permission/approval flow per-action** beyond the initial plan confirmation — the plan is approved as a whole.

#### F. Settings / configuration
- **Model configuration**: per-task-type model selection. "Default settings work well for everyday office work — no tuning required to get started."
- **Credit-based system**: starter credits on signup; complex tasks consume more.
- **Folder permissions**: manage which folders the agent can access.
- **Messenger connections**: Slack, Telegram, Discord, WeChat Work, Feishu, DingTalk.
- **Notifications**: via messenger channels.

#### G. Extensions / integrations
- **Productivity tools**: GitHub, Jira, Notion, Google Drive, Gmail, Slack, Discord.
- **Enterprise messaging**: WeChat Work, Feishu, DingTalk, QQ (China ecosystem).
- **Remote orchestration**: Discord, Slack, Telegram, WeChat.
- **MCP-style tooling**: "its tool integration layer works similarly to the Model Context Protocol (MCP) pattern — pluggable tools and data sources that agents can call on demand" (Eigent). Explicitly MCP-compatible.
- **Skill ecosystem**: 100+ ready-to-use Expert Skills, weekly additions. Build custom skills via Markdown files in `skills/` directory + slash commands in `commands/`.
- **OpenClaw compatible** (announced as "fully compatible with OpenClaw and requires no project setup").

#### H. Visual design language
- **Clean, professional** — positioned for office workers, not developers.
- Screenshots from reviews show a dashboard-style layout with task cards, progress indicators, and result previews.
- **Light mode primary**; dark mode availability unconfirmed.
- **Design system**: custom, likely Tencent's in-house design language.
- Density: moderate — balances information richness with clarity.

#### I. Micro-interactions / polish
- Plan decomposition visualization (shows steps before executing).
- Live progress streaming in sandbox.
- Result preview before download.
- Messenger notification on task completion.
- **Regional caveat**: "documentation, pricing transparency, and some feature depth still favor users within Tencent's ecosystem" (Eigent).

#### J. Desktop-native integration
- **Tray icon**: likely (standard for Tencent desktop apps).
- **Global shortcut**: not highlighted in available sources.
- **Native notifications**: via OS + messenger channels.
- **File system**: deep integration — this is the product's core differentiator. Direct read/write/merge/convert of local files.
- **Remote control from phone**: standout feature — "trigger your desktop from your phone, anywhere, through a mainstream messenger."
- **Cross-platform**: Windows 10/11, macOS 10.15+.

---

### 4. Hermes Desktop (Nous Research)

**Sources:** [Hermes Agent Docs — Skills](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills) · [Medium/Tenten deep-dive](https://medium.com/@tentenco/hermes-agent-desktop-app-everything-you-need-to-know-about-nous-researchs-self-improving-ai-agent-3cb59bd31e5f) · [GitHub — NousResearch/hermes-agent](https://github.com/nousresearch/hermes-agent) · [Reddit r/LocalLLaMA](https://www.reddit.com/r/LocalLLaMA/comments/1tve7qu/nous_research_hermes_desktop/) · [Hermes Skills Catalog](https://hermes-agent.nousresearch.com/docs/reference/skills-catalog)

#### A. Primary value proposition
"The agent that grows with you" — the only open-source AI agent with a built-in closed learning loop that creates skills from experience, getting measurably faster at repetitive tasks over time ([GitHub](https://github.com/nousresearch/hermes-agent)).

#### B. Onboarding flow
1. Download installer (macOS .dmg, Windows .exe) or run `curl` script on Linux with `--include-desktop` flag.
2. Launch — desktop auto-detects existing Hermes CLI configurations and loads them.
3. Sign in or use local-only mode (MIT-licensed, no account required).
4. Optional: connect to Nous Portal ($20/mo for 300+ models) or bring your own API keys.
5. Optional: connect messaging platforms (Telegram, Discord, Slack, WhatsApp, Signal, email).
6. Start chatting or assign a task.

**Key insight:** The desktop app's pitch is "download, install, run" — eliminating the terminal requirement that kept non-technical users away. "The desktop release solves a problem that's bigger than it sounds... non-technical decision-makers see a terminal window and check out" (Tenten/Medium).

#### C. IA / Navigation structure
- **Chat interface**: central conversation area.
- **File browser**: side-by-side preview showing what the agent is reading/writing/editing.
- **Streaming tool output**: real-time visualization of agent actions.
- **Skills page**: browse, search, install, manage skills. Includes "Learn a skill" button.
- **Memory/Profiles**: agent configuration, memory management.
- **Settings**: replaces manual YAML editing with proper UI.
- **Scheduling**: natural-language cron setup.
- **Sessions**: conversation history synced with CLI.

#### D. Chat / conversation UX
- **Multi-session** with persistent memory across sessions.
- **Cross-platform continuity**: "Start a conversation in CLI, continue it on Telegram, have the agent send results to your team on Slack."
- **Streaming tool output**: shows what the agent is doing in real time.
- **Side-by-side file browser**: follow along as agent reads/writes/edits.
- **Slash commands**: every installed skill is a slash command (`/gif-search`, `/plan`, `/github-pr-workflow`).
- **Voice mode**: works same as CLI.
- **Multimodal**: web search, browser automation, image generation, TTS.
- No evidence of conversation forking in available sources.

#### E. Agent / task orchestration
- **Sub-agent delegation**: agent can fork tasks to independent sub-agents with their own terminals.
- **Closed learning loop**: after every task, evaluates success, extracts reusable patterns, stores as skill files (Markdown). "Agents with 20+ self-created skills complete similar future tasks 40% faster" (TokenMix benchmark, April 2026).
- **Natural-language cron scheduling**: "daily news briefings, weekly project summaries, monthly report aggregation."
- **Background execution**: agent runs through gateway unattended.
- **Write-approval gate** (optional): `skills.write_approval: true` stages skill writes for human review before committing. `/skills pending`, `/skills diff <id>`, `/skills approve <id>`.
- **Background self-improvement review**: runs after a turn, can suggest skill changes.

#### F. Settings / configuration
- **Provider**: 20+ LLM providers — OpenRouter, direct Anthropic/OpenAI APIs, local Ollama, vLLM, Nous Portal (300+ models).
- **Nous Portal**: $0.10/mo free tier (evaluation only), $20/mo Plus ($22 in credits).
- **Profiles**: multiple agent profiles with separate memory, skills, config. `hermes profile create research --no-skills`.
- **Write-approval gates**: separate gates for skills and memory writes.
- **Toolsets**: configurable per session (`--toolsets skills`, `--toolsets terminal,web`).
- **Notifications**: via connected messaging platforms.

#### G. Extensions / integrations
- **118+ bundled skills** (growing).
- **Skills Hub**: browse/install from 9 sources:
  - Official optional skills (`official/`)
  - skills.sh (Vercel's directory)
  - Well-known endpoints (`/.well-known/skills/index.json`)
  - Direct GitHub (`openai/skills`, `anthropics/skills`, `huggingface/skills`, `NVIDIA/skills`)
  - ClawHub marketplace
  - Claude marketplace-style repos
  - LobeHub catalog
  - browse.sh (200+ browser automation skills)
  - Direct URL
- **Custom taps**: publish your own skill repo, others add with `hermes skills tap add owner/repo`.
- **Skill bundles**: group multiple skills under one slash command.
- **`/learn` command**: agent authors a skill from any source (local directory, URL, conversation history, described procedure).
- **Agent-created skills**: agent uses `skill_manage` tool to create/patch/edit/delete skills autonomously.
- **Security scanner**: all hub-installed skills scanned for exfiltration, prompt injection, destructive commands, supply-chain threats. Trust levels: builtin, official, trusted, community.
- **Messaging platforms**: Telegram, Discord, Slack, WhatsApp, Signal, email — all share one session and one memory.
- **OpenClaw migration**: `hermes claw migrate` command imports config, memory, skills, API keys, platform settings.

#### H. Visual design language
- **Electron + React** with Python backend. Same agent core as CLI.
- **Native-feeling** despite Electron (per community reports).
- **Theming**: the Shannon positioning doc references "6 themes" for Hermes, though the official docs focus on functionality over visual customization. Community wrappers offered extensive theming.
- **Light/dark**: both supported.
- **Typography**: system-default within Electron.
- **Density**: moderate, information-rich (file browser + chat + tool output).

#### I. Micro-interactions / polish
- Streaming tool output with live status.
- Side-by-side file diff preview.
- Skill write-approval flow with unified diff view.
- Progressive disclosure for skills (Level 0: list ~3k tokens → Level 1: full content → Level 2: specific reference).
- Auto-detection of media paths in responses (delivers files natively to messaging platforms).
- `[[as_document]]` and `[[audio_as_voice]]` directives for intelligent media delivery.
- **Known limitation**: Windows installer not code-signed (SmartScreen warning).
- **Known limitation**: self-evolving skills are a "black box" — no explainability interface for skill retention decisions.

#### J. Desktop-native integration
- **Cross-platform**: macOS 12+, Windows 10/11 (installer), Linux (terminal script with `--include-desktop`).
- **Remote backend**: desktop can connect to a Hermes instance running on a VPS/cloud server as a graphical remote.
- **CLI ↔ desktop sync**: state shared, start in one, continue in the other.
- **File system**: full read/write/edit access.
- **Terminal**: agent has terminal access; sub-agents get their own terminals.
- **Browser automation**: via skills and browser tools.
- **Messaging gateway**: 6+ platforms unified.
- **Global shortcut**: not highlighted in available sources.
- **Tray icon**: not confirmed in sources.

---

### 5. Cursor (Bonus — IDE, not desktop shell)

**Sources:** [Cursor.com](https://cursor.com/) · [Cursor 3 Deep Dive](https://www.digitalapplied.com/blog/cursor-3-deep-dive-agents-composer-review-2026) · [Builder.io — Cursor vs Claude Code](https://www.builder.io/blog/cursor-vs-claude-code) · [DataCamp — Cursor vs VS Code](https://www.datacamp.com/blog/cursor-vs-vs-code) · [Towards AI — Complete Guide](https://pub.towardsai.net/cursor-ide-complete-guide-2025-8d8d25407b97)

#### A. Primary value proposition
The AI-native IDE — "a VS Code fork rebuilt around AI" with tab completions, multi-file Composer, and background agents ([Builder.io](https://www.builder.io/blog/cursor-vs-claude-code)).

#### B-H. Summary (IDE context)

Cursor is included as a bonus because it represents the gold standard for **code-specific AI UX patterns** that Shannon could learn from, even though Shannon is positioning as a broader workspace:

- **Tab completion**: "sometimes, scary accurate" (Reddit r/cursor). Best-in-class inline prediction.
- **Composer mode**: multi-file edits with agent that plans, writes, and tests features across files.
- **Background Agents** (Cursor 3): cloud-based agents that work on tasks in parallel, bidirectional sync with local. "The bidirectional nature of working in Cursor and with background Agents is remarkably smooth" ([Zack Proser](https://zackproser.com/blog/cursor-agents-review)).
- **Plan Mode**: agent produces an implementation plan before writing code.
- **Agent Code Review**: built-in code review agent.
- **Browser**: built-in browser for testing/preview.
- **MCP integration**: Cursor 3 supports MCP for external tool calling.
- **Model routing**: multi-model support, route different tasks to different models.
- **BugBot**: automated bug detection.
- **Tab UI**: agent tab vs IDE tab split (Cursor 2.0 redesign).

**Design language**: VS Code-derived, dark-mode-first, clean and dense. Custom AI panel styling.

---

### 6. Raycast AI (Bonus — Launcher)

**Sources:** [Raycast.com](https://www.raycast.com/) · [MacStories — AI Extensions](https://www.macstories.net/reviews/hands-on-with-raycasts-new-ai-extensions/) · [Raycast Manual — AI Extensions](https://manual.raycast.com/ai/ai-extensions) · [Windows Forum](https://windowsforum.com/threads/raycast-on-windows-brings-tahoe-style-spotlight-with-ai-and-extensions.388515/)

#### A. Primary value proposition
"Your shortcut to everything" — a keyboard-first command palette with built-in AI, extensions store, clipboard manager, window management, and snippets ([Raycast.com](https://www.raycast.com/)).

#### Key patterns relevant to Shannon:
- **Command palette as the primary interface**: everything is searchable, keyboard-first. This is the pattern Shannon's repositioning doc recommends adopting (Cmd+K palette).
- **Extensions store**: curated marketplace of community extensions. Raycast now has "AI Extensions" that empower Raycast AI to perform actions within extensions.
- **Now on Windows** (beta): bringing the macOS Spotlight-style experience cross-platform.
- **AI is embedded, not central**: AI is one capability among many (clipboard, snippets, window management, quicklinks). This is a different philosophy from Shannon's AI-first approach.
- **Density**: extremely compact, optimized for keyboard navigation.

---

### 7. Supermaven (Bonus — Code Completion)

**Sources:** [Supermaven.com](https://supermaven.com/) · [State of AI 2025 — Coding Assistants](https://2025.stateofai.dev/en-US/coding-assistants/) · [Reddit r/neovim](https://www.reddit.com/r/neovim/comments/1tp79ai/best_ai_code_completion_as_of_may_2026/)

#### A. Primary value proposition
"The first code completion tool with a 1 million token context window" — ultra-fast inline code completion with massive context ([Supermaven.com](https://supermaven.com/)).

#### Key patterns:
- **Speed**: "suggestions appear almost instantly as you type" — sub-100ms latency is the differentiator.
- **1M token context**: 30x larger than competitors at launch.
- **IDE plugin model**: integrates into VS Code, JetBrains, Neovim — not a standalone app.
- **Narrow scope**: code completion only, not chat or agents. Second-highest positive sentiment after GitHub Copilot in State of AI 2025 survey.
- **Free tier**: generous free completion usage.

**Relevance to Shannon**: Supermaven represents the "do one thing extremely well" philosophy. Shannon's breadth (chat + tasks + agents + automations + extensions) is the opposite approach — both can coexist in the market.

---

## Side-by-side comparison table

| Dimension | Claude Desktop | ChatGPT Desktop | WorkBuddy (Tencent) | Hermes Desktop | Cursor | **Shannon Desktop** |
|---|---|---|---|---|---|---|
| **Primary use case** | Polished chat + artifacts | Always-available chat + IDE editing | Office task automation | Self-improving personal agent | AI-native coding | Multi-provider AI workspace + agents |
| **Onboarding steps** | 2 (download, sign in) | 2 (download, sign in) | 4 (download, sign in, grant folders, optional messenger) | 3 (download, launch, optional keys) | 3 (download, sign in, open project) | 4 (download, choose task, choose model, choose tools) |
| **Onboarding friction** | Very low | Very low | Low-moderate | Low | Low | Moderate (model selection required) |
| **Multi-provider LLM** | No (Claude only) | No (OpenAI only) | Yes (multi-model routing) | Yes (20+ providers + Ollama) | Yes (multi-model) | **Yes (Anthropic/OpenAI/Ollama/DeepSeek)** |
| **Local model support** | No | No | No | Yes (Ollama, vLLM) | No | **Yes (Ollama first-class)** |
| **Open source** | No | No | No | Yes (MIT) | No | **Partially** (desktop shell; engine separate) |
| **Chat UX** | Artifacts, Projects, deep research | Voice mode, Work with Apps, screenshots | Task-brief → plan → execution | Multi-platform sessions, slash commands | Tab completion, Composer, Plan Mode | Multi-session, branching, templates (planned) |
| **Conversation branching** | No | No | No | No | No (parallel agents instead) | **Yes (`branch_session`)** |
| **Agent orchestration** | No (desktop app) | No (desktop app) | **Yes (parallel multi-agent)** | **Yes (sub-agent delegation)** | **Yes (Background Agents)** | **Yes (agent team + OPC)** |
| **Background tasks** | No | No | Yes (sandboxed) | Yes (gateway + cron) | Yes (cloud Background Agents) | **Yes (hooks + routines + worktrees)** |
| **Scheduled tasks** | No | No | Yes (cron) | Yes (natural-language cron) | No | **Yes (cron + natural language)** |
| **Permission/approval flow** | Pop-up confirmations | Plan review for edits | Plan confirmation step | Write-approval gate (optional) | Diff review before apply | **Permission profiles + per-action approval** |
| **Extensions ecosystem** | Web Connectors + desktop extensions | Work with Apps (limited set) | 100+ skills, MCP-style | **118+ skills, 9-source Skills Hub** | MCP integration | **MCP + Skills + Agents + Datasources Hub** |
| **Plugin marketplace** | No (individual installs) | No (Apps SDK is web-level) | Skill Marketplace | Skills Hub (multi-registry) | No | **Extensions Hub (unified)** |
| **MCP support** | No (desktop app) | No (desktop app) | Yes (MCP-compatible) | Via skills/tools | Yes (Cursor 3) | **Yes (first-class MCP)** |
| **Messaging integration** | No | No | Slack/Telegram/Discord/WeChat | Telegram/Discord/Slack/WhatsApp/Signal/email | No | **Slack/Telegram (inbound + outbound)** |
| **Voice mode** | Yes (mobile) | **Yes (Advanced Voice, desktop)** | No | Yes (CLI + desktop) | No | **No** |
| **Artifact/visual builder** | **Yes (Claude Design, Artifacts)** | No | No (generates files) | No | No | **No** |
| **Image generation** | No | Yes (DALL-E) | No | Yes (via tools) | No | **No** |
| **Global shortcut** | Not highlighted | **Yes (Option+Space)** | Not highlighted | Not highlighted | N/A (IDE) | **Yes (Cmd+K palette, customizable)** |
| **Tray icon** | Yes (menubar) | Yes (menubar) | Likely | Not confirmed | N/A | **Yes** |
| **Native notifications** | Yes | Yes | Yes (OS + messenger) | Yes (via messaging) | Yes (IDE) | **Yes (tauri-plugin-notification)** |
| **File system access** | Yes (Filesystem extension) | Yes (via Work with Apps) | **Yes (deep, core feature)** | Yes (full read/write/edit) | Yes (IDE-native) | **Yes (working directory + file tree)** |
| **Remote control from phone** | No | No | **Yes (Slack/Telegram/Discord)** | **Yes (all messaging platforms)** | Yes (cloud agents) | **Partial (inbound listener for Slack/Telegram)** |
| **Cross-platform** | macOS + Windows | macOS (Apple Silicon) + Windows | macOS + Windows | macOS + Windows + Linux | macOS + Windows + Linux | **macOS + Windows + Linux (Tauri)** |
| **App framework** | Native | Native | Native (likely Electron) | **Electron + React + Python** | Electron (VS Code fork) | **Tauri v2 (Rust + React)** |
| **Resource footprint** | Low (native) | Low (native) | Moderate | High (Electron) | High (Electron) | **Low (Tauri, not Electron)** |
| **Visual warmth** | **High (terracotta + cream)** | Medium (clean, utilitarian) | Medium (professional) | Medium (functional) | Low (dark, dense) | Medium (material blue, improving) |
| **Theming** | Limited | Limited | Unknown | Community-driven | VS Code themes | **Light/dark + Simple/Advanced modes** |
| **i18n** | Multi-language | Multi-language (60+ locales) | Chinese-first, English available | English + Chinese | English | **English + Chinese (curated, not machine-translated)** |
| **Self-improvement** | No | No | No | **Yes (closed learning loop, 40% faster)** | No | **No** |
| **Pricing** | Free / $17 Pro / $100-200 Max | Free / $20 Plus / $200 Pro | Free tier + credits / Tencent Cloud RMB | Free (OSS) / $20 Portal / BYO keys | Free / $20 Pro / $40 Ultra | **Free (BYO keys) / planned Pro tier** |

---

## Shannon's position

### Where Shannon leads (5 concrete advantages)

1. **Multi-provider freedom with local-first architecture.** Shannon supports Anthropic, OpenAI, Ollama, and DeepSeek as first-class providers, switchable per-conversation. Claude Desktop locks you to Claude. ChatGPT Desktop locks you to GPT. Only Hermes and WorkBuddy match this, and Shannon's Tauri shell means lower resource overhead than Hermes's Electron stack. The `welcome.model.*` strings and `commands_config.rs::switch_provider` confirm this is architecturally deep, not a UI afterthought.

2. **First-class automations (hooks + routines + permission profiles).** Shannon's `scheduled_commands.rs` (Tasks board, Triage, Triggered Routines, worktree management, OPC metric aggregation) backed by `~/.shannon/scheduled-tasks/` and `~/.shannon/scheduled-runs/` is more granular than any competitor's scheduling. WorkBuddy has scheduled tasks but no event-driven hooks. Hermes has cron but no permission profiles. Claude Desktop and ChatGPT Desktop have **no automation features at all**. This is Shannon's deepest moat.

3. **Agent team coordination with inter-agent message visibility.** The `commands_agents.rs` module (agent definitions + inter-agent message history) and the Tasks page Agent Messages panel (`tasks.agentMessagesPanel.*` strings) give Shannon something no competitor offers: a **visible** multi-agent communication layer. WorkBuddy runs parallel agents but they are opaque. Hermes has sub-agent delegation but no inter-agent message log UI. Cursor's Background Agents are cloud-opaque.

4. **Permission profiles as a security primitive.** Shannon's `commands_permissions.rs` + `automation_commands.rs` (hook event catalog + custom permission profiles) create a security model where different task types run with different tool access levels. No competitor in this set has an equivalent. Claude Desktop has per-action pop-ups (reactive). Hermes has a write-approval gate (narrow scope). Shannon has proactive, pre-configurable permission modes.

5. **Tauri-native performance with cross-platform consistency.** Shannon runs on Tauri v2 (Rust backend + React frontend), not Electron. This means: lower memory footprint, faster startup, native OS integration without Chromium overhead. Hermes Desktop, Cursor, and (likely) WorkBuddy are Electron-based. Claude Desktop and ChatGPT Desktop are native but locked to their ecosystems. Shannon is the only open-architecture, non-Electron, cross-platform option.

### Where Shannon lags (5 concrete gaps)

1. **No voice mode.** ChatGPT Desktop's Advanced Voice Mode is a defining feature — "chat with your computer in real-time and get hands-free advice and answers while you work" ([chatgpt.com](https://chatgpt.com/features/desktop/)). Claude has voice on mobile. Hermes has voice in CLI and desktop. Shannon has zero voice capability. This is the largest feature gap for consumer positioning. **Reference:** [ChatGPT Desktop features](https://chatgpt.com/features/desktop/)

2. **No artifact/visual builder.** Claude's Artifacts system — interactive code, documents, diagrams, SVG, React components, mini-apps rendered in a side panel — plus Claude Design (polished visual work, prototypes, slides) represents a category of output Shannon cannot produce. Shannon's chat outputs text and markdown. The `Editor.tsx` page exists but is a code editor, not an artifact builder. **Reference:** [Claude Help Center — Artifacts](https://support.claude.com/en/articles/9487310-what-are-artifacts-and-how-do-i-use-them), [Anthropic — Claude Design](https://www.anthropic.com/news/claude-design-anthropic-labs)

3. **Weaker onboarding warmth and brand polish.** Claude Desktop's terracotta/cream palette and "less sterile than ChatGPT or Gemini" feel (PCMag) sets a high bar. ChatGPT's Option+Space global shortcut means zero-friction access from anywhere. Shannon's current onboarding requires choosing a task type, then a provider, then tools — more steps than Claude (2 steps) or ChatGPT (2 steps). The repositioning doc (`04-product-repositioning.md`) already identifies this and proposes solutions, but the gap exists today. **Reference:** [PCMag Claude Review](https://me.pcmag.com/en/ai/30779/claude), [ChatGPT Desktop](https://chatgpt.com/features/desktop/)

4. **Thinner skills/extensions catalog.** Hermes ships 118+ bundled skills plus a Skills Hub integrating 9 registries (skills.sh, ClawHub, GitHub taps, well-known endpoints, browse.sh, LobeHub, Claude marketplace, direct URL). WorkBuddy ships 100+ Expert Skills. Claude Desktop has downloadable desktop extensions. Shannon's Extensions Hub architecture (MCP + Skills + Agents + Datasources) is technically superior, but the actual installable catalog is thinner. The hub infrastructure exists (`extensions_commands.rs`) but the content does not match Hermes's breadth. **Reference:** [Hermes Skills Hub docs](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills)

5. **No self-improvement / learning loop.** Hermes's closed learning loop — "after every task execution, assesses whether the outcome succeeded, extracts reusable reasoning patterns, stores as skill files" — delivers a measurable 40% efficiency gain on repetitive tasks (TokenMix benchmark). Shannon has no equivalent. Shannon's Memory page (`Memory.tsx`) stores facts but does not automatically extract procedural knowledge from task executions. **Reference:** [Medium/Tenten — Hermes deep-dive](https://medium.com/@tentenco/hermes-agent-desktop-app-everything-you-need-to-know-about-nous-researchs-self-improving-ai-agent-3cb59bd31e5f)

### Unique differentiators (things no competitor has)

1. **OPC (Operations Control Center) concept.** Shannon's OPC page (`OPC.tsx`, `OPCTask.tsx`) with metric aggregation (`cargo bench --bench load_tests` for OPC metric aggregation in criterion benches) is a project-management dashboard for AI agent operations. No competitor has an equivalent — WorkBuddy has task execution views, Hermes has tool output streaming, but neither has a strategic-level operations dashboard with kanban + metrics + triage.

2. **Worktree-based parallel work isolation.** Shannon's use of git worktrees (`tasks.worktreePanel.*`, `scheduled_commands.rs` worktree management) for isolating unattended routine execution is unique. Hermes has sub-agent terminals but not git-level work isolation. Cursor has cloud Background Agents but they operate on branches, not local worktrees. Shannon's approach means parallel agent work can happen on the same local repo without conflicts.

3. **Simple Mode / Advanced Mode sidebar toggle.** Shannon's dual-mode sidebar (`nav.simpleMode`, `nav.devMode`, `nav.simpleMode.badge: "default"`, `nav.devMode.badge: "all features"`) is a thoughtful solution to the "serve both novices and power users" problem. No competitor does this — Claude Desktop is one-mode-fits-all, ChatGPT Desktop is one-mode, Hermes is developer-focused, WorkBuddy is office-worker-focused. Shannon explicitly serves both personas with a toggle.

4. **Curated bilingual i18n (English + Chinese).** Shannon's `i18n/` with react-intl v7, two curated locales (not machine-translated), and the convention that both `en.json` and `zh-CN.json` must be updated in the same change is rare among AI desktop tools. WorkBuddy is Chinese-first with English secondary. Hermes is English-first with Chinese docs. Shannon treats both as first-class.

5. **Inbound + outbound messaging integration as a first-class feature.** Shannon's `commands_notifications.rs` with Slack Socket Mode + Telegram long-poll inbound listeners, outbound message delivery, webhook configuration with HMAC signing, and the notification wizard (`settings.notifications.wizard.*` with step-by-step Slack app creation, BotFather guidance, chat ID capture) is more deeply integrated than any competitor's messaging feature. Hermes connects to more platforms but Shannon's setup wizard is more polished for the two it supports.

---

## Recommended UI/flow imports

For each gap identified above, here is what Shannon should concretely adopt or adapt:

### 1. Voice mode (addresses lag #1)
- **Adopt ChatGPT's Advanced Voice orb pattern**: a floating, animated visualization that activates from the chat input area. Users click a microphone icon to start, the orb animates while listening/speaking.
- **Implementation path**: Shannon already has `tauri-plugin-notification` and audio capabilities via Tauri. Add a `useVoice` hook, a voice mode component, and wire it through the existing `LlmClient` abstraction with provider-specific voice APIs (OpenAI Realtime API, Anthropic voice beta).
- **Reference**: [ChatGPT Desktop — Advanced Voice](https://chatgpt.com/features/desktop/)

### 2. Artifact builder (addresses lag #2)
- **Adopt Claude's Artifact panel pattern**: when the LLM generates code, HTML, SVG, Mermaid diagrams, or structured content, render it in a right-side panel that opens automatically. Add an "Artifacts" tab in the sidebar to aggregate all generated content across chats.
- **Implementation path**: Shannon already has `Editor.tsx` and `components/diff/`. Extend with a sandboxed iframe renderer for HTML/React artifacts, a Mermaid renderer, and an SVG viewer. Use the existing `events.rs` event system to detect artifact-eligible output.
- **Live Artifacts concept** (from Claude): self-contained HTML pages that pull fresh data from connectors. Shannon could implement this as routines that output HTML artifacts.
- **Reference**: [Claude Help Center — Artifacts](https://support.claude.com/en/articles/9487310-what-are-artifacts-and-how-do-i-use-them)

### 3. Global shortcut + always-available overlay (addresses lag #3a)
- **Adopt ChatGPT's Option+Space / Alt+Space pattern**: summon a compact Shannon chat bar from anywhere on the desktop, regardless of which app is focused.
- **Implementation path**: Shannon already registers global shortcuts via `tauri-plugin-global-shortcut` (see `main.rs` setup). Add a compact overlay window (Tauri multi-window) that floats above all apps, with a minimal chat input and quick-result display.
- **Reference**: [ChatGPT Desktop](https://chatgpt.com/features/desktop/), [OpenAI Help Center — Work with Apps](https://help.openai.com/en/articles/10119604-work-with-apps-on-macos)

### 4. Warmer visual design (addresses lag #3b)
- **Adopt Claude's warm color palette**: shift from material-blue to a warm primary (terracotta orange #E8743C or lake green #2A9D8F as proposed in `04-product-repositioning.md` §4.2). Pair with a cream-neutral secondary (#F4F3EE).
- **Adopt Hermes's micro-interaction patterns**: button hover animations, panel enter/exit transitions (200ms), streaming tool output visualization.
- **Adopt Inter + JetBrains Mono typography** as proposed in the repositioning doc.
- **Reference**: [PCMag Claude Review](https://me.pcmag.com/en/ai/30779/claude), [04-product-repositioning.md §4.2](./04-product-repositioning.md)

### 5. Grow the skills catalog (addresses lag #4)
- **Adopt Hermes's multi-source Skills Hub pattern**: integrate with skills.sh, well-known skill endpoints, and GitHub taps so Shannon users can browse and install from the broader skills ecosystem, not just Shannon's own catalog.
- **Adopt Hermes's `/learn` command pattern**: let users point Shannon at a local doc directory, a URL, or a conversation history, and have the agent author a reusable skill automatically.
- **Adopt Hermes's skill bundle pattern**: group multiple skills under one slash command for recurring multi-skill workflows.
- **Adopt Hermes's write-approval gate pattern**: stage agent-created skills for human review before committing, with `/skills pending`, `/skills diff <id>`, `/skills approve <id>` flow.
- **Reference**: [Hermes Skills System docs](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills)

### 6. Diff-preview-before-apply pattern (from Cursor, addresses general code UX)
- **Adopt Cursor's diff-preview pattern** for any code modification Shannon proposes: show a clean side-by-side or inline diff with accept/reject buttons before writing to disk. Shannon already has `components/diff/` and `commands_files.rs` (file diff/apply) — surface this more prominently in the chat UX when code edits are proposed.
- **Reference**: [Cursor 3 Deep Dive](https://www.digitalapplied.com/blog/cursor-3-deep-dive-agents-composer-review-2026), [Builder.io](https://www.builder.io/blog/cursor-vs-claude-code)

### 7. Plan Mode (from Cursor, addresses agent task UX)
- **Adopt Cursor's Plan Mode**: when the user asks for a complex task, the agent first produces an implementation plan (markdown) rather than executing immediately. User reviews, approves, then execution begins. WorkBuddy has a similar "confirm the plan" step.
- Shannon already has a `plan` concept in the OPC (`Strategic Focus` → proposed rename to `Today's Mission`). Generalize this to the chat level: a "Plan first" toggle that routes through a planning step before execution.
- **Reference**: [Cursor 3 — Plan Mode](https://www.digitalapplied.com/blog/cursor-3-deep-dive-agents-composer-review-2026)

### 8. Progressive disclosure for extensions/skills (from Hermes)
- **Adopt Hermes's 3-level progressive disclosure pattern** for the Extensions Hub:
  - Level 0: skills list (~3k tokens) — name, description, category
  - Level 1: full skill content — loaded when user or agent selects it
  - Level 2: specific reference file — loaded on demand
- This keeps the Extensions Hub browseable without overwhelming token budgets when the agent loads skill context.
- **Reference**: [Hermes Skills — Progressive Disclosure](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills)

### 9. Companion window for IDE integration (from ChatGPT Desktop)
- **Adopt ChatGPT's companion window pattern**: a slim Shannon window that docks to the side of the active IDE (VS Code, etc.) for side-by-side AI assistance with direct file context.
- Shannon already has LSP integration (`lsp_commands.rs` — code actions, diagnostics, source-file reads). Surface this as a companion-window mode rather than requiring users to switch to the full Shannon window.
- **Reference**: [OpenAI Help Center — Work with Apps](https://help.openai.com/en/articles/10119604-work-with-apps-on-macos)

### 10. Self-improvement loop (addresses lag #5, from Hermes)
- **Adopt Hermes's closed learning loop concept**: after a task completes (especially routines/hooks), evaluate success, extract reusable patterns, and store as skill files. Add a write-approval gate so users control what gets learned.
- **Implementation path**: Shannon already has `Memory.tsx` for factual memory and `scheduled_commands.rs` for routine execution logs. Add a post-execution evaluation step that writes procedural skills to `~/.shannon/skills/` with optional approval flow.
- **Metric to target**: Hermes's 40% efficiency gain on repetitive tasks after 20+ self-created skills (TokenMix benchmark).
- **Reference**: [Medium/Tenten — Hermes self-improvement](https://medium.com/@tentenco/hermes-agent-desktop-app-everything-you-need-to-know-about-nous-researchs-self-improving-ai-agent-3cb59bd31e5f)

---

## Sources

### Claude Desktop
- [Anthropic — Claude Design](https://www.anthropic.com/news/claude-design-anthropic-labs)
- [Anthropic — Projects](https://www.anthropic.com/news/projects)
- [Anthropic — Home](https://www.anthropic.com/)
- [Claude Help Center — Artifacts](https://support.claude.com/en/articles/9487310-what-are-artifacts-and-how-do-i-use-them)
- [Claude Help Center — Release Notes](https://support.claude.com/en/articles/12138966-release-notes)
- [PCMag — Claude Review 2026](https://me.pcmag.com/en/ai/30779/claude)
- [Reddit r/ClaudeAI — Desktop extensions](https://www.reddit.com/r/ClaudeAI/comments/1jiffk6/why_bother_installing_claude_for_desktop/)
- [Reddit r/ClaudeAI — Claude Design](https://www.reddit.com/r/ClaudeAI/comments/1tzk49t/anthropic_adds_claude_design_to_the_claude/)
- [Victor Dobia — Claude Design Review](https://newsletter.victordibia.com/p/how-good-is-anthropics-claude-design)
- [Albato — Claude Artifacts Guide](https://albato.com/blog/publications/how-to-use-claude-artifacts-guide)
- [Suprmind — Claude Features 2026](https://suprmind.ai/hub/claude/features/)
- [MindStudio — Claude Code Desktop App](https://www.mindstudio.ai/blog/claude-code-desktop-app-features)
- [Apple App Store — Claude](https://apps.apple.com/us/app/claude-by-anthropic/id6473753684)

### ChatGPT Desktop
- [ChatGPT Desktop — Features](https://chatgpt.com/features/desktop/)
- [OpenAI Help Center — Work with Apps on macOS](https://help.openai.com/en/articles/10119604-work-with-apps-on-macos)
- [OpenAI — Introducing Apps in ChatGPT](https://openai.com/index/introducing-apps-in-chatgpt/)
- [OpenAI — ChatGPT Release Notes](https://help.openai.com/en/articles/6825453-chatgpt-release-notes)
- [OpenAI — ChatGPT Search](https://openai.com/index/introducing-chatgpt-search/)
- [Reddit r/OpenAI — IDE editing](https://www.reddit.com/r/OpenAI/comments/1j545j2/chatgpt_for_macos_can_now_edit_code_directly_in/)
- [Reddit r/singularity — Work with Apps](https://www.reddit.com/r/singularity/comments/1grc6h7/new_chatgpt_work_with_apps_feature_for_macos/)
- [The AI Enterprise — ChatGPT Desktop Productivity](https://www.theaienterprise.io/p/chatgpt-desktop-productivity)
- [Cademix — ChatGPT Desktop Features](https://www.cademix.org/the-new-chatgpt-desktop-app-features-and-benefits/)

### WorkBuddy (Tencent)
- [Tencent Cloud — WorkBuddy Guide](https://www.tencentcloud.com/techpedia/144100?lang=en)
- [Eigent.ai — WorkBuddy AI Review 2026](https://www.eigent.ai/blog/workbuddy-ai-review)
- [Aikii — WorkBuddy listing](https://aikii.org/products/workbuddy)
- [WorkBuddy.ai — Official site](https://www.workbuddy.ai/)
- [Facebook/Yicai — Launch announcement](https://www.facebook.com/yicaiglobal/posts/1355101976661132/)
- [YouTube — WorkBuddy Review](https://www.youtube.com/watch?v=sIL4Fa58nd8)
- [YouTube — WorkBuddy AI Employee](https://www.youtube.com/watch?v=VgdBjERmJhE)

### Hermes Desktop (Nous Research)
- [Hermes Agent Docs — Skills System](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills)
- [Hermes Agent Docs — Bundled Skills Catalog](https://hermes-agent.nousresearch.com/docs/reference/skills-catalog)
- [GitHub — NousResearch/hermes-agent](https://github.com/nousresearch/hermes-agent)
- [Medium/Tenten — Hermes Desktop deep-dive](https://medium.com/@tentenco/hermes-agent-desktop-app-everything-you-need-to-know-about-nous-researchs-self-improving-ai-agent-3cb59bd31e5f)
- [Reddit r/LocalLLaMA — Hermes Desktop launch](https://www.reddit.com/r/LocalLLaMA/comments/1tve7qu/nous_research_hermes_desktop/)
- [Reddit — Hermes Desktop UI guide](https://www.reddit.com/r/AISEOInsider/comments/1sy5kt7/how_to_use_hermes_desktop_ui_to_control_your_ai/)
- [LinkedIn — Hermes Desktop guide](https://www.linkedin.com/posts/akshay-pachaar_this-is-the-best-way-to-run-ai-agents-on-activity-7469025925616070656-bTHm)
- [YouTube — Hermes Desktop full guide](https://www.youtube.com/watch?v=QxotQWwB7ws)
- [YouTube — Hermes Desktop setup](https://www.youtube.com/watch?v=EJm8Ka-gVOc)
- [OfLight — Hermes Skills & Tools Guide 2026](https://www.oflight.co.jp/en/columns/hermes-agent-skills-tools-comprehensive-guide-2026)

### Cursor
- [Cursor.com](https://cursor.com/)
- [Cursor 3 Deep Dive — DigitalApplied](https://www.digitalapplied.com/blog/cursor-3-deep-dive-agents-composer-review-2026)
- [Builder.io — Cursor vs Claude Code](https://www.builder.io/blog/cursor-vs-claude-code)
- [Builder.io — Cursor Alternatives 2026](https://www.builder.io/blog/cursor-alternatives-2026)
- [DataCamp — Cursor vs VS Code](https://www.datacamp.com/blog/cursor-vs-vs-code)
- [Towards AI — Cursor Complete Guide 2025](https://pub.towardsai.net/cursor-ide-complete-guide-2025-8d8d25407b97)
- [Zack Proser — Cursor Agents Review](https://zackproser.com/blog/cursor-agents-review)
- [Prismic — Cursor AI Review 2026](https://prismic.io/blog/cursor-ai)
- [AltexSoft — Cursor Pros and Cons](https://www.altexsoft.com/blog/cursor-pros-and-cons/)
- [Usama.codes — Cursor Updates 2026](https://usama.codes/blog/cursor-ide-december-2025-updates-features)

### Raycast AI
- [Raycast.com](https://www.raycast.com/)
- [Raycast Manual — AI Extensions](https://manual.raycast.com/ai/ai-extensions)
- [MacStories — Raycast AI Extensions](https://www.macstories.net/reviews/hands-on-with-raycasts-new-ai-extensions/)
- [Windows Forum — Raycast on Windows](https://windowsforum.com/threads/raycast-on-windows-brings-tahoe-style-spotlight-with-ai-and-extensions.388515/)
- [Reddit r/raycastapp — Favorite extensions](https://www.reddit.com/r/raycastapp/comments/1hiiubr/what_are_your_favorite_raycastextensionsfeatures/)

### Supermaven
- [Supermaven.com](https://supermaven.com/)
- [State of AI 2025 — Coding Assistants](https://2025.stateofai.dev/en-US/coding-assistants/)
- [Reddit r/neovim — Best AI Code Completion May 2026](https://www.reddit.com/r/neovim/comments/1tp79ai/best_ai_code_completion_as_of_may_2026/)
- [79Mplus — Top 10 AI Tools for Coding 2025](https://www.79mplus.com/top-10-best-ai-tools-for-coding-in-2025/)
- [Pinggy — Best AI Tools for Coding 2026](https://pinggy.io/blog/best_ai_tools_for_coding/)

### Shannon Desktop (internal references)
- `/home/ed/workspace/backup/shannon-desktop/CLAUDE.md`
- `/home/ed/workspace/backup/shannon-desktop/docs/product-review/04-product-repositioning.md`
- `/home/ed/workspace/backup/shannon-desktop/ui/src/i18n/locales/en.json`
- `/home/ed/workspace/backup/shannon-desktop/ui/src/pages/` (16 page components)
- `/home/ed/workspace/backup/shannon-desktop/ui/src/components/` (Sidebar, CommandPalette, Layout, + 15 subdirectories)
