# shannon-commands

Built-in slash commands for the REPL. Commands are registered in `CommandRegistry` and invoked via `/command-name`.

## Built-in Commands

| Command | Aliases | Description |
|---------|---------|-------------|
| `/help` | `/h`, `/?` | Show available commands |
| `/config` | `/set` | View/edit configuration |
| `/model` | | Switch LLM model |
| `/clear` | | Clear conversation history |
| `/compact` | | Manually trigger context compaction |
| `/undo` | | Undo last change with diff preview |
| `/rewind` | | Restore conversation/code/state |
| `/commit` | | Generate git commit |
| `/diff` | | Show uncommitted changes |
| `/memory` | `/mem` | Manage persistent memory |
| `/session` | `/snap` | Save/load session templates |
| `/preset` | `/template` | Apply conversation presets |
| `/batch` | `/parallel` | Parallel worktree-isolated PR creation |
| `/pdf` | | Extract text from PDF files |
| `/context` | | Manage context window |
| `/quit` | `/q` | Exit |

## Command Types

- **Prompt commands** — Inject a prompt template for the LLM to follow
- **Immediate commands** — Execute immediately without LLM involvement
- **Workflow commands** — Multi-step orchestrated operations

## Custom Commands

Commands can also come from:
- **Plugins** — Registered via `PluginRegistry`
- **Skills** — Skill plugins register as executable slash commands
