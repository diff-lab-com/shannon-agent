# Wave-7: Memory Curated-Layer → JSONL-Append + Scoped Injection

**ADR**: [0010 — Memory Curated-Layer Alignment to JSONL-Append + Scoped Injection](../adr/0010-memory-curated-layer-jsonl-injection.md) (Accepted)
**Branch**: `feat/w7-adr0010-memory-jsonl`
**Status**: C2'-C5' landed (ADR Accepted); C6' gated on one release cycle.

## Goal

Align Shannon's curated memory layer — the **one** structured store that is
not JSONL — to the append-only + inject-into-context model the rest of
Shannon (transcripts / analytics / scheduled-runs / recordings) and Claude
Code's curated `memory/` layer already use. See ADR-0010 for the decision,
competitor evidence, and rejected alternatives (markdown, SQLite).

## Phases (one PR each)

### C2' — Format + migration + flock

- `MemoryStore`: `<project_hash>.json` full-rewrite → `<project_hash>.jsonl`
  **append-only** (one `MemoryEntry` per line).
- **One-shot migration**: on load, if `<hash>.json` exists and `<hash>.jsonl`
  does not, rewrite the JSON array as JSONL, keep the `.json` for the grace
  period.
- **Read path** tolerates a stale `.json` (one release) so a downgrade does
  not lose data.
- **Crash safety**: load skips + logs a trailing partial line rather than
  failing the whole store.
- **flock + atomic temp-rename** for the only rewriter (compaction). This
  folds in the "B2 stopgap" work — no separate throwaway flock on the old
  JSON store.
- **Verify**: round-trip (add → restart → entries persist); two threads
  appending concurrently lose nothing; a sample `.json` migrates correctly;
  a deliberately truncated last line is skipped on load.

### C3' — Retrieval flip (delete `search()`, add scoped injection)

- Delete `MemoryStore::search()` (the Jaccard tokenizer/scoring path).
- At query-engine launch, load the active project's memories into the
  system prompt — `content` field, grouped by `MemoryCategory`, short
  header (same shape as Claude Code's `MEMORY.md` index).
- **Verify**: a memory phrased differently from the query still surfaces
  (recall = 100%); insta snapshot of the injected prompt shape; net line
  count drops.

### C4' — Write-time dedup — ✅ landed

- `MemoryStore::add_or_update`: before append, token-overlap (Jaccard ≥ 0.8)
  check vs same-`category` entries; on match, reuse the id and update in place
  (newer content, max confidence, union tags, earliest `created_at`). The old
  JSONL line becomes a stale duplicate reclaimed by the next compaction.
- Production write paths switched to it: AutoDream `process_conversation`,
  `/remember`, REPL auto-memory. `add()` stays raw for tests + consolidator.
- **Verified**: appending a near-duplicate updates (`AddOutcome::Updated`)
  rather than duplicates; the stale line is reclaimed on the next `save()`.

### C5' — AutoDream periodic trigger + size control — ✅ landed

- `MemoryCompactionTrigger` + `AutoDreamService::maybe_compact`, wired into
  the engine post-query path. Schedule persisted in a sidecar
  (`compaction-state.json`) because `AutoDreamService` is recreated per query.
  Compacts at ~24 h wall-clock **or** ≥5 sessions (each query = 1 session):
  dedupe + stale + per-category caps (rule-based `MemoryConsolidator`),
  reclaim stale C4' lines, then token-budget prune.
- **Size control**: `prune_to_token_budget` shrinks the injected prompt to a
  ~2000-token budget, lowest-confidence / oldest-accessed first.
- **Multi-agent-safe compaction**: `save()` reloads disk under flock and
  reconciles — other agents' concurrent appends are preserved, deliberately
  deleted ids are not resurrected.
- **Verified**: trigger fires on the session/age threshold; compaction
  reduces count without losing high-confidence facts; injected prompt stays
  under budget; reconcile preserves another agent's append.
- **Deferred**: relative→absolute date resolution (see ADR-0010 D5
  refinement) — fiddly/low-value for ML-extracted content; revisit if
  observed in practice.

### C6' — Old `<hash>.json` read-compat removal

- After one release grace period, drop the `.json` read path and migration.
- **Verify**: no `.json` references remain in `crates/shannon-core/src/memory/`;
  full test suite passes.

## Out of scope

- **Agent-scoped memories** — Open Question in ADR-0010; layer later as
  `<hash>.<agent_id>.jsonl` alongside the shared file if a workflow needs
  private agent memory.
- **SQLite / embeddings** — rejected (ADR-0010 Alt B); revisit only if a
  single project exceeds ~500 curated entries **and** injection recall is
  reported insufficient.
- **Codebase RAG** — separate concern (vector stores belong here, not in
  curated memory).

## Sequencing

C2' → C3' → C4' and C5' (C5' depends on C2' + C4') → C6' (after one
release). Each phase is independently mergeable; C3' is the point where
the ADR moves Proposed → Accepted.
