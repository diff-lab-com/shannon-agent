# P0: Agent Execution as Independent Processes

## Problem

Currently, all agents run as in-process tokio tasks sharing the same address space.
This limits:
- **Crash isolation**: One agent panic can crash the entire process
- **Resource isolation**: No memory/CPU boundaries between agents
- **Parallel execution**: Limited by single-process threading
- **Security**: Agents share filesystem access and environment

## Design

### Architecture

```
┌─────────────────────────┐
│   Coordinator Process    │
│  (parent / orchestration)│
│                          │
│  - TaskBoard             │
│  - MessageRouter         │
│  - ProcessManager        │
│  - FilePersistence       │
└──────────┬──────────────┘
           │ IPC (stdin/stdout JSON-RPC)
    ┌──────┼──────────┐
    ▼      ▼          ▼
┌───────┐ ┌───────┐ ┌───────┐
│Agent 1│ │Agent 2│ │Agent 3│
│process│ │process│ │process│
└───────┘ └───────┘ └───────┘
```

### IPC Protocol

Use JSON-RPC over stdin/stdout for coordinator-agent communication:

```json
// Coordinator → Agent
{"jsonrpc": "2.0", "method": "execute_task", "params": {"task_id": "...", "subject": "...", "description": "..."}, "id": 1}

// Agent → Coordinator (streaming)
{"jsonrpc": "2.0", "method": "task_progress", "params": {"task_id": "...", "chunk": "partial output..."}}
{"jsonrpc": "2.0", "method": "task_complete", "params": {"task_id": "...", "success": true, "output": "..."}, "id": 1}

// Agent → Coordinator (notification)
{"jsonrpc": "2.0", "method": "agent_idle", "params": {"available_tasks_count": 0}}
```

### Key Components

1. **`AgentProcessManager`** — Manages agent process lifecycle
   - `spawn_agent(config) -> AgentHandle`
   - `kill_agent(agent_name)`
   - `send_message(agent_name, message)`
   - `watch_health(agent_name) -> HealthStream`

2. **`AgentHandle`** — Represents a running agent process
   - `child: Child` — The spawned process
   - `stdin: WriteHalf` — For sending messages to agent
   - `stdout: ReadHalf` — For receiving messages from agent
   - `status: AgentProcessStatus` — Running/Stopped/Crashed

3. **`AgentProcessConfig`** — Configuration for spawning
   - `binary_path: PathBuf` — Path to the agent binary
   - `args: Vec<String>` — Command-line arguments
   - `env: HashMap<String, String>` — Environment variables
   - `worktree_path: Option<PathBuf>` — Working directory (isolated worktree)
   - `model: Option<String>` — LLM model to use
   - `system_prompt: Option<String>` — Agent system prompt

4. **Agent Binary** — A standalone `shannon-agent` binary
   - Reads JSON-RPC from stdin
   - Writes JSON-RPC to stdout
   - Logs to stderr (captured by coordinator)
   - Uses the same LLM client as the main process

### Lifecycle

```
1. Coordinator creates task board with tasks
2. Coordinator spawns agent processes (with worktree isolation)
3. Each agent process:
   a. Sends "ready" notification
   b. Waits for task assignment (via claim_next RPC or push)
   c. Executes task using LLM + tools
   d. Sends progress updates (streaming)
   e. Sends completion/failure result
   f. Goes idle, waits for next task
4. Coordinator monitors health, restarts crashed agents
5. On shutdown: coordinator sends "shutdown" RPC to all agents
```

### Implementation Plan

#### Phase 1: Core Infrastructure
- [ ] Create `shannon-agent` binary crate (agent process entry point)
- [ ] Define JSON-RPC protocol types (shared between coordinator and agent)
- [ ] Implement `AgentProcessManager` in `shannon-agents`
- [ ] Add stdin/stdout JSON-RPC transport layer

#### Phase 2: Integration
- [ ] Wire `AgentProcessManager` into `AgentCoordinator`
- [ ] Replace in-process `Teammate` with `AgentHandle` when configured
- [ ] Implement health monitoring and auto-restart
- [ ] Add process-based execution to `spawn_background_task_with_timeout`

#### Phase 3: Polish
- [ ] Add `CoordinatorConfig::agent_mode: AgentMode` (InProcess | Process)
- [ ] Graceful shutdown with timeout
- [ ] Resource limits per agent (memory, CPU via cgroups on Linux)
- [ ] Integration tests with process-based agents

### File Changes

| File | Change |
|------|--------|
| `crates/shannon-agents/src/process_manager.rs` | **New** — AgentProcessManager, AgentHandle |
| `crates/shannon-agents/src/protocol.rs` | **New** — JSON-RPC protocol types |
| `crates/shannon-agent/Cargo.toml` | **New** — Agent binary crate |
| `crates/shannon-agent/src/main.rs` | **New** — Agent process entry point |
| `crates/shannon-agents/src/coordinator.rs` | Modify — Use process manager when configured |
| `crates/shannon-agents/src/lib.rs` | Modify — Export new modules |
| `Cargo.toml` | Modify — Add workspace member |

### Backward Compatibility

- In-process mode remains the default (`AgentMode::InProcess`)
- Process mode is opt-in via config: `agent_mode: "process"`
- Both modes share the same coordinator API
- Transition can be gradual (per-team or per-agent configuration)
