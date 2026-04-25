# Shannon Code - Implementation Roadmap

> Updated: 2026-04-23
> Priority: P0 features referencing Claude Code's implementation approach
> Goal: Claude Code MCP/Skill/Agent ecosystem compatibility

## Current Infrastructure Status

Shannon already has significant foundational code. The work is primarily **integration and compatibility**, not greenfield development.

| Module | Files | Status | What's Missing |
|--------|-------|--------|----------------|
| MCP Protocol | `shannon-mcp/src/protocol.rs` | Complete | Config file loader (settings.json/.mcp.json) |
| MCP Transport | `shannon-mcp/src/transport.rs` | Stdio/SSE/HTTP/WS | Environment variable expansion in config |
| MCP Client | `shannon-mcp/src/client.rs` | Working | Server lifecycle management from config |
| Skills | `shannon-skills/` | Working | Already reads `.claude/skills/*/SKILL.md` |
| Compaction | `shannon-core/src/compact.rs` | 5 strategies | Query engine integration, LLM summarizer, re-injection |
| Hooks | `shannon-core/src/hooks.rs` | Types + config | Tool execution pipeline integration |
| Project Memory | `shannon-core/src/project_instructions.rs` | Loads CLAUDE.md | Session lifecycle integration, auto-memory |
| Project Memory | `shannon-core/src/project_memory.rs` | Types defined | Merge + inject into system prompt |
| Agent Teams | `shannon-agents/` | Coordinator + TaskBoard + Teammate | See gap analysis below |

---

## Agent Teams: Claude Code Gap Analysis

> Comparison against Claude Code's agent teams system (v2.1.32+, Feb 2026)

### What Shannon Has (Implemented)

| Feature | Shannon Implementation | Claude Code Equivalent |
|---------|----------------------|----------------------|
| Team creation | `AgentCoordinator.create_team()` | `TeamCreate` tool |
| Agent spawning | `AgentTool.spawn_agent()` + `SubAgentRegistry.spawn()` | `Agent` tool with `team_name` + `name` |
| Inter-agent messaging | `AgentCoordinator.send_message()` + `SendMessageTool` | `SendMessage` tool (file mailbox) |
| Task board | `TaskBoard` with priorities, dependencies, assignments | `TaskCreate/TaskList/TaskUpdate` (per-file JSON) |
| Task dependencies | `blocked_by` / `blocks` arrays on `AgentTask` | `addBlocks` / `addBlockedBy` on task files |
| Teammate state machine | `TeammateStatus` (Idle/Busy/Planning/ShuttingDown/Stopped/Error) | Idle/active state in config.json |
| Agent executor | `AgentExecutor` trait + `LlmAgentExecutor` | In-process Claude Code instance |
| Worktree isolation | `WorktreeManager` + `EnterWorktreeTool`/`ExitWorktreeTool` | `isolation: "worktree"` param |
| Teammate config | `TeammateConfig` (agent_type, capabilities, max_tasks, plan_mode, model, system_prompt) | `.claude/agents/{name}.md` frontmatter |
| Protocol messages | `ProtocolMessage` (ShutdownRequest/Response, PlanApproval) | Same protocol via JSON-in-JSON |
| Coordinator events | `CoordinatorEvent` broadcast channel | File polling on inbox |
| Agent discovery | `coordinator.team_manifest()` | Read `config.json` members array |
| Idle detection | `coordinator.idle_agents()` | Auto `idle_notification` after every turn |
| Task tools for LLM | `TeamTaskCreateTool/UpdateTool/ListTool` | `TaskCreate/TaskList/TaskUpdate` tools |
| Multi-agent spawner | `MultiAgentSpawner` with parallel execution | Background agent spawning |
| Assignment strategies | RoundRobin/LeastLoaded/CapabilityBased/FirstAvailable | Self-claim (lowest ID first) |
| Shared context | `TeamContext` bridges coordinator + registry + LLM config | `~/.claude/teams/{team}/config.json` |

### Critical Gaps (Missing vs Claude Code)

| Gap | Priority | Description | Effort |
|-----|----------|-------------|--------|
| **File-based coordination** | P0 | Claude Code uses flat files (`~/.claude/teams/`, `~/.claude/tasks/`) for all state. Shannon uses in-process `Arc<RwLock>` — teams don't survive restarts and aren't visible cross-process. | Large |
| **Separate process spawning** | P0 | Claude Code spawns each teammate as a **separate OS process** (tmux pane or in-process). Shannon runs teammates in the same process with shared memory. No true isolation. | Large |
| **Idle notification loop** | P1 | Claude Code teammates auto-send `idle_notification` after every LLM turn. Shannon has `idle_agents()` but no automatic polling/notification mechanism. | Medium |
| **Peer-to-peer messaging** | P1 | Claude Code supports direct teammate-to-teammate DMs. Shannon routes all messages through the coordinator (hub-and-spoke). | Medium |
| **Broadcast messaging** | P1 | Claude Code supports `SendMessage` with `type: "broadcast"` to all teammates. Shannon has the coordinator for this but no explicit broadcast tool. | Low |
| **Self-claim task scheduling** | P1 | Claude Code teammates self-claim tasks (call TaskList, grab lowest ID). Shannon uses coordinator-assigned strategies. Self-claim is more natural for autonomous agents. | Medium |
| **File-based task persistence** | P1 | Claude Code stores each task as a separate JSON file with `flock()` for concurrency. Shannon uses in-memory `HashMap`. Tasks don't survive restarts. | Large |
| **Custom agent definitions** | P1 | Claude Code reads `.claude/agents/{name}.md` with YAML frontmatter for custom agent configs. Shannon has `TeammateConfig` but no file-based discovery. | Medium |
| **Feature flag gating** | P2 | Claude Code requires `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`. Shannon has no feature flag for team features. | Low |
| **tmux display backend** | P2 | Claude Code shows each teammate in its own tmux pane. Shannon has no multi-pane agent display. | Large |
| **Permission inheritance** | P2 | Claude Code teammates inherit lead's permission mode. Shannon doesn't pass permission context to teammates. | Medium |
| **Team hooks** | P2 | Claude Code has `SubagentStart`, `SubagentStop`, `TeammateIdle`, `TaskCreated`, `TaskCompleted` hooks. Shannon has no team-specific hooks. | Medium |
| **High-watermark task IDs** | P3 | Claude Code uses auto-incrementing integer IDs via `.highwatermark` file. Shannon uses UUIDs (works fine but differs from standard). | Low |
| **Message summary field** | P3 | Claude Code `SendMessage` requires a 5-10 word `summary` for UI preview. Shannon messages have no summary field. | Low |
| **Team member colors** | P3 | Claude Code assigns display colors to teammates. Shannon has no color system. | Low |
| **Metadata on tasks** | P3 | Claude Code `TaskUpdate` supports merging arbitrary `metadata` key-value pairs. Shannon has `metadata: serde_json::Value` but no merge API in update tool. | Low |

### Architectural Differences (Design Choices, Not Gaps)

| Dimension | Shannon | Claude Code | Assessment |
|-----------|---------|-------------|------------|
| **State storage** | In-process `Arc<RwLock<HashMap>>` | Flat files on disk (`~/.claude/`) | Shannon is faster but not persistent |
| **Messaging** | In-process channels (mpsc) | File-based mailbox (append + poll) | Shannon is faster; Claude Code is cross-process |
| **Task format** | In-memory `AgentTask` structs | One JSON file per task + `flock()` | Shannon is simpler; Claude Code is crash-safe |
| **Agent execution** | `AgentExecutor` trait (pluggable) | Separate Claude Code CLI process | Shannon is more flexible; Claude Code is more isolated |
| **Assignment** | Coordinator-assigned (4 strategies) | Self-claim (lowest ID first) | Different paradigms, both valid |
| **Coordinator** | Central `AgentCoordinator` | No central coordinator; peer-based via files | Shannon has a coordinator; Claude Code is distributed |

---

## P0 - Approved for Implementation

### 1. Project Memory (CLAUDE.md Compatible)

**Reference**: Claude Code's CLAUDE.md hierarchy + auto-memory system

**Current State**: `project_instructions.rs` already walks up directories loading `CLAUDE.md`/`AGENTS.md`/`GEMINI.md`. `project_memory.rs` has types but no integration.

**Work Required**:
- [ ] Wire `project_instructions.rs` into session startup (inject into system prompt)
- [ ] Support Claude Code's file hierarchy: `~/.claude/CLAUDE.md` > `./CLAUDE.md` > `./.claude/CLAUDE.md` > `./CLAUDE.local.md`
- [ ] Support `@import` syntax (`@README`, `@docs/guide.md`)
- [ ] Implement auto-memory (`/remember` command → `~/.shannon/projects/<project>/memory/`)
- [ ] Support `MEMORY.md` index file (first 200 lines loaded at session start)
- [ ] Re-inject project-root CLAUDE.md after compaction

**Effort**: Medium | **Files**: `project_instructions.rs`, `project_memory.rs`, `query_engine/engine.rs`

---

### 2. Hook System (Claude Code Compatible)

**Reference**: Claude Code's hook JSON format with stdin/stdout protocol

**Current State**: `hooks.rs` has types (`HookEventType`, `HookConfig`, matcher) and shell execution. Not wired into tool pipeline.

**Work Required**:
- [ ] Support Claude Code's settings.json hook format:
  ```json
  {
    "hooks": {
      "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "command": "check-script.sh" }] }],
      "PostToolUse": [{ "matcher": "Edit|Write", "hooks": [{ "command": "prettier --write" }] }]
    }
  }
  ```
- [ ] Implement stdin JSON protocol (tool_name, tool_input on stdin; exit code 0=allow, 2=deny)
- [ ] Wire `PreToolUse` into `tool_execution.rs` (before tool execute)
- [ ] Wire `PostToolUse` into `tool_execution.rs` (after tool execute)
- [ ] Wire `SessionStart`/`SessionEnd` into REPL lifecycle
- [ ] Wire `UserPromptSubmit` into `submit_input()`
- [ ] Support config locations: `~/.shannon/settings.json`, `.shannon/settings.json`, `.shannon/settings.local.json`
- [ ] Also read from `~/.claude/` and `.claude/` for Claude Code compatibility

**Effort**: Medium | **Files**: `hooks.rs`, `tool_execution.rs`, `repl/mod.rs`

---

### 3. Context Compaction (3-Tier, Claude Code Approach)

**Reference**: Claude Code's auto-compaction with smart re-injection

**Current State**: `compact.rs` has full engine (5 strategies, auto-trigger, message grouping). NOT integrated into query engine.

**Work Required**:
- [ ] Wire `CompactEngine` into `query_engine/engine.rs` — auto-check before each LLM call
- [ ] Implement `LlmSummarizer` (calls LLM API to summarize old messages)
- [ ] Re-inject project-root CLAUDE.md + auto-memory after compaction (Claude Code approach)
- [ ] Re-invoke active skill bodies after compaction (capped at 5K per skill)
- [ ] Add `/compact` REPL command for manual compaction
- [ ] Show compaction metrics in TUI (tokens before/after, reduction %)

**Effort**: Medium | **Files**: `compact.rs`, `query_engine/engine.rs`, `repl/commands.rs`

---

### 4. MCP Ecosystem Compatibility (Claude Code Compatible)

**Reference**: Claude Code's settings.json / .mcp.json configuration format

**Current State**: `shannon-mcp` has full protocol, transports, client, auth. **Missing**: Config file discovery and server management.

**Work Required**:
- [ ] Implement Claude Code-compatible MCP config loader:
  ```json
  // .mcp.json (project-level, shared via git)
  { "mcpServers": { "github": { "type": "stdio", "command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"] } } }

  // settings.json / settings.local.json (user/project-level)
  { "mcpServers": { "my-api": { "type": "http", "url": "https://api.example.com/mcp", "headers": { "Authorization": "Bearer $MY_TOKEN" } } } }
  ```
- [ ] Support 3 config scopes: user (`~/.claude/settings.json`), project (`.mcp.json`), local (`.claude/settings.local.json`)
- [ ] Environment variable expansion (`$VAR`, `${VAR}`) in URLs, headers, args
- [ ] Auto-discover and start MCP servers at session init
- [ ] Register MCP tools into Shannon's `ToolRegistry`
- [ ] CLI commands: `shannon mcp add`, `shannon mcp remove`, `shannon mcp list`
- [ ] Graceful shutdown of MCP server processes on session end
- [ ] **Compatibility**: Read from BOTH `.claude/` and `.shannon/` paths

**Effort**: Large | **Files**: `shannon-mcp/src/config.rs`, `shannon-cli/src/main.rs`

---

### 5. Skill System Compatibility (Claude Code Compatible)

**Reference**: Claude Code's SKILL.md format with YAML frontmatter

**Current State**: `shannon-skills` already reads `.claude/skills/*/SKILL.md` with YAML frontmatter. **Already mostly compatible!**

**Work Required**:
- [ ] Verify full YAML frontmatter compatibility (`name`, `description`, `allowed-tools`)
- [ ] Implement progressive loading: metadata always → full content on invocation
- [ ] Build `<available_skills>` list injected into LLM tool description
- [ ] Implement LLM-based skill routing (let model select, not pattern matching)
- [ ] Support Claude Code's `@import` syntax within skill files
- [ ] Wire skill execution into REPL (`/skill-name` invocation)
- [ ] Support both `.claude/skills/` AND `.shannon/skills/` directories
- [ ] Support `~/.claude/skills/` (global) AND `.claude/skills/` (project)

**Effort**: Medium | **Files**: `shannon-skills/src/loader.rs`, `shannon-skills/src/executor.rs`, `repl/commands.rs`

---

### 6. Sandboxed Tool Execution

**Reference**: Codex CLI's Seatbelt/bubblewrap approach; Claude Code's permission modes

**Work Required**:
- [ ] Implement permission modes: `default`, `auto`, `bypassPermissions`, `plan`
- [ ] On Linux: integrate `bubblewrap` (bwrap) for sandboxed Bash execution
- [ ] On macOS: research Seatbelt (sandbox-exec) integration
- [ ] Restrict filesystem access to project directory + tmp
- [ ] Block network access in sandbox mode (except MCP)
- [ ] Configurable via `.shannon/settings.json` or `.claude/settings.json`
- [ ] Permission prompts in TUI for dangerous operations

**Effort**: Large | **Files**: `shannon-tools/src/bash.rs`, new `shannon-core/src/sandbox.rs`

---

### 7. Multi-Session Architecture

**Reference**: Open Code's multi-session design; Claude Code's session continuity

**Work Required**:
- [ ] Session persistence: save/restore conversation state to disk
- [ ] Session listing and switching (`/sessions` command)
- [ ] Resume last session on startup
- [ ] Named sessions for context switching
- [ ] Session state: messages, tool registry state, MCP connections
- [ ] Background session execution (non-interactive mode)

**Effort**: Large | **Files**: New `shannon-core/src/session.rs`, `repl/mod.rs`

---

## P0.5 - Agent Teams Enhancements

> These are specific gaps identified against Claude Code's agent teams implementation.
> The foundation is in place (coordinator, task board, teammate, executor, tools).
> These items close the functional gaps.

### AT-1. File-Based Team & Task Persistence

**Reference**: Claude Code stores all team state as flat files (`~/.claude/teams/`, `~/.claude/tasks/`)

**Why**: Current in-process state dies on restart. File-based state enables cross-process coordination and crash recovery.

**Work Required**:
- [ ] Team config persistence: write `config.json` on team create/update, read on startup
- [ ] Task file persistence: one JSON file per task with `flock()` for concurrency
- [ ] High-watermark file for auto-incrementing task IDs
- [ ] Mailbox files: `~/.claude/teams/{team}/inboxes/{agent}.json` for message delivery
- [ ] `TeamContext::new()` loads existing teams from disk
- [ ] Graceful degradation: if no files exist, create fresh in-memory state

**Effort**: Large | **Files**: New `shannon-agents/src/persistence.rs`, `coordinator.rs`, `task_board.rs`

---

### AT-2. Separate-Process Agent Spawning

**Reference**: Claude Code spawns teammates as separate `claude` CLI processes (tmux pane or in-process)

**Why**: True isolation between agents — a crash in one doesn't take down others. Each agent has its own context window and tool access.

**Work Required**:
- [ ] Define agent spawn backend trait: `InProcessBackend` (current) + `ProcessBackend` (new)
- [ ] `ProcessBackend`: spawn `shannon` CLI with `--team-agent` mode
- [ ] Pass agent config via env vars or temp file (`SHANNON_TEAM_NAME`, `SHANNON_AGENT_NAME`)
- [ ] Teammate reads config, connects to coordinator, starts work loop
- [ ] Capture stdout/stderr for agent output
- [ ] Optional tmux integration: spawn in tmux pane with `tmux split-window`

**Effort**: Large | **Files**: New `shannon-agents/src/spawn.rs`, `shannon-cli/src/main.rs`, `shannon-ui/src/repl/mod.rs`

---

### AT-3. Idle Notification + Self-Claim Task Loop

**Reference**: Claude Code teammates auto-send `idle_notification` after every LLM turn and self-claim tasks

**Why**: Enables autonomous agent behavior — agents find their own work without lead micromanagement.

**Work Required**:
- [ ] After `AgentExecutor.execute()` returns, auto-send idle notification to coordinator
- [ ] Teammate work loop: idle → TaskList (find unblocked, no owner) → TaskUpdate (claim) → execute → complete → idle
- [ ] Self-claim logic: prefer lowest ID, respect `blocked_by` constraints
- [ ] Lead notification: when agent goes idle, surface to LLM as `QueryEvent::Info`
- [ ] Configurable idle poll interval (default: check tasks every 3s while idle)

**Effort**: Medium | **Files**: `teammate.rs`, `coordinator.rs`, `task_tools.rs`

---

### AT-4. Custom Agent Definitions (`.claude/agents/`)

**Reference**: Claude Code reads `.claude/agents/{name}.md` with YAML frontmatter for agent configs

**Why**: Users define reusable agent templates (like "backend-dev", "test-runner") without code changes.

**Work Required**:
- [ ] Parse `.claude/agents/*.md` files with YAML frontmatter
- [ ] Frontmatter fields: `description`, `tools`, `disallowedTools`, `model`, `permissionMode`, `maxTurns`, `isolation`
- [ ] Body = system prompt
- [ ] `AgentTool` resolves `agent_type` to custom definitions
- [ ] Merge custom definition with `TeammateConfig` defaults
- [ ] Discovery: list available agent types from files
- [ ] Support both `.claude/agents/` and `.shannon/agents/`

**Effort**: Medium | **Files**: New `shannon-agents/src/agent_defs.rs`, `shannon-tools/src/agent.rs`

---

### AT-5. Peer-to-Peer + Broadcast Messaging

**Reference**: Claude Code supports direct teammate-to-teammate DMs and `type: "broadcast"` messages

**Why**: Hub-and-spoke (through coordinator) limits parallel collaboration. Peer messaging enables agents to coordinate directly.

**Work Required**:
- [ ] Add `broadcast` message type to `MessageType` enum
- [ ] `SendMessageTool` support for `recipient: "*"` (broadcast)
- [ ] Allow teammates to send messages to each other (not just to/from lead)
- [ ] Message summary field (5-10 word preview for UI)
- [ ] Lead visibility into peer DMs (summary in idle notification)

**Effort**: Medium | **Files**: `message.rs`, `coordinator.rs`, `sub_agent.rs`

---

### AT-6. Team Feature Flag + Permission Inheritance

**Reference**: Claude Code requires `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` and teammates inherit lead's permissions

**Why**: Team features are complex and resource-intensive. Feature flag lets users opt in. Permission inheritance ensures consistent security.

**Work Required**:
- [ ] Add `SHANNON_AGENT_TEAMS` env var (or config flag) to enable team tools
- [ ] When disabled: `TeamContext::new()` returns error, team tools not registered
- [ ] Pass lead's `PermissionManager` mode to spawned teammates
- [ ] Document feature flag in help text and README

**Effort**: Low | **Files**: `context.rs`, `shannon-tools/src/lib.rs`, `shannon-cli/src/main.rs`

---

### AT-7. Team Hooks (Quality Gates)

**Reference**: Claude Code has `SubagentStart`, `SubagentStop`, `TeammateIdle`, `TaskCreated`, `TaskCompleted` hooks

**Why**: Quality gates prevent bad task completion, block inappropriate agent spawns, and enforce team policies.

**Work Required**:
- [ ] Add team hook events to `HookEventType` enum
- [ ] Fire `SubagentStart` before agent spawn (exit 2 = block)
- [ ] Fire `SubagentStop` after agent finishes
- [ ] Fire `TeammateIdle` when agent goes idle (stderr = feedback to agent)
- [ ] Fire `TaskCreated` / `TaskCompleted` on task lifecycle events (exit 2 = block)
- [ ] Hook handlers: command (shell), prompt (LLM eval), agent (multi-turn)

**Effort**: Medium | **Files**: `hooks.rs`, `coordinator.rs`, `task_board.rs`

---

## Implementation Order

```
Phase 1 (Integration):  1 → 2 → 3
  Project Memory → Hooks → Compaction
  These wire existing code into the query engine. Each builds on the previous.

Phase 2 (Ecosystem):    5 → 4
  Skills → MCP Config
  Skills is mostly done (verify compatibility). MCP config unlocks 300+ servers.

Phase 3 (Security):     6
  Sandboxed Execution
  Requires hooks (Phase 1) to be in place for permission prompts.

Phase 4 (Architecture): 7
  Multi-session
  Requires stable foundation from Phases 1-3.

Phase 5 (Agent Teams):  AT-6 → AT-1 → AT-3 → AT-5 → AT-4 → AT-7 → AT-2
  Feature Flag → File Persistence → Idle/Self-Claim → P2P Messaging → Agent Defs → Team Hooks → Process Spawn
  Feature flag first (gating), then persistence (foundation), then behavioral features, then heavy lift (process spawn).
```

**Why this order**:
- Project Memory first: lowest effort, highest immediate impact on every session
- Hooks second: enables extensibility without waiting for every feature
- Compaction third: requires project memory for re-injection after compact
- Skills fourth: mostly compatible already, just needs verification + wiring
- MCP fifth: unlocks the entire Claude Code MCP server ecosystem
- Sandbox sixth: security hardening after core features are stable
- Multi-session seventh: architectural change, benefits from stable foundation
- Agent Teams last: foundation is in place; enhancements are additive

---

## Future Considerations (P1-P3)

### Competitive Analysis Deferred Items (2026-04-23)

Based on comparison against Claude Code, Codex CLI, OpenCode, Aider, Cursor, and Gemini CLI.
These items are deferred from the current sprint for future evaluation.

#### P0-Future: PTY Pseudo-Terminal (Gemini CLI Pattern)
- **Why**: Interactive programs (vim, htop, npm init) currently block the session
- **Competitor**: Gemini CLI spawns a PTY, takes snapshots, renders inline — handles any interactive program
- **Approach**: Integrate `portable-pty` crate into `BashTool`, detect interactive commands, switch to PTY snapshot mode
- **Effort**: High (7+ days)
- **Impact**: Solves a universal pain point; only Gemini CLI has this

#### P0-Future: RepoMap (Aider Pattern)
- **Why**: Shannon has SmartContext (path/identifier detection) but no global repo structure overview
- **Competitor**: Aider's RepoMap is its signature feature — tree-sitter-based concise map of all classes, functions, types
- **Approach**: Integrate tree-sitter, build project-level RepoMap, inject into context with token budget
- **Effort**: High (5+ days, tree-sitter + language grammars)
- **Impact**: Significantly improves code understanding accuracy

#### P1-Future: Computer Use / GUI Interaction (Claude Code Pattern)
- **Why**: Frontend developers need to see and interact with rendered UIs
- **Competitor**: Claude Code supports macOS Computer Use — screenshot, click, fill forms
- **Approach**: Leverage existing screenshot + terminal_image system, add screenshot→analyze→coordinate return
- **Effort**: High (platform-specific APIs)
- **Impact**: Strong differentiator for frontend workflows

#### P1-Future: @ Reference Enhancement (Gemini CLI Pattern)
- **Why**: Shannon has @ file picker with image auto-attach
- **Competitor**: Gemini CLI @ supports files, images, PDFs, audio, URLs — all reference types
- **Approach**: Extend @ to support PDFs (reuse /pdf backend), URLs (WebFetchTool result injection), folders (directory tree injection)
- **Effort**: Low-Medium (2 days)

#### P2-Future: Client-Server Architecture (OpenCode Pattern)
- **Why**: Shannon is a single-process REPL
- **Competitor**: OpenCode C/S architecture allows local server + remote control from mobile/another PC
- **Approach**: Split `shannon serve` (existing HTTP API) + TUI client, support remote sessions
- **Effort**: Large (architecture change)

#### P2-Future: Docker Sandbox Default (Codex CLI Pattern)
- **Why**: Shannon has DockerSandbox config but incomplete integration
- **Competitor**: Codex CLI defaults to Docker container execution, only exposes project directory
- **Approach**: Complete Docker sandbox as opt-in default, reduce permission prompts
- **Effort**: Medium (3-5 days)

#### P2-Future: Google Search Grounding (Gemini CLI Pattern)
- **Why**: Shannon has WebSearchTool and WebFetchTool but no auto-grounding
- **Competitor**: Gemini CLI embeds Google Search in conversations for real-time information
- **Approach**: Auto-detect queries needing current info, trigger web search, inject results
- **Effort**: Low (tools exist, needs routing logic)

#### 2026-04-23 竞品对比推迟项

基于 Claude Code / Codex CLI / OpenCode 三方对比，以下任务推迟到后续迭代。

##### 高优先级（下一批次候选）

- **CI/CD Headless 模式**: `shannon -p "task"` 非交互模式，`--allowedTools` 预授权，`--output-format json` 结构化输出。参考 Codex CLI `codex exec` 和 Claude Code `-p` flag。
- **.git 路径默认只读保护**: Sandbox 层面默认保护 `.git/`、`.shannon/` 为只读，即使 workspace-write 模式。参考 Codex CLI protected paths。
- **Surface-agnostic Agent 协议**: Core 层与 Surface 层解耦，async channel 驱动，JSON-RPC 协议。参考 Codex CLI Thread/Turn/Item 三层原语设计。关键：Core 是纯库，不知道上层是 TUI/Web/IDE。
- **Guardian Agent 审批机制**: 专用只读 agent 做审批决策，替代交互式用户确认。CI/CD 场景自动审批，企业合规审批策略集中管理。参考 Codex CLI guardian subagent。
- **技能 Progressive Disclosure**: 启动时只注入 metadata（名称+描述），调用时才加载完整 SKILL.md。节省 context window。参考 Codex CLI progressive disclosure skills。
- **OpenTelemetry 集成**: `tracing-opentelemetry` crate，OTLP exporter，覆盖对话、API、工具调用事件。企业可观测性需求。参考 Codex CLI opt-in OTel。

##### 中优先级

- **`run` 子命令**: `shannon run "task"` 一次性执行，不进入交互 REPL。参考 OpenCode `opencode run`。
- **Feature Flags 系统**: 轻量 runtime feature toggle，配置文件 + CLI 切换，灰度发布新功能。参考 Codex CLI `codex features enable/disable`。
- **Plugin 系统兼容层**: ~~已完成~~ 已移除自定义 plugin 系统，统一使用 MCP 协议（Claude Code 兼容）。

##### 低优先级（长期路线图）

- **VS Code 扩展**: 利用已有 `api_server.rs` + `axum` 构建。优先于 Web UI。
- **Web UI**: 浏览器版 Shannon Code，无需本地安装。
- **桌面应用**: tauri 或 wry 构建轻量壳。参考 OpenCode desktop app。
- **Remote TUI**: 在一台机器运行 agent，从另一台通过 WebSocket 交互。参考 Codex CLI remote TUI。
- **CSV 批量 Subagent 编排**: 每个 CSV 行启动一个 worker agent，结果导回 CSV。参考 Codex CLI `spawn_agents_on_csv`。

---

#### 2026-04-24 竞品对比推迟项（Round 2）

基于 Claude Code / Codex CLI / OpenCode 深度对比，以下高价值任务推迟到下一迭代周期。

- **Remote TUI / 多设备续接**: 基于 BridgeService 暴露 WebSocket 端点，实现远程 TUI 客户端连接。参考 Codex CLI remote TUI (app-server + WebSocket)。Claude Code 支持手机续接本地会话。
- **对话分享**: `/share` 生成可分享链接。将 session transcript 导出为静态 HTML/markdown，上传至匿名 pastebin 或自托管服务。参考 OpenCode `/share`。
- **CSV Batch Agent Jobs**: CSV 每行启动一个 worker agent 的扇出模式，结果导回 CSV。结合已有 TeamCreate + TaskCreate 实现。参考 Codex CLI `spawn_agents_on_csv`。
- **Guardian Reviewer (审批审查子agent)**: 专用只读 subagent 做审批决策，替代交互式用户确认。CI/CD 场景自动审批，企业合规审批策略集中管理。参考 Codex CLI guardian_subagent。在 hook 系统中添加 `agent` 类型 hook，触发时 spawn 审查 agent 做风险评估。
- **企业级托管设置**: org-wide policy deployment，系统级策略文件 `/etc/shannon/policy.toml`（不可被用户覆盖），支持 MDM 部署。参考 Claude Code managed settings 和 Codex CLI requirements.toml。扩展现有 remote_settings.rs。

---

### Previously Deferred Items

- P1: Auto-commit, Repo Map (tree-sitter), Auto-test Loop, LSP Integration, IDE Extensions
- P2: HTTP API Server, Architect Mode, Session Export, PDF Processing, Debug Instrumentation
- P3: Cross-surface Continuity, Cloud Execution, Agent SDK, Skills Marketplace, Voice Input
