# Shannon Desktop (Experimental)

**Status: Experimental — maintenance only, no active feature development.**

This Tauri-based desktop app is preserved for future evaluation but is not
under active development. The team is focused on the VS Code extension
(`editors/vscode/`) as the primary IDE integration path.

## Why Experimental

- All major competitors (Claude Code, Codex CLI, OpenCode) use Electron
- The VS Code extension provides a better IDE integration experience with
  less engineering effort
- Tauri requires significant additional work to reach feature parity

## Decision

- **Maintain**: Keep existing code compiling, fix regressions
- **No new features**: All feature work targets the VS Code extension
- **Revisit**: After VS Code extension is stable and has user adoption data

## When to Reconsider

- VS Code extension reaches stable release with positive adoption metrics
- User demand for standalone desktop app is validated
- Cross-platform native packaging becomes a differentiator
