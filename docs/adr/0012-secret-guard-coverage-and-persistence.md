# ADR-0012: Secret-Guard Coverage Boundaries & Persistence Posture

**Date**: 2026-09-25
**Status**: Accepted
**Sprint**: continuous (post-v0.11.0)

## Context

The secret-guard (blueprint artifact (c), `shannon-plugin-api` +
`shannon-core/src/secret_guard.rs`) reached Phase 2 productization in
PR #115, which fixed the repeat-occurrence redaction leak and the shape
regex false positives. The review that produced that PR surfaced a set of
open questions about **what the redact mode actually covers**, **where
secret values live at rest**, and **how the surrogate registry survives
restarts**. This ADR records the decisions made when closing those gaps
(follow-up PR implementing tasks T1–T6 of the 2026-09-25 review).

## Decisions

### D1 — Coverage: messages + system blocks + tool descriptions; never tool names or input schemas

| Surface | Transformed? | Source tag | Rationale |
|---|---|---|---|
| Conversation messages (user / tool_result / assistant text) | yes | `UserMessage` / `Other` | the main leak channel (file reads, shell output) |
| Structured system blocks (base prompt, CLAUDE.md / AGENTS.md / GEMINI.md, repo map, memory injection) | yes | `InjectedContext` | project instruction files are the most likely place users paste config with real keys; `audit_wire` already scanned them, so redact-mode coverage was inconsistent with audit |
| Plain-string system prompt fallback | yes | `InjectedContext` | same content, non-structured providers |
| Tool **descriptions** | yes | `Other` | free-form prose; may quote config examples |
| Tool **names** and **`input_schema`** | **never** | — | the model must reproduce names and parameter shapes byte-exactly for calls to parse; rewriting them breaks every invocation. Nested schema descriptions are accepted residual exposure (rare in practice) |

All transforms ride the deterministic (I1) HMAC surrogate derivation, so
the cached stable prefix (system blocks + tool defs) stays byte-stable
across turns — provider prompt caching is unaffected.

### D2 — Enablement: env overrides config; a present-but-unparseable env var means explicit off

`SHANNON_SECRET_GUARD` decides when set — `audit` / `redact` enable,
anything else (including `"off"`) disables **even if** the config file
says otherwise. Only when the env var is absent does the `[secret_guard]`
section of `~/.shannon/config.toml` (overridable by `.shannon.toml`)
apply. The section is parsed independently of the merged `ShannonConfig`
so the process-wide one-shot init in `process_query` covers every host
(CLI / desktop / server) without touching the 55+ config-literal call
sites. Locked as the pure function `secret_guard::resolve_mode`.

### D3 — Persistence: no surrogate-store file; rebuild from restored raw history

The surrogate registry stays **in-memory, per process**. Writing a
persistent `(surrogate → secret)` map would centralize plaintext secrets
in a new at-rest location, contradicting the "disk is clean" posture for
the L0 log without removing any existing exposure.

This is safe because the derivation is deterministic
(`SG1:` = HMAC(master_key, secret)): the first send after a restart
re-detects the raw secrets that are still present in conversation history
and re-registers **identical** mappings, so the wire and restore behavior
are unchanged across restarts. In addition, `QueryEngine::restore_messages`
runs a rebuild scan over the restored raw history so pre-send restores
(tool arguments, display) work before the first send of the new process.

Accepted residual gaps (surfaced, not silent — see D4):

- a secret whose raw text was dropped from history (compaction/truncation)
  while a model-echoed surrogate survives cannot be restored;
- deleting/corrupting `secret_guard.key` regenerates all surrogates —
  warned loudly at regeneration; old persisted history keeps unrestorable
  tokens (visibly marked per D4) and provider prompt caches miss once.

### D4 — Failure surfacing: unresolved placeholders are visible, never silent

Contract F3/F4 is implemented on both restore faces:

- **Execution face** — `restore_tool_args_for_execution` returns the
  unresolved token list; `ToolRegistry` appends a visible
  `[secret-guard] warning: …` line to the tool **output** (the input is
  never rewritten — I2). Not flagged as a tool error: the tool ran.
- **Display face** — `HostSecretGuard::restore_text` wraps any remaining
  surrogate-shaped token as
  `[secret-guard: unresolved placeholder SG1:…]` in the emitted copy
  only. History keeps the bare token.

### D5 — Streaming: display-face restore runs through a carry buffer

Per-delta restore cannot match a surrogate token split across deltas.
`secret_guard::DisplayRestorer` holds back only a possible partial token
(complete tokens restore immediately; `finish` flushes at stream end) and
is wired for the visible-text stream **and** the thinking stream. Thinking
content is display face — restored values are user-visible there too.

## Consequences

- Redact mode now covers every outbound prose surface; the only accepted
  plaintext channel is tool `input_schema` (documented, low-risk).
- No new secret-at-rest location was introduced; session files holding
  raw conversation content remain the existing (deliberate) boundary —
  the L0 log stays redacted at write time.
- Restart behavior: identical wire, restore works after the rebuild scan,
  unrebuildable tokens are visibly marked.
- Cost: one extra history scan per session restore and a small per-delta
  hold-back buffer; both measured inside the existing soft budgets
  (`full_history_retransform_is_byte_stable_and_within_budget`).

## References

- `docs/research/llm-secret-redaction-research-2026-09.md` (blueprint §9 wiring points, F3/F4)
- `crates/shannon-plugin-api/src/lib.rs` (contract invariants I1–I4)
- `crates/shannon-core/src/secret_guard.rs` (host implementation + wiring points 1/1b/1c/2/3)
- PR #115 (repeat-leak + word-boundary fixes, precedent for this ADR's task list)
