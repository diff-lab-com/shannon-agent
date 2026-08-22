# Dogfood fix report — iter-01

**Signature:** `outcome_fail:l1-bulk-migrate:artifact:src/people/customer.rs,artifact:src/sales/pricing.rs,…`

## Root cause

The `l1-bulk-migrate` run (session `5d4ef25b`, 6 turns, exit 0, `cargo test` green)
migrated nothing: all four expected artifacts were missing. The wire fixtures under
`record/minimax_d388385b5f4938fd.json` show why the session ended after six turns
having only ever called Read/Bash:

1. Shannon sent `max_completion_tokens: 4096` (the `LlmClientConfig` default — no
   user/provider override was configured).
2. MiniMax-M3 streams its reasoning as `<think>…</think>` **inside `content`**. On
   the final request the model spent the entire budget there: the usage frame
   reports `completion_tokens: 4096, reasoning_tokens: 4095` — zero visible answer.
3. The stream closed with `finish_reason: "length"` (truncation), no tool calls.
4. The engine's agent loop matched the no-tool-use branch of the `MessageDelta`
   handler and finalized the query as a **normal completion** — `Completed`, exit 0.
   `stop_reason` was destructured away (`MessageDelta { usage, .. }`) and never
   consulted, so a truncated no-op message was indistinguishable from "the model
   is done".

So a mid-reasoning cutoff ended the whole headless run silently. The artifacts
were never written because the model never got another turn.

## What changed

`crates/shannon-core/src/query_engine/engine.rs` — truncation-aware turn
continuation:

- `is_truncation_stop()` helper: recognizes the output-limit stop reasons across
  wire formats (OpenAI-compatible `length`, Anthropic/Gemini `max_tokens`), plus
  the `TRUNCATION_CONTINUATION_PROMPT` re-prompt constant.
- The `MessageDelta` arm now latches `delta.stop_reason` per turn (providers split
  `finish_reason` and real usage across separate SSE frames, so the reason must
  survive the sentinel-usage deferral).
- No-tool-use finalize path: when the response was truncated, keep the partial
  assistant message, append the continuation re-prompt, emit a `Warning` +
  `ConversationUpdate`, and run another turn instead of completing the query.
- Same recovery in the post-stream safety net (providers whose stream ends
  without a usable usage frame).
- Bounded: `MAX_TRUNCATION_CONTINUATIONS = 5` per query **and** the existing
  `max_turns` cap (`turn` is incremented per continuation), so a model that always
  overruns its output budget cannot monopolize the loop — the query then completes
  as before (visible, bounded truncation instead of a silent first-cut no-op).

Deliberately **not** changed: the 4096 default `max_tokens` and the per-provider
`default_max_tokens` knob. Raising the default only hides the defect for one
budget size and risks 400s against models with lower output ceilings; the robust
fix is making truncation recoverable regardless of cap size.

## How verified

- `crates/shannon-core/tests/query_engine_tool_use_tests.rs` — new
  `openai_truncation_continuation_tests` module replaying the recorded MiniMax-M3
  wire shape (think-content chunks → `finish_reason: "length"` → trailing usage
  frame → `[DONE]`):
  - `length_truncation_without_tool_calls_continues_instead_of_completing` — a
    second HTTP request must go out (mockito `expect(1)` on both), the query
    reaches `Completed` via the continuation, a truncation `Warning` is emitted,
    Cost totals cover both requests (700 in / 4216 out), and the conversation
    keeps the truncated reasoning, the re-prompt, and the final answer.
  - `perpetual_truncation_is_bounded_and_still_completes` — six consecutive
    truncated responses produce exactly six requests and a normal `Completed`.
- Unit test `truncation_stop_reason_detection` for the stop-reason classifier.
- `cargo nextest run -p shannon-core`: 3584 passed / 0 failed.
- `cargo nextest run --workspace`: full suite green.
- `cargo clippy -p shannon-core --all-targets`: warning count identical to the
  pre-change baseline (74, all pre-existing); `cargo fmt --all -- --check` clean.
