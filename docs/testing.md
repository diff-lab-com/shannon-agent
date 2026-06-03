# Testing Guide

## Quick Commands

```bash
just test          # All unit + mock tests (no API key needed)
just dev           # check + lint + test (run before commits)
just ci            # Full CI suite (tests + doctests + clippy)
just scenarios     # YAML declarative scenario tests
just bench         # Criterion benchmarks
just perf          # Performance threshold tests
just record        # Record real API fixtures (needs API key)
just replay        # Replay recorded fixtures (no key needed)
```

Install tooling: `cargo install just cargo-nextest`

## Test Architecture

### Unit Tests

Most tests live in `#[cfg(test)] mod tests` within the source file they test. This keeps tests close to the code and allows access to private functions.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        // ...
    }
}
```

### Integration Tests

Cross-module tests go in `crates/<crate>/tests/`:

| Crate | Test Files | Purpose |
|-------|-----------|---------|
| `shannon-core` | `api_integration.rs`, `multi_turn_conversation.rs`, `streaming_stress.rs`, `snapshot_regression.rs`, `perf_tests.rs`, `scenario_tests.rs` | API mocking, streaming, performance |
| `shannon-cli` | `cli_args_tests.rs`, `cli_interactive_tests.rs`, `cli_e2e_tests.rs`, `cli_mock_tests.rs`, `live_tests.rs` | CLI argument parsing, provider mocks |
| `shannon-agents` | `e2e_team_lifecycle_tests.rs` | Team coordination, task management |

### Test Helpers

Common test utilities available across crates:

- **`CollectingSender`** — Collects `ToolProgress` events for assertion
- **`tempfile::TempDir`** — Temporary directories for file tests
- **`mockito::Server`** — HTTP mocking for API integration tests

### HTTP Mocking with Mockito

All API integration tests use `mockito` to mock HTTP responses. Never hit real APIs in tests.

```rust
let mut server = mockito::Server::new_async().await;
let mock = server.mock("POST", "/v1/messages")
    .match_header("content-type", "application/json")
    .with_status(200)
    .with_body_from_file("fixtures/response.json")
    .expect(1)
    .create_async()
    .await;
```

**Note**: `mockito` matchers are order-dependent when using `.expect(N)`.

## YAML Scenario Tests

Declarative test scenarios in `tests/scenarios/*.yaml`:

```yaml
name: "Basic file creation"
steps:
  - prompt: "Create a file hello.txt with content 'world'"
    expect_tool: "Write"
    expect_file: "hello.txt"
    expect_content: "world"
```

Run with `just scenarios`.

## Record/Replay System

For testing with realistic API responses:

### Recording (needs API key)

```bash
SHANNON_API_KEY=sk-ant-... just record
SHANNON_RECORD_PROVIDER=deepseek just record-deepseek
```

Fixtures are saved as JSONL files in `tests/fixtures/`.

### Replaying (no key needed)

```bash
just replay
```

Replays recorded fixtures through mockito mocks.

## Performance Testing

### Threshold Tests

Run with `just perf`. Tests that verify operations complete within acceptable time bounds:
- Compaction (100 turns)
- Session load
- Tool chain execution
- Streaming parse
- Token estimation
- Cache hit rate

### Benchmarks

Run with `just bench`. Uses `criterion` for micro-benchmarks:
- Compact engine
- File edit operations
- Repomap generation
- Context budget calculation

Regression thresholds: noise 3%, confidence 98%.

## Test Requirements

- Every `src/**/*.rs` file must have at least one `#[test]`
- New features must include tests
- Bug fixes should include a regression test
- `just dev` must pass before committing

## nextest Configuration

Config in `.config/nextest.toml`:
- `shannon-core` and `shannon-commands` run single-threaded to avoid env contention
- All other crates run with default parallelism

Fallback without nextest: `cargo test --workspace -- --test-threads=1`
