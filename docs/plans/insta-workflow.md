# insta snapshot workflow (P2-3)

Goal: lock down chat-rendering, command output, and MCP tool responses so silent
behavior changes surface as a reviewable diff instead of a flaky test.

## Tooling

- `insta = "1.48"` (pinned in `[workspace.dependencies]`; crates reference
  `insta = { workspace = true }`).
- `cargo-insta` is the CLI used to review pending `.snap.new` files. Install
  with `cargo install cargo-insta --locked`.

## Authoring snapshots

1. Pick the smallest unit that proves behavior — for selector output a single
   `SelectorOutcome` round-trip, for an MCP tool a single JSON-RPC response.
2. Use `insta::assert_snapshot!(name, value)` for free-form text or
   `assert_yaml_snapshot!` / `assert_json_snapshot!` for structured data.
3. Keep snapshots deterministic: sort keys / message lists, normalize
   timestamps to RFC3339, freeze Uuid v4 to `"00000000-…-000000000000"`.
4. Commit the resulting `.snap` file alongside the test.

## Reviewing changes

```bash
cargo nextest run -p <crate> <test_name>
cargo insta review                       # interactive
cargo insta accept --workspace           # bulk accept after a known-good refactor
cargo insta reject                       # drop pending snapshots
```

`cargo insta pending-snapshots` lists files without accepting them — useful in
CI logs.

## CI contract

`.github/workflows/ci.yml` runs the new `insta` job which fails the build when
`.snap.new` files are present (no silent acceptance). To update snapshots in a
PR: review locally, commit the `.snap` files, push.

## When not to snapshot

- Pure whitespace reformatting (run `cargo fmt` instead).
- Tests that already cover the same surface with stronger assertions — prefer
  the assertion.
- Anything time- or locale-sensitive without normalization.