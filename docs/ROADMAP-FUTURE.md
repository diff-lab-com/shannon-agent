# Shannon Code - Future Roadmap (P1-P3)

> Moved from ROADMAP.md on 2026-04-16
> These items are deferred for future consideration after P0 is complete.

---

## P1 - High-Value Gaps (Future)

### Auto-commit with Contextual Messages
Automatically create a git commit after each AI edit with a descriptive, context-aware commit message.
- **Inspired by**: Aider's auto-commit every edit
- **Effort**: Medium (wire into tool execution pipeline)

### Undo/Snapshot System
Snapshot-based undo/redo of AI edits. Shannon has `/rewind` (message-level) but not file-state snapshots.
- **Inspired by**: Open Code's snapshot undo/redo
- **Effort**: Medium (git-based snapshots)

### Repository Map (tree-sitter)
Build a concise map of the entire git repo showing classes, functions, types, and call signatures.
Send to the LLM with each request so it understands the full codebase.
- **Inspired by**: Aider's repo map feature
- **Effort**: Large (new crate dependency, multi-language grammar support)

### Auto-test Loop
Edit files, run tests, fix failures, repeat until passing.
- **Inspired by**: Aider's auto-test feedback loop
- **Effort**: Medium (new loop in query engine)

### Deep LSP Integration
Auto-discover and auto-install LSP servers for code intelligence (go-to-def, references, rename).
- **Inspired by**: Open Code's 25+ LSP servers with auto-install
- **Effort**: Large (LSP client implementation, server management)

### IDE Extensions (VS Code / JetBrains)
Official IDE extensions for seamless integration.
- **Inspired by**: Claude Code's VS Code and JetBrains extensions
- **Effort**: Very Large (separate extension projects)

---

## P2 - Differentiators (Future)

### HTTP API Server (`shannon serve`)
Run Shannon as an HTTP API server for programmatic access.
- **Inspired by**: Open Code's `opencode serve`
- **Effort**: Large (new HTTP layer, auth, session management)

### Architect Mode (Two-model Collaboration)
Use a "planner" model (Opus) to design changes and a separate "editor" model (Sonnet) to implement.
- **Inspired by**: Aider's architect mode
- **Effort**: Large (dual-model orchestration)

### Session Export (`/export`)
Export conversations to markdown/JSON.
- **Status**: Dead code exists in `crates/shannon-commands/src/builtin/export.rs`
- **Effort**: Low

### PDF Processing (`/pdf`)
PDF text extraction, table extraction, OCR.
- **Status**: Dead code exists in `crates/shannon-commands/src/builtin/pdf.rs`
- **Effort**: High (external PDF library)

### Debug Instrumentation (`/debug`)
Runtime log level switching, profiling.
- **Status**: Dead code exists in `crates/shannon-commands/src/builtin/debug.rs`
- **Effort**: Low

### Diff Review (`/diff`)
Intelligent diff viewer with change categorization.
- **Status**: Dead code exists in `crates/shannon-commands/src/builtin/diff.rs`
- **Effort**: Medium

### PR Review (`/review_pr`)
AI-powered PR review with severity-based feedback.
- **Status**: Dead code exists in `crates/shannon-commands/src/builtin/review_pr.rs`
- **Effort**: Medium

---

## P3 - Long-term (Future)

### Cross-surface Continuity
Seamless sessions across terminal, VS Code, JetBrains, web, and mobile.
- **Effort**: Very Large (requires server infrastructure, multiple frontends)

### Cloud Execution Infrastructure
Run tasks in isolated cloud containers with two-phase runtime.
- **Inspired by**: Codex CLI's Cloud Codex, Cursor's background agents
- **Effort**: Very Large (infrastructure, container orchestration)

### Agent SDK
Build fully custom agents powered by Shannon's tools.
- **Inspired by**: Claude Code's Agent SDK (Python/TypeScript)
- **Effort**: Large (SDK design, language bindings)

### Skills Marketplace
Third-party skill distribution with central registry.
- **Effort**: Very Large (registry infrastructure, packaging, security review)

### Voice Input
Speech-to-text using whisper-rs or system whisper CLI.
- **Effort**: Medium (audio capture + whisper integration)

---

## Module-Level Roadmap (Pre-existing)

The following modules have dead code that should be wired up:

| Module | File | Dead Code | Priority |
|--------|------|-----------|----------|
| `/diff` | `shannon-commands/src/builtin/diff.rs` | ChangeCategory, DiffAnalysis | P2 |
| `/review_pr` | `shannon-commands/src/builtin/review_pr.rs` | ReviewSeverity, PRAnalysis | P2 |
| `/export` | `shannon-commands/src/builtin/export.rs` | ExportFormat, export_to_markdown/json | P2 |
| `/pdf` | `shannon-commands/src/builtin/pdf.rs` | PdfTable, ImageFormat | P2 |
| `/debug` | `shannon-commands/src/builtin/debug.rs` | DebugCategory, LogLevel | P2 |
| Agent Coordinator | `shannon-agents/src/coordinator.rs` | AgentTeam, task assignment | P3 |
| Compact Strategies | `shannon-core/src/compact.rs` | All 5 strategies defined, not wired | P0 (item 3) |
| Doctor Command | `shannon-core/src/doctor.rs` | DoctorError variants | P3 |
| UI Adapter | `shannon-core/src/ui_adapter.rs` | UiAdapter trait | P3 |
