# Testing

Shannon Code has 8,600+ tests across all crates.

## Running Tests

```bash
# All tests (use --test-threads=1)
cargo test --workspace -- --test-threads=1

# Specific crate
cargo test -p shannon-core -- --test-threads=1

# Single test
cargo test -p shannon-core test_query_engine -- --test-threads=1

# With output
cargo test -p shannon-core -- --test-threads=1 --nocapture
```

**Always use `--test-threads=1`** — some tests share environment variables and file paths.

## Test Organization

- **Unit tests**: `#[cfg(test)] mod tests` within source files
- **Integration tests**: `crates/shannon-*/tests/` directories
- **E2E tests**: `crates/shannon-cli/tests/cli_e2e_tests.rs` (some need API access)

## Test Helpers

| Helper | Purpose |
|--------|---------|
| `CollectingSender` | Progress sender for tool tests |
| `tempfile::TempDir` | Temporary directories for file tests |
| `mockito::Server` | HTTP mocking for API tests |

## HTTP Mocking

Use `mockito` for API integration tests:

```rust
let mut server = mockito::Server::new_async().await;
let mock = server.mock("POST", "/v1/messages")
    .with_status(200)
    .with_body(response_json)
    .create_async()
    .await;
```

Note: `mockito` server matchers are order-dependent with `.expect(N)`.

## Skipping E2E Tests

E2E tests that require API access can be skipped:

```bash
cargo test --workspace -- --test-threads=1 --skip test_long_conversation --skip test_multiturn
```

## Insta Snapshots

Some UI tests use `insta` for snapshot testing. Update snapshots with:

```bash
INSTA_UPDATE=always cargo test -p shannon-ui -- --test-threads=1
```
