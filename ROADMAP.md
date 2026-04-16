# Shannon Code - Implementation Roadmap

> Updated: 2026-04-16
> Priority: P0 features referencing Claude Code's implementation approach
> Goal: Claude Code MCP/Skill ecosystem compatibility

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

**Effort**: Large | **Files**: New `shannon-mcp/src/config.rs`, `shannon-core/src/plugins.rs`, `shannon-cli/src/main.rs`

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
```

**Why this order**:
- Project Memory first: lowest effort, highest immediate impact on every session
- Hooks second: enables extensibility without waiting for every feature
- Compaction third: requires project memory for re-injection after compact
- Skills fourth: mostly compatible already, just needs verification + wiring
- MCP fifth: unlocks the entire Claude Code MCP server ecosystem
- Sandbox sixth: security hardening after core features are stable
- Multi-session last: architectural change, benefits from stable foundation

---

## Future Considerations (P1-P3)

Moved to [docs/ROADMAP-FUTURE.md](docs/ROADMAP-FUTURE.md) for later review:

- P1: Auto-commit, Undo/Snapshot, Repo Map (tree-sitter), Auto-test Loop, LSP Integration, IDE Extensions
- P2: HTTP API Server, Architect Mode, Session Export, PDF Processing, Debug Instrumentation
- P3: Cross-surface Continuity, Cloud Execution, Agent SDK, Skills Marketplace, Voice Input
