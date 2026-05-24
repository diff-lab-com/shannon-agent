# shannon-agents

Multi-agent orchestration with teammate coordination, task boards, and worktree isolation.

## Architecture

```
AgentCoordinator
├── SubAgentRegistry — manages agent lifecycle
├── TaskBoard — shared task list with dependencies
├── Protocol (JSON-RPC) — inter-agent communication
└── WorktreeManager — git worktree isolation
```

## Key Components

### AgentCoordinator
Central orchestrator that manages agent spawning, task assignment, and message routing.

### SubAgentRegistry
Tracks active agents with status (Idle, Working, Completed, Failed). Supports spawning with custom model and tool configurations.

### TaskBoard
Shared task list with:
- Priority levels (Critical, High, Medium, Low)
- Dependency tracking (blocked_by)
- Assignment and ownership
- Events (TaskCreated, TaskAssigned, TaskCompleted)

### Protocol
JSON-RPC based inter-agent communication:
- `agent_ready` / `agent_idle` — lifecycle
- `claim_task` / `execute_task` / `task_complete` — task management
- `send_message` — peer-to-peer messaging
- `shutdown` — graceful termination

### WorktreeManager
Git worktree isolation for agents:
- Each agent gets its own working directory
- System prompt includes isolation instructions
- Automatic cleanup on agent shutdown

## Per-Agent Configuration

```rust
pub struct AgentSpawnInput {
    pub model: Option<String>,        // Override LLM model
    pub allowed_tools: Vec<String>,   // Restrict available tools
    pub working_directory: Option<PathBuf>,  // Worktree path
}
```
