---
title: Contributing
order: 40
section: development
---

# Contributing

## Prerequisites

- Rust 1.88+ (edition 2024)
- Git
- An LLM API key for testing (or use offline tests only)

## Development Workflow

1. Fork the repository
2. Create a feature branch from `dev`
3. Make changes with tests
4. Ensure `cargo clippy --workspace -- -D warnings` passes
5. Ensure `cargo test --workspace -- --test-threads=1` passes
6. Submit a pull request to `dev`

## Code Style

- Follow standard Rust conventions (`cargo fmt`)
- Use `thiserror` for library error types, `anyhow` for CLI/binary
- Production code: use `expect("reason")` over `unwrap()`
- Add `#[cfg(test)] mod tests` within source files
- Keep tests near the code they test

## Branch Strategy

- `main` — Stable releases
- `dev` — Active development (PR target)
- `feat/*` — Feature branches

## Commit Messages

Use conventional commit format:
```
feat: add tool result caching
fix: resolve context overflow on large files
docs: update architecture diagram
test: add integration tests for MCP protocol
refactor: simplify permission classifier
```
