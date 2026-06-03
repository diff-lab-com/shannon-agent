---
title: Testing
order: 11
section: development
---

# Testing

## Quick Start

```bash
# Install test runner
cargo install just cargo-nextest

# Run all tests (no API key needed)
just test

# Check + lint + test before commits
just dev
```

## Test Commands

| Command | What | Needs API key? |
|---------|------|----------------|
| `just test` | All unit + mock tests | No |
| `just scenarios` | YAML scenario tests | No |
| `just perf` | Performance regression | No |
| `just bench` | Criterion benchmarks | No |
| `just record` | Record real API fixtures | Yes |
| `just replay` | Replay recorded fixtures | No |
| `just ci` | Full CI suite | No |

## Test Types

### Unit Tests

Most crates use `#[cfg(test)] mod tests` within source files, keeping tests close to the code they test:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_validation() {
        let result = validate_token("test-token");
        assert!(result.is_ok());
    }
}
```

### Integration Tests

Cross-module tests live in `crates/shannon-*/tests/`:

- `api_integration.rs` — mockito HTTP tests
- `multi_turn_conversation.rs` — multi-turn mockito conversations
- `streaming_stress.rs` — streaming stress tests
- `snapshot_regression.rs` — insta snapshot tests

### Scenario Tests

YAML-driven declarative tests in `tests/scenarios/*.yaml`:

```yaml
name: basic_query
provider: mock
steps:
  - user: "Hello"
    expect_response_contains: ["Hello", "help"]
  - user: "Read src/main.rs"
    expect_tool_call: "read_file"
```

### Mock HTTP with Mockito

API tests use `mockito` for HTTP mocking:

```rust
use mockito::Server;

#[tokio::test]
async fn test_api_call() {
    let mut server = Server::new_async().await;
    let mock = server.mock("POST", "/v1/messages")
        .with_status(200)
        .with_body(mock_response)
        .expect(1)
        .create_async()
        .await;

    let client = LlmClient::new(config);
    let result = client.stream_message(messages).await;

    mock.assert_async().await;
    assert!(result.is_ok());
}
```

## Test Helpers

| Helper | Purpose |
|--------|---------|
| `CollectingSender` | Collects `ToolProgress` events |
| `tempfile::TempDir` | Temporary directories for file tests |
| `mockito::Server` | HTTP mock server |

## Record/Replay

Record real API interactions for CI:

```bash
# Record (needs API key)
SHANNON_API_KEY=sk-ant-... just record

# Replay (CI, no key needed)
just replay
```

## Performance Testing

Performance thresholds and E2E latency benchmarks run via `just perf`. Criterion benchmarks are in `crates/shannon-*/benches/`.

## nextest Configuration

Per-crate thread limits are configured in `.config/nextest.toml`. `shannon-core` and `shannon-commands` run single-threaded to avoid test interference.
