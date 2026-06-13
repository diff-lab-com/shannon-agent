# ADR-0001: Scheduled Tasks Storage — SKILL.md + JSONL Partitioned History

**Date**: 2026-06-13
**Status**: Accepted
**Sprint**: Sprint 1 (post-implementation retrospective)
**Supersedes**: Storage design proposed in `SCHEDULED-FIX-PLAN.md` v2 §2.1–§2.3

---

## Context

The original scheduled-task storage was a single JSON file at `~/.shannon/routines.json`
holding a `HashMap<String, ScheduledRoutine>`. Sprint 1 of the Scheduled feature
introduced cron-mode scheduling and Claude Code-style task storage. We needed
to decide:

1. **Task records**: Stay with `routines.json` (one JSON blob for all tasks) or
   move to per-task files?
2. **Execution history**: One file per run, JSONL append-only, or a single
   `runs.json` file?
3. **Naming**: Rename `ScheduledRoutine` → `ScheduledTask` to match Claude
   Code's terminology, or keep the existing name?

These decisions affect future UI work, migration safety, and disk efficiency
as run count grows.

## Decision

We adopt a **hybrid model**:

### 1. Task records: per-task directories (Claude Code-compatible)

```
~/.shannon/scheduled-tasks/
├── <slug>-<id>/
│   ├── SKILL.md          # Human-editable prompt (markdown)
│   └── task.json         # Machine-managed ScheduledRoutine struct
└── ...
```

- `slug` is derived from the task name (filesystem-safe, lowercase, hyphenated)
- `id` is the 8-char UUID prefix already used by `ScheduledRoutine.id`
- `SKILL.md` is editable by users and tools; `task.json` is the source of truth
- Matches Claude Code's layout at `~/.claude/scheduled-tasks/<name>/`

### 2. Execution history: JSONL partitioned by YYYY/MM

```
~/.shannon/scheduled-runs/
├── 2026/
│   ├── 06.jsonl   # One line per run, newest appended at end
│   └── 05.jsonl
└── 2025/
    └── 12.jsonl
```

- Append-only — no rewrite needed to record a run
- Each line is a complete `ScheduledRun` JSON with a `revision` field
- Updates append a new line with `revision += 1`; reads resolve to the latest
  revision per `run_id` (last-write-wins)
- Pruning is in-place file rewrite of lines outside the 90-day window

### 3. Naming: keep `ScheduledRoutine` (no rename)

The struct stays `ScheduledRoutine`. We extend it with a `trigger_type` field
(`Interval` | `Cron` | `Webhook` | `Event`) rather than introducing a
new `ScheduledTask` type. Avoids cross-crate rename churn.

## Consequences

### Positive

- **Human-editable prompts**: `SKILL.md` can be opened in any editor; changes
  are picked up on next `task.json` reload (single source of truth for
  metadata, prompt mirrored to SKILL.md)
- **Disk-friendly history**: JSONL scales to thousands of runs without
  inode bloat (vs one-file-per-run); appending is O(1)
- **Backward compatible**: New `ScheduledRoutine` fields use
  `#[serde(default)]`, so legacy `routines.json` deserializes cleanly;
  `ScheduledTaskStore::migrate_from_routines_json()` is idempotent and
  creates a `.bak` backup
- **Last-write-wins semantics**: The `revision` field lets `update()` be a
  simple append — no index maintenance, no file locking for the common case
- **Standard tooling**: `jq`, `grep`, `wc -l` work directly on JSONL files
  for ad-hoc analysis

### Negative

- **Read amplification**: `find_by_id(run_id)` scans all year/month files
  (no index). Acceptable for a single-user desktop app with ~10k runs/year;
  would need an index for cloud/multi-tenant
- **Two storage locations to keep in sync**: `task.json` holds the full
  struct; `SKILL.md` mirrors only the prompt. If a user edits `SKILL.md`
  directly, the prompt in `task.json` becomes stale until next reload.
  Documented in module-level `//!` comment; reload path is the source of truth
- **Cron-mode `next_fire_at` requires eager computation**: At construction
  time we call `find_next_occurrence` to populate `next_fire_at`. If the
  timezone changes between construction and fire time, the cached value is
  wrong. Documented trade-off; `mark_fired()` recomputes on each fire
- **`slugify` collisions**: Two tasks named "Daily Scan" produce the same
  slug; uniqueness comes from the `-<id>` suffix. Slug is for human
  readability only, not for lookup

### Neutral

- Module layout added two new files: `scheduled_task_store.rs` (~370 lines)
  and `scheduled_runs.rs` (~530 lines). Total ~1560 lines for the full
  Sprint 1 backend surface

## Alternatives Considered

### A. Keep `routines.json` as the only storage

**Rejected**: Single-file storage blocks concurrent edits, requires full
rewrite on every save, doesn't scale to history (which is 10–100x larger
than the task list). Also diverges from Claude Code's standard layout,
which would surprise users coming from Claude Code.

### B. SQLite for both tasks and history

**Rejected**: Adds a C dependency, complicates the build, overkill for a
single-user desktop app. The `~10k runs/year` volume doesn't justify a
query engine. JSONL is easier to inspect and debug.

### C. One file per run (`<run-id>.json`)

**Rejected**: Causes inode bloat at scale (~10k files/year). Filtering by
task or time requires loading every file. The proposed plan v2 §2.3
suggested this; rejected after seeing that `find_by_id` and `list_by_task`
would be O(N files opened) rather than O(N lines parsed).

### D. Rename `ScheduledRoutine` → `ScheduledTask`

**Rejected**: Would require updating 4+ crates (shannon-core, shannon-ui,
shannon-tools, shannon-commands) plus all tests and docs. The
`trigger_type` enum expresses the same intent (legacy interval vs. new
cron) without breaking changes. Future migration to a renamed type can
happen later if/when the legacy interval mode is removed.

### E. JSONL with no `revision` field (overwrite in place)

**Rejected**: In-place edits to JSONL require rewriting the whole file
(since lines are variable-length). The `revision` field lets `update()`
be a simple append; reads resolve to the latest revision per `run_id`.
Trade-off: stale revisions stay in the file until the next `prune_old()`
pass (90-day window). Disk cost is negligible.

## Implementation References

- `crates/shannon-core/src/scheduled_task_store.rs` — per-task directory store
- `crates/shannon-core/src/scheduled_runs.rs` — JSONL partitioned history store
- `crates/shannon-core/src/scheduled_routines.rs` — extended struct + `drain_due_with_history`
- `crates/shannon-desktop/SCHEDULED-FIX-PLAN.md` §2.1–§2.3, §11 — original
  design and Sprint 1 retrospective

## Open Questions

- Should `ScheduledTaskStore` watch `SKILL.md` for external edits and
  auto-reload the prompt into `task.json`? Defer until UI layer needs it.
- Should history archive (>10MB single file) be a separate `.jsonl.gz`
  or moved to a `archive/` subdirectory? Decision deferred to Sprint 5.
