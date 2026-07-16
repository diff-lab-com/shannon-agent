# Contributing to Shannon Code

Thank you for your interest in contributing! This guide covers the essentials.

## Quick Start

```bash
# Install tooling
cargo install just cargo-nextest

# Build
cargo build --workspace

# Run before every commit
just dev
```

`just dev` runs `cargo check`, `cargo clippy`, and `cargo nextest run` in sequence. All three must pass before committing.

## Branch Workflow

- **`main`** — Stable release branch. Only updated via PR from `dev`.
- **`dev`** — Active development branch. All feature branches merge here first.
- **Feature branches** — Create from `dev`: `git checkout -b feature/my-feature dev`

### PR Process

1. Create a feature branch from `dev`
2. Make changes, commit with clear messages
3. Run `just dev` to verify everything passes
4. Push and open a PR targeting `dev`
5. Wait for CI to pass before merging
6. Periodically, `dev` is merged to `main` via a PR

**Important**: Never push directly to `main`. Always use PRs. Never use `--admin` with `gh pr merge`.

## Commit Messages

- Use imperative mood: `add`, `fix`, `refactor`, `test`, `docs`
- Prefix with scope when helpful: `test: add unit tests for live_tests helpers`
- Keep the first line under 72 characters
- Reference issues when applicable: `fix: resolve cache miss on resume (#42)`

## Testing

### Running Tests

```bash
just test          # All unit + mock tests (no API key needed)
just ci            # Full CI suite (tests + doctests + clippy)
just dev           # check + lint + test (run before commits)
```

### Writing Tests

- Most tests go in `#[cfg(test)] mod tests` within the source file they test.
- Integration tests go in `crates/<crate>/tests/`.
- Use `mockito::Server` for HTTP API tests — never hit real APIs.
- Use `tempfile::TempDir` for file system tests.
- Read existing test files in the crate before writing new ones to match patterns.

### Test Requirements

- Every `src/**/*.rs` file must have at least one `#[test]`.
- New features must include tests.
- Bug fixes should include a regression test.

## Code Style

- Rust 1.88+ (edition 2024).
- `cargo clippy --workspace -- -D warnings` must pass with zero warnings.
- `cargo fmt --all -- --check` must pass.
- Use `thiserror` for library crate error types, `anyhow` for CLI/bin.
- Production code uses `expect("reason")` over `unwrap()`.

## Project Structure

```
crates/
├── shannon-core/      # API client, query engine, permissions, state
├── shannon-tools/     # Tool implementations
├── shannon-agents/    # Multi-agent orchestration
├── shannon-ui/        # Terminal UI (ratatui)
├── shannon-mcp/       # MCP protocol
├── shannon-commands/  # Slash commands
├── shannon-skills/    # Skill system
├── shannon-cli/       # CLI entry point
└── shannon-agent/     # Agent binary (JSON-RPC)
```

## Key Patterns

- **Multi-provider**: `LlmClient` normalizes Anthropic/OpenAI/Ollama via adapter pattern.
- **Streaming**: SSE byte stream → `SseStream` → `MessageStream`.
- **Config priority**: CLI args > env vars (`SHANNON_*`) > `.shannon.toml` > `~/.shannon/config.toml`.
- **Tool interface**: `Tool` trait in `shannon-tool-interface` with `execute()`, `is_read_only()`, etc.
- **MCP**: Configured in `.mcp.json` or `~/.claude/settings.json` via `mcpServers` key.

## Reporting Issues

- Use [GitHub Issues](https://github.com/shannon-agent/shannon-agent/issues).
- Include: Rust version (`rustc --version`), OS, steps to reproduce, expected vs actual behavior.
