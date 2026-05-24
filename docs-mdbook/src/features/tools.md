# Tool System

Shannon Code's tool system lets the LLM interact with your filesystem, execute commands, and search code.

## Core Tools

### Read
Read file contents with support for:
- Text files with line numbers
- Image files (PNG, JPG, GIF, WebP, BMP) → base64 encoding
- Progressive loading for large files (head/tail preservation)
- Offset/limit for reading specific sections

### Edit
String replacement with intelligent fallback:
1. Find `old_string` in file → replace with `new_string`
2. If not found, check git diff for recent changes
3. Attempt three-way merge (base = git HEAD, ours = current, theirs = intended edit)
4. If merge conflicts, report conflict markers for resolution

### Write
Create or overwrite files. Used for new file creation.

### Bash
Execute shell commands with:
- Real-time streaming output
- Working directory control
- Configurable timeout
- Environment variable injection

### Grep
Regex search across the project using ripgrep. Supports:
- Pattern syntax (regex, literal)
- File type filtering
- Context lines (-A, -B, -C)

### Glob
File pattern matching for finding files by name/path patterns.

## Advanced Tools

### Agent
Spawn sub-agents with per-agent configuration:
- Model override
- Tool restrictions
- Worktree isolation

### AnalyzeImage
Analyze images (file or URL) via LLM vision capabilities.

### MergeResolve
Resolve merge conflicts detected by the Edit tool's three-way merge.

## Tool Result Cache

Read-only tool results (Read, Glob, Grep) are cached with:
- TTL-based expiration (5 min default)
- Invalidation on file changes (via SourceWatcher)
- Concurrent access via DashMap
