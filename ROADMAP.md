# Shannon Code - Future Roadmap

Features deferred from the 2026-04 competitive gap analysis for future implementation.

## P0 - High Priority (Future)

### Repository Map (Tree-sitter)
Build a concise map of the entire git repo showing classes, functions, types, and call signatures.
Send to the LLM with each request so it understands the full codebase without reading every file.
Uses `tree-sitter` crate for multi-language parsing.
- **Inspired by**: Aider's repo map feature
- **Effort**: Large (new crate dependency, multi-language grammar support)

## P1 - Medium Priority (Future)

### Auto-commit with Contextual Messages
Automatically create a git commit after each AI edit with a descriptive, context-aware commit message.
Provides clean, granular, reviewable git history by default.
- **Inspired by**: Aider's auto-commit every edit
- **Effort**: Medium (wire into tool execution pipeline)

### Auto-test Loop
Edit files, run tests, fix failures, repeat until passing.
Configurable test command with `--auto-test --test-cmd "npm test"`.
- **Inspired by**: Aider's auto-test feedback loop
- **Effort**: Medium (new loop in query engine)

### Architect Mode (Two-model Collaboration)
Use a "planner" model (Opus) to design changes and a separate "editor" model (Sonnet) to implement them.
Improves accuracy on complex tasks by separating planning from execution.
- **Inspired by**: Aider's architect mode
- **Effort**: Large (dual-model orchestration)

## P2 - Nice-to-have (Future)

### HTTP API Server Mode
Run Shannon as an HTTP API server (`shannon serve`) for programmatic access.
Enables CI/CD integration, editor integration, and automated workflows.
- **Inspired by**: OpenCode's `opencode serve`
- **Effort**: Large (new HTTP layer, auth, session management)

### Voice Input Support
Speak coding requests aloud using whisper-rs or system whisper CLI for speech-to-text.
- **Inspired by**: Aider's voice-to-code
- **Effort**: Medium (audio capture + whisper integration)

## P3 - Long-term (Future)

### Cross-surface Continuity
Seamless sessions across terminal, VS Code, JetBrains, web, and mobile.
Start in terminal, continue on web, pick up on phone.
- **Inspired by**: Claude Code's cross-surface continuity
- **Effort**: Very Large (requires server infrastructure, multiple frontends)

### Cloud Execution Infrastructure
Run tasks in isolated cloud containers with two-phase runtime (setup + agent).
- **Inspired by**: Codex CLI's Cloud Codex, Cursor's background agents
- **Effort**: Very Large (infrastructure, container orchestration)

### Agent SDK
Build fully custom agents powered by Shannon's tools with your own orchestration.
Programmable session management with auto-continue.
- **Inspired by**: Claude Code's Agent SDK (Python/TypeScript)
- **Effort**: Large (SDK design, language bindings)

### Skills Marketplace
Third-party skill distribution for community-built command packages.
Central registry with search, install, and update.
- **Inspired by**: Claude Code's skills ecosystem, Codex's skills marketplace
- **Effort**: Very Large (registry infrastructure, packaging, security review)
