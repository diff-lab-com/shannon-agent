# Fix report — `api_error:l2-deep-analysis:Schema validation failed: Invalid JSON in response: EOF while parsing a value at`

## Signature

```
Schema validation failed: Invalid JSON in response: EOF while parsing a value at line 1 column 0
```

Task `l2-deep-analysis` (26 turns, 2.5M tokens in) ended with exit code 1 **even though the
model's final turn contained a complete, valid, fenced JSON object** matching the requested
`--schema`.

## Root cause

Two bugs compounded; the failure needs both.

**1. Adapter drops `delta.content` on chunks that also carry `tool_calls`**
(`crates/shannon-engine/src/api/adapter.rs`, `normalize_openai_event`).

Wire fixture `record/minimax_b4a41957c1d58a26.json` shows MiniMax-M3 gluing the reasoning
close tag onto the same delta that opens the tool call:

```json
{"delta":{"content":"</think>\n\n","role":"assistant","tool_calls":[{"id":"call_01a02a9c...","function":{"name":"Read",...},"index":0}]}}
```

The tool_calls branch populated the event vector and returned early
(`if !events.is_empty() { return events; }`), never emitting the chunk's `content`. The
assembled text stream therefore opened `<think>` (turn 1) with **no `</think>` anywhere**
— verified against the reconstructed NDJSON text: 1 open, 0 closes. A previous fix had
already made `content` non-exclusive with `finish_reason`; the `tool_calls` case was missed.

**2. Headless `--schema` validation ran on the whole conversation transcript**
(`crates/shannon-cli/src/main.rs`, `run_headless_query`).

`response_text` accumulated `QueryEvent::Text` from **every** turn (15854 chars; the final
answer is only 5487). `StructuredOutputConfig::validate_response` → `strip_reasoning_blocks`
saw the unclosed `<think>` from turn 1 and truncated at position 0 → empty string →
`serde_json::from_str("")` → `EOF while parsing a value at line 1 column 0` → exit 1.

Either bug alone was masked: with the close tag intact, the last-balanced-JSON fallback in
`output_format.rs` rescued the glued transcript; with final-turn-only validation, the
unclosed `<think>` never reached the validator. Together they produced the observed hard
failure.

## What changed

- `crates/shannon-engine/src/api/adapter.rs` — `normalize_openai_event` now emits
  `choice.delta.content` **before** the tool_calls/finish_reason handling (non-exclusive
  with both). The duplicate emission formerly inside the finish_reason branch and the
  now-dead tail fallback were removed. The engine's `ContentBlockDelta`→`QueryEvent::Text`
  path is index-agnostic, so a text delta sharing a chunk with tool blocks is safe.
- `crates/shannon-cli/src/main.rs` — `run_headless_query` clears `response_text` on
  `ToolUseRequest`: a tool call supersedes the preceding text as the turn's contribution,
  so schema validation and the `response` output field see only the final answer turn.
  NDJSON `text_delta` events still stream all text from every turn.

No public API changes; no new dependencies.

## Regression tests

- `crates/shannon-engine/src/api/adapter.rs` —
  `test_minimax_think_close_on_tool_call_chunk_survives`: exact fixture shape
  (`"content":"</think>\n\n","tool_calls":[...]`); asserts the close tag reaches the
  assembled text **and** the tool call still opens. Fails pre-fix with
  `left: "<think>Planning the search."` (close dropped).
- `crates/shannon-cli/tests/cli_mock_tests.rs` —
  `schema_minimax_think_close_on_tool_chunk_validates`: two-turn mockito replay of the
  dogfood run (reasoning+tool turn, then fenced-JSON answer) under `--schema`. With both
  fixes reverted it fails with the **exact** signature error (`EOF while parsing a value
  at line 1 column 0`); passes with either fix, so it guards the end-to-end signature.
  `schema_validation_targets_final_turn_not_transcript`: plain preamble + tool turn, then
  fenced JSON; asserts validation targets the final answer only and `response` excludes
  intermediate-turn prose. Fails pre-CLI-fix with `Schema validation failed: Invalid JSON
  in response: expected value at line 1 column 1`.

Deliverable note: instead of copying a wire fixture under `tests/fixtures/real_tasks/`,
the tests replay the recorded provider shapes through mockito (deterministic, keyless,
runs in CI) — the SSE bodies are transcribed from the actual fixtures.

## How verified

- Pre-fix failure modes confirmed by reverting each fix individually (see above).
- `cargo nextest run -p shannon-engine` — 1138 passed.
- `cargo nextest run -p shannon-cli` — 398 passed (includes the new tests).
- `cargo clippy --workspace -- -D warnings …` (exact CI allowlist) — clean.
- `cargo fmt --all -- --check` — clean.
