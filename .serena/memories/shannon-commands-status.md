# Shannon Commands Status

## Completed Features

### Core Command System
- `CommandRegistry`: Async command registration and lookup
- `CommandParser`: Nom-based parser for `/command args` syntax with flag support
- `CommandExecutor`: Permission-checked command execution
- `CommandContext`: Tool use context and execution environment

### Command Types
- `PromptCommand`: Commands that generate AI prompts
- `LocalCommand`: Local-only commands without AI
- `LocalJSXCommand`: Commands with rich TUI components

### Built-in Commands (6)
1. **/commit**: Git commit with safety protocols and AI message generation
2. **/review-pr**: Pull request review with structured categories
3. **/pdf**: PDF processing with OCR and extraction options
4. **/help**: Command help with categorized documentation
5. **/status**: Git status parsing and formatting
6. **/diff**: Git diff with scope options and statistics

### File Structure
```
crates/shannon-commands/
├── src/
│   ├── lib.rs           # Public API exports
│   ├── command.rs       # Core types (Command enum, CommandBase, errors)
│   ├── registry.rs      # CommandRegistry with async lookup
│   ├── parser.rs        # CommandParser using nom
│   ├── executor.rs      # CommandExecutor with permission checking
│   ├── context.rs       # CommandContext, ToolUseContext
│   ├── builtin.rs       # Built-in command registry
│   └── builtin/
│       ├── commit.rs     # /commit command with safety templates
│       ├── review_pr.rs  # /review-pr with review categories
│       ├── pdf.rs        # /pdf with PdfOptions, OCR support
│       ├── help.rs       # /help with HelpCategory system
│       ├── status.rs     # /status with GitStatusInfo parsing
│       └── diff.rs       # /diff with DiffOptions, DiffStats
└── Cargo.toml
```

## Key Design Decisions

### Async-First Architecture
- CommandRegistry uses async for lookup (supports remote command sources)
- CommandExecutor uses async for execution
- SharedExecutor wraps with Arc<RwLock<>> for concurrent access

### Type-Safe Command System
- Enum-based Command variants (Prompt, Local, LocalJSX)
- Strongly-typed options per command (DiffOptions, PdfOptions, etc.)
- Result types with CommandError for proper error handling

### Parser Combinator Pattern
- Uses `nom` for efficient parsing
- Supports flags, arguments, and multi-command parsing
- Regex-based stat parsing for git output

## Integration Points

### From QueryEngine
- Commands can be invoked via `/command` syntax in user messages
- CommandContext provides tool permissions and execution environment
- ExecutionResult can be Text, Skip, Compact, or Error

### Future Work
- Implement actual command execution (currently returns placeholder results)
- Add command discovery from external sources
- Implement command history and aliases
- Add command completion and validation
- Support for custom user commands

## Dependencies
- `nom`: Parser combinators
- `regex`: String pattern matching
- `once_cell`: Lazy static initialization
- `tokio`: Async runtime
- `serde`: Serialization support
- `thiserror`: Error derive macros
