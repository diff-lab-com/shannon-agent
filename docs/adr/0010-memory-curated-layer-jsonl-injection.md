# ADR 0010 — Memory Curated-Layer Alignment to JSONL-Append + Scoped Injection

**Date**: 2026-08-12
**Status**: Accepted
**Sprint**: Wave-7

## TL;DR

Shannon's *event-log* storage layer (transcripts, analytics, scheduled runs,
recordings) is **already** JSONL append-only — identical to Claude Code's
`~/.claude/projects` layer-1. The one storage layer that does **not** align
with the append-only/inject-into-context model is the **curated memory
layer** (`memory/store.rs`): it writes one `<project_hash>.json` per project
with a full read-modify-rewrite on every `save()` and retrieves via linear
Jaccard word-overlap `search()`.

This ADR aligns the curated memory layer to the same model the rest of
Shannon (and Claude Code's curated `memory/` layer) already uses:

- **Storage**: per-project `<hash>.json` full-rewrite → per-project
  `<hash>.jsonl` **append-only** (one `MemoryEntry` per line).
- **Retrieval**: delete the Jaccard `search()` path; instead **load the
  active project's memories into the system prompt at launch** (scoped
  injection), same model as Claude Code's `MEMORY.md`.
- **Write safety**: append-only eliminates the multi-agent `lost-update`
  race that full-rewrite has; a flock + atomic-temp-rename guards the
  low-frequency compaction pass that *does* rewrite.
- **Schema**: `MemoryEntry` is unchanged (serde round-trips losslessly
  between the old JSON array and the new JSONL — migration is a pure
  re-shuffle).

Rejected alternatives: **markdown per-file** (loses structured fields,
no reuse of Shannon's existing JSONL infrastructure), and **SQLite**
(rejected earlier in Wave-6 — Claude Code and Codex CLI both use flat
file storage + injection, never a DB, for curated memory).

## Context

### Shannon already speaks JSONL everywhere except curated memory

Four subsystems already persist append-only JSONL — this is the proven
in-repo pattern, not a new one:

- `crates/shannon-core/src/session_transcript.rs` — `TranscriptStore`,
  `~/.shannon/transcripts/<session_id>.jsonl` (one event per line)
- `crates/shannon-core/src/analytics.rs` — `<YYYY-MM-DD>.jsonl`
- `crates/shannon-core/src/scheduled_runs.rs` — `<MM>.jsonl`
- `crates/shannon-core/src/recording/recorder.rs` — `<session_id>.jsonl`

The curated-memory layer is the **only** structured store that did not get
this treatment. `memory/store.rs` instead does:

- `MemoryStore::save()` (`store.rs:136`-ish) — `serde_json::to_string_pretty`
  of the **entire** in-memory `HashMap` to `{storage_path}/{project_hash}.json`
  on **every** write (`store.rs:152`-ish).
- `MemoryStore::load()` (`store.rs:162`-ish) — reads **all** `*.json` files
  in the storage dir at startup.
- `MemoryStore::search()` (`store.rs:91`-ish) — linear scan, Jaccard
  word-level overlap between query and entry content/tags.

### Three problems with the current curated-memory layer

1. **Multi-agent write race (lost update).** Full-rewrite means two agents
   that `save()` concurrently each read the old file, mutate their own
   in-memory map, and the second to finish clobbers the first. Shannon now
   has real concurrent writers: `Team` teammates, `/batch` worktree agents,
   and sub-agents. The event-log layer doesn't have this problem because
   append-only writes don't invalidate each other.
2. **Jaccard word-overlap retrieval misses semantically-related memories.**
   A memory tagged "release" phrased as "version bump" will not match a
   query phrased "cut a release". Word-level Jaccard has no synonymy.
3. **Retrieval model mismatch with the reference implementation.** Claude
   Code's curated `memory/` does **not** retrieve by search — it injects
   the active scope's memory index into context and lets the model decide
   relevance. Shannon's search-then-inject-only-matches model is both
   slower and lower-recall than plain injection for the volumes a curated
   memory layer actually holds (tens of entries, not thousands).

### Competitor evidence (verified, not assumed)

Inspected `~/.claude/projects/<project>/` directly (2026-08-11). Claude
Code uses a **4-layer mixed** store, each layer picking its format by
lifecycle + write pattern:

| Layer | Carrier | Format | Write pattern |
|---|---|---|---|
| L1 transcript | `<sessionId>.jsonl` | JSONL | append-only |
| L2 tool results | `tool-results/<callId>.txt` | text | write-once spill |
| L3 subagent state | `subagents/agent-*.meta.json` | JSON | write-once |
| L4 curated memory | `memory/MEMORY.md` + `<slug>.md` | markdown + YAML frontmatter | read-modify-rewrite, **injected** |

The transferable principle is **not** "use markdown" — it is: *high-frequency
events ⇒ append-only log; low-frequency curated facts ⇒ structured store +
inject-into-context (delete the search path).* Codex CLI follows the same
principle (`AGENTS.md` static + `~/.codex/memories/` flat files, no DB).
Neither leader uses a database or embeddings for **curated** memory;
vector stores are reserved for codebase RAG, a different concern.

## Decision

### Decision 1 — Storage: per-project `<hash>.jsonl`, append-only

Replace `{storage_path}/{project_hash}.json` (full-rewrite of a JSON array)
with `{storage_path}/{project_hash}.jsonl` (one `MemoryEntry` per line).

- **Append** a single JSON line on `add()`/`update()`. No read-before-write
  of the whole file.
- **Load** at startup = read the file line-by-line (streaming, not
  `to_string_pretty` of the whole map). Tolerant of a trailing partial line
  (skip + log) so a crashed append never corrupts the store.
- **Schema unchanged**: `MemoryEntry` (`types.rs:163`) serializes the same
  whether it's an array element or a JSONL line. `id` remains the
  per-entry key.

### Decision 2 — Retrieval: flip engine injection to scoped injection

Replace the query engine's **injection** path — which called
`search(&user_message, None)` and injected the top-5 substring matches —
with **scoped injection**: at query time, load the active project's
memories into the system prompt directly (`MemoryStore::format_for_injection`):

- Inject only the `content` field (natural language), grouped by
  `MemoryCategory`, with a short header — the same shape Claude Code's
  `MEMORY.md` index takes.
- Scope = the current working directory (the same string `AutoDreamService`
  stores in `MemoryEntry.project`), capped at ~50 most-recent entries
  (`MAX_INJECTED_MEMORIES`).
- The full `MemoryEntry` (tags, confidence, timestamps) stays in the
  in-memory map for the consolidator and UI; only `content` rides in the
  prompt.

**`search()` is retained.** The REPL `/memory <query>` command
(`shannon-ui/.../repl/commands/memory.rs`) uses it as a deliberate user
keyword search, distinct from auto-injection. The original "delete search()"
wording was refined during C3' implementation once that consumer was
discovered. Recall for *injection* is now 100% for the bounded volume a
curated layer holds; the Jaccard path no longer gates what reaches the model.

### Decision 3 — Write safety: append for writes, flock + atomic-rename for compaction

- **Hot path** (`add`/`update`): append-only. Concurrent appends from
  multiple agents are safe at the OS level (append is atomic for lines
  ≤ PIPE_BUF; `MemoryEntry` lines are small). No flock needed on the hot
  path.
- **Cold path** (consolidation/compaction in `MemoryConsolidator`): this
  pass *does* rewrite the file (dedupe, prune, merge). Guard it with an
  `flock` (exclusive) on the project file + write to `<hash>.jsonl.tmp`
  then `rename()` atomically. This mirrors the ADR-0008 D3
  `providers.toml` write pattern and is the "flock stopgap" work folded
  into this layer rather than bolted onto the old JSON store.

### Decision 4 — Write-time dedup (before append)

Before appending, compare the candidate against same-`category` entries
already in the in-memory map using a light token-overlap check (Claude
Auto Memory's write-time dedup pattern). If overlap exceeds a threshold,
**update** the existing entry in place (append an updated line; the old
line becomes a stale duplicate that the next compaction pass reclaims)
rather than appending a near-duplicate. This keeps the JSONL from growing
unbounded with paraphrases.

### Decision 5 — Compaction (consolidation) stays, gains a periodic trigger

`MemoryConsolidator` and `AutoDreamService` are retained and strengthened:

- Add a **periodic trigger** (~24h wall-clock OR ≥5 sessions for the
  project, Claude's cadence) that runs compaction: dedupe near-duplicates,
  prune stale entries (respecting the existing `cleanup()` `max_age` /
  `max_entries` bounds), resolve relative dates to absolute, and reclaim
  stale duplicate lines from Decision 4.
- Compaction is the **only** writer that rewrites the whole file, and it
  runs under the Decision 3 flock.

### Decision 6 — Multi-agent scope: project-scoped (shared)

All agents operating under the same project path share one
`<project_hash>.jsonl`. This is the Claude Code model (`memory/` is
per-project, shared across all sessions/agents for that project).

- **Why shared**: append-only (Decision 1) already makes concurrent writes
  safe, so the classic reason to shard (write contention) is gone. Agents
  in a team benefit from each other's learned facts (e.g. the build
  command one agent discovered should be available to the others).
- **Why not agent-scoped**: per-agent files (`<hash>.<agent_id>.jsonl`)
  would isolate cleanly but (a) prevent teammates from sharing learned
  facts — a hard loss for team coordination — and (b) multiply file count
  by agent count. The "noise" cost of sharing (one agent's module-specific
  fact being irrelevant to another) is bounded and reclaimed by the
  compaction pass (Decision 5).

### Decision 7 — Migration: one-shot JSON→JSONL, read-compat grace period

- On first load, if `<hash>.json` exists and `<hash>.jsonl` does not,
  read the old JSON array and rewrite it as JSONL (one entry per line),
  then leave the `.json` in place (or rename to `.json.migrated`).
- The read path **also** tolerates a stale `.json` (for one release) so a
  downgrade doesn't lose data. After one release cycle, drop the `.json`
  read path (see C6' in the plan).

## Consequences

### Positive

- **Multi-agent write safety** — append-only eliminates the lost-update
  race; flock guards the only rewriter (compaction).
- **Higher recall** — scoped injection returns every active-scope memory
  to the model, instead of only Jaccard-matched ones.
- **Higher recall at injection** — the engine injects every active-scope
  memory instead of only Jaccard top-5 matches; the Jaccard path no longer
  gates what reaches the model (`search()` is retained for the REPL
  `/memory` command).
- **Pattern reunification** — curated memory now uses the same append-only
  JSONL shape as the four event-log subsystems; one mental model.
- **Crash safety** — a partial last line no longer corrupts the whole
  store (vs. full-rewrite JSON where a crash mid-write can truncate the
  array).

### Negative

- **Context budget** — injecting all active-scope memories costs prompt
  tokens. Mitigated by the existing `cleanup()` `max_entries` bound and
  the Decision 5 compaction pass; the curated layer is sized for tens of
  entries, not thousands.
- **No human-editable format** — JSONL is machine-oriented, unlike
  markdown. Shannon memories are machine-generated (`AutoDreamService`)
  and not user-hand-edited in practice, so this trades little. (If
  user-editing becomes a real driver later, revisit via a new ADR.)
- **Stale-duplicate lines** between compaction passes (Decision 4 appends
  updates; old lines linger until compaction). Bounded and self-healing.

### Neutral

- Retrieval model changes from "search-then-inject-matches" to
  "inject-all, let the model ignore". This is the right model for a
  curated layer (small, high-signal) and the wrong model for a large
  corpus — which is why codebase RAG remains a separate concern.

## Alternatives Considered

### A. Markdown per-file (align Claude Code `memory/` format verbatim)

Rejected. Shannon's `MemoryEntry` is structured (category, confidence,
tags, access_count, timestamps); markdown frontmatter is a lossy fit.
Shannon already has four JSONL subsystems to reuse; markdown would
introduce a fifth storage shape for no functional gain. Markdown's one
real advantage — human editability + Claude `memory/` file-level interop
— is not a driver: memories are machine-generated and the schemas differ.

### B. SQLite (evaluated and rejected in Wave-6)

Rejected. Both reference implementations (Claude Code, Codex CLI) use flat
file storage + injection for curated memory — never a database. SQLite's
concurrency story is heavier than append-only JSONL needs to be, and it
adds a C dependency to the build. Revisited: only if single-project memory
volume grows past ~500 entries **and** injection recall is reported
insufficient — neither is true today. (Codebase RAG, the legitimate
vector-store use case, is a separate subsystem.)

### C. Status quo (keep JSON full-rewrite + Jaccard)

Rejected. Leaves the multi-agent lost-update race in place, keeps the
low-recall retrieval path, and leaves curated memory as the only
non-JSONL store — the inconsistency this ADR exists to resolve.

## Implementation References

### Code (current state, to change in C2'-C6')

- `crates/shannon-core/src/memory/store.rs` — `MemoryStore` (struct
  `:47`-ish), `search()` Jaccard (`:91`-ish), `save()` full-rewrite
  (`:136`/`:152`-ish), `load()` (`:162`-ish).
- `crates/shannon-core/src/memory/types.rs:163` — `MemoryEntry` (unchanged).
- `crates/shannon-core/src/memory/consolidator.rs` — `MemoryConsolidator`
  (retained, gains periodic trigger).
- `crates/shannon-core/src/memory/auto_dream.rs` — `AutoDreamService`
  (retained, drives the compaction trigger).
- `crates/shannon-core/src/session_transcript.rs` — the JSONL append-only
  precedent to mirror (write-one-line, skip-partial-last-line on load).

### Companion plan

- `docs/plans/w7-memory-jsonl-alignment.md` — phased plan (C2' format+migration+flock →
  C3' retrieval flip → C4' write-time dedup → C5' AutoDream trigger + size
  control → C6' old-`.json` read-compat removal), one PR per phase.

### Related docs

- `docs/adr/0008-provider-model-command-architecture-remediation.md` — D3
  `providers.toml` flock + atomic-rename write pattern (the model Decision 3 mirrors).
- `~/.claude/projects/` inspection (2026-08-11) — Claude Code 4-layer
  mixed store; see project memory `claude-code-project-storage-4-layer`.

## Open Questions / Re-evaluation Triggers

- **Context budget ceiling.** What `max_entries` value keeps injected
  memory under ~2K tokens? Decide empirically in C3' once injection lands;
  tune `cleanup()` accordingly.
- **Agent-scoped memories.** If a future workflow wants per-agent private
  memory (e.g. a reviewer agent that should not pollute the shared store),
  layer it as `<hash>.<agent_id>.jsonl` **alongside** the shared
  `<hash>.jsonl`, injected additively — no need to revisit this ADR.
- **Re-evaluate B (SQLite) iff** single-project curated memory exceeds
  ~500 entries **and** a user reports injection missing relevant recall.
  Until then, injection + compaction is sufficient.

## Acceptance

C2' (format + migration + flock) and C3' (retrieval flip) have landed; this
ADR is **Accepted**. Refinement: `search()` is retained for the REPL
`/memory` command (a deliberate user keyword search), not deleted as the
original D2 proposed — only the query-engine injection path flipped to
scoped injection. C4'-C5' (write-time dedup, compaction trigger) are
follow-on within the same Wave-7 plan.
