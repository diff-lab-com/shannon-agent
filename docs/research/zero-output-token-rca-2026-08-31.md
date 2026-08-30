# Zero-output-token failures — RCA (2026-08-31)

## TL;DR

**3 / 50 SWE-bench batch-3 tasks (6%) ended with `tokens_in < 100K, tokens_out < 100, resolved=False`** despite the agent exiting cleanly (`rc=0`, `turn/end reason=completed`).

Root cause: **query_engine early-bail at `engine.rs:3793` fires after a single malformed tool-call JSON parse error**, surfacing the parse error back to the model but never starting a second turn. The agent produces 1-2 `assistant/chunk` thinking deltas, the model's tool_use arrives with non-parseable args, the framework emits a `tool/result` with `"Failed to parse tool arguments"` — and then exits the conversation as if the model had produced nothing.

## Affected runs

| Task | model | provider | app_version | tokens_in | tokens_out | first tool | error |
|---|---|---|---|---|---|---|---|
| `matplotlib__matplotlib-23314` | MiniMax-M3 | minimax | 0.11.0 | 35,754 | 62 | Bash | `EOF while parsing a string at line 1 column 43` |
| `sympy__sympy-12481` | MiniMax-M3 | minimax | 0.11.0 | 39,726 | 84 | Bash | `EOF while parsing a string at line 1 column 29` |
| `django__django-10914` | MiniMax-M3 | minimax | 0.11.0 | 66,895 | 89 | Grep | `EOF while parsing an object at line 1 column 1` |

All 3 share the pattern: **first turn, first tool call, malformed JSON, agent exits**. The `tool_use_id` (`chatcmpl-tool-XXXX`) is the OpenAI-compatible ID format, so the bug is upstream of the provider adapter — it's the engine's bail-out logic.

## What the events.jsonl shows

```
session/start           → seq=1
user/message            → the issue text
turn/start              → query_id=...
request/header          → wire_body (the full prompt)
assistant/chunk × 5–15  → ONLY `<think>...</think>` text. No tool_call content.
tool/result             → is_error=True, "Failed to parse tool arguments: EOF..."
turn/end                → reason=completed, usage={in: 35K, out: 62}
```

The model emits thinking then a tool_use whose `input` JSON is truncated mid-string (the parser hits EOF before the closing quote). The framework captures the tool_use_id + name, tries `serde_json::from_str(&raw)` on the accumulated `InputJsonDelta`, fails, and pushes a synthetic tool_result with the parse failure.

**Then**: the bail-out at `engine.rs:3793` triggers because:
- `has_content` is `false` (no text delta, only thinking chunks — `assistant_text` stays empty)
- `tool_inputs` is empty (only valid JSON parses push to `tool_inputs`)
- `phase != Finalized`

So the engine returns `QueryEvent::Completed` and exits. The model never sees its own parse-error feedback, never gets to retry.

## The bail-out condition (engine.rs:3793)

```rust
if !has_content
    && tool_inputs.is_empty()
    && phase != StreamingPhase::Finalized
{
    // emit Cost, ConversationUpdate, Completed
    return;
}
```

The condition checks "did the model produce text? did it produce a valid tool call?" — but **does not check** "did the framework synthesize a tool result?" When JSON parse fails:
- `assistant_text` stays empty (`has_content = false`)
- `tool_inputs` stays empty
- `assistant_tool_uses` gets 1 entry: `ContentBlock::ToolUse { id: "", name, input: Null }` (engine.rs:2302)
- `tool_results` gets 1 entry: the parse-error message (engine.rs:2294)

The framework has *something* to send back to the model (the assistant's null-input tool_use + the synthetic tool_result). The bail-out prevents that round-trip.

## Fix

One-line change: bail out only if **no assistant content, no successful tool inputs, AND no assistant tool_use blocks at all** (including the null-input blocks that come from parse-error recovery).

```rust
if !has_content
    && tool_inputs.is_empty()
    && assistant_tool_uses.is_empty()   // ← NEW
    && phase != StreamingPhase::Finalized
{
    // ...
}
```

After the fix:
- Parse-error path: `assistant_tool_uses` is non-empty → no bail → loop continues → framework sends assistant turn (with null input) + tool_result (with parse error) → model retries with valid JSON.
- Empty-response path (no chunks at all): all three are empty → bail (same as before; correct behavior).

## Test plan

A unit test in `crates/shannon-core/src/query_engine/` that simulates the parse-error stream:

1. Build a `MessageStream` that emits `ContentBlockStart{tool_use}` followed by `ContentBlockStop` with a tool_use whose `input` is malformed JSON.
2. Drive `process_streaming_response` (or whatever the entry point is).
3. Assert that the loop continues past the parse error (does NOT emit `QueryEvent::Completed` after the first turn).
4. Assert that the second turn's `request/header` `wire_body` contains the tool_result with the parse error message.

## Recommendation for batch-4

- Land the one-line fix + test in a small PR before any new batch run.
- Add a metric `tool_parse_errors` to `eval_metrics.rs::TaskMetrics` so this failure mode is observable (currently invisible: it looks like a normal successful run with tiny token count).
- Re-run the 3 affected tasks (matplotlib-23314, sympy-12481, django-10914) with the fix; expect them to produce patches.

## Why this matters beyond batch-3

- 6% silent task loss = ~3 tasks per 50 = ~6% undercount on the **headline pass rate** (48.0% → ~54% with fix).
- The bail-out pattern (`!has_content && tool_inputs.is_empty()`) is the only thing protecting against runaway empty-response loops; any future provider that emits non-content deltas would hit the same bug.
- The bug is **provider-agnostic** in code path — it's the engine's bail-out that fires regardless of which adapter produced the malformed tool_use.

## Anchors

- Code: `crates/shannon-core/src/query_engine/engine.rs:3793` (bail-out), `engine.rs:2275` (parse-error recovery), `engine.rs:2302` (null-input push to `assistant_tool_uses`)
- Affected events: `~/.shannon/eval/swe50-n3/rep-{3,17}-{matplotlib__matplotlib-23314,sympy__sympy-12481,django__django-10914}/shannon-home/sessions/<uuid>/events.jsonl`
- Batch-3 ledger: `~/.shannon/eval/swe50-n3/results.tsv` (rows for the 3 tasks above)