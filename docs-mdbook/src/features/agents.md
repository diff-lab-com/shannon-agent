# Multi-Agent Orchestration

Shannon Code supports multi-agent workflows for complex tasks that benefit from parallel execution.

## /batch Command

The `/batch` command (alias `/parallel`) decomposes a task into subtasks, creates git worktrees for isolation, spawns agents, and creates PRs:

```
/batch "Add unit tests for all crates that are missing them"
```

This will:
1. Analyze the task and decompose into subtasks
2. Create a git worktree for each subtask
3. Spawn an agent per subtask with isolated working directory
4. Each agent works independently
5. PRs are created for completed work

## Per-Agent Configuration

Each agent can have custom settings:

| Setting | Description |
|---------|-------------|
| `model` | Override the LLM model (e.g., use a faster model for simple tasks) |
| `allowed_tools` | Restrict which tools the agent can use |
| `working_directory` | Worktree path for filesystem isolation |

## Agent Dashboard

Press `Ctrl+A` to open the agent dashboard showing:
- Active agents and their status
- Task assignments and progress
- Agent output in real-time

Three view modes: compact, expanded, and detailed.

## Task Board

Shared task list with:
- Priority levels (Critical, High, Medium, Low)
- Dependency tracking between tasks
- Automatic assignment based on agent availability
- Events for task lifecycle (created, assigned, completed)
