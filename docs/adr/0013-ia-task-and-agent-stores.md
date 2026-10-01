# ADR-0013: Task & Agent IA — Declared Store Roles and Seam Rules, Not a Data Migration

**Date**: 2026-10-01
**Status**: Accepted
**Sprint**: continuous (journey remediation, P2-9 — user ruling R2)

## Context

The full-journey walkthrough (`docs/research/2026-10-01-full-journey-competitive-review.md`,
§4.3 — structural root causes) found that Shannon Desktop runs **three task
stores, two automation systems, and two agent concepts** side by side:

| Axis | Stores / systems / concepts | Nature |
|---|---|---|
| Task stores | 1. **Scheduled routine store** — `~/.shannon/scheduled-tasks/<slug>-<id>/{SKILL.md, task.json}` (+ `working_dir` sidecar), one JSONL runs-history store per routine (ADR-0001; `shannon-core/src/scheduled_task_store.rs`) | durable, time-scheduled |
| | 2. **Background task table** — `AppState.background_tasks: Arc<Mutex<Vec<BackgroundTaskMeta>>>` (`desktop/src/commands.rs`), in-memory only, gone on restart | volatile, in-flight |
| | 3. **`.claude/tasks/` board files** — `.claude/tasks/{team}/{id}.json` (`desktop/src/commands_tasks.rs`), Claude Code-compatible kanban format, seeded on onboarding | board / interop |
| Automation systems | **Triggered routines** — hook-event automations declared in `~/.shannon/routines.toml` (+ `~/.shannon/routine-overrides.json` toggles), surfaced in the /tasks *Pipelines* tab | fire on agent lifecycle hooks |
| | **Scheduled routines** — cron/interval/webhook jobs in the scheduled store, surfaced in the /tasks *Routines* tab | fire on a clock |
| Agent concepts | **InstalledAgent directories** — what the extensions installer wrote (`~/.shannon/agents/<plugin>/agent.md` subdir shape) | install-time artifact |
| | **AgentDefinition runtime** — what `shannon_agents::AgentDefinitionRegistry::load_from_dirs` reads (flat `~/.shannon/agents/*.toml` + Claude-compatible `*/agents/*.md` dirs; `crates/shannon-agents/src/agent_defs.rs`) | runtime truth |

None of this was *declared*, so features kept committing to the wrong store
and users kept paying for it. The walkthrough tied three P1 breakages to this
single upstream cause:

- **created-then-invisible** (J2-5): a Simple-mode background run was visible
  only in the dev-only Pipelines tab;
- **OPC quick-create not on the board** (J2-6): the toast fired but no card
  appeared anywhere;
- **installed-but-unusable agents** (J3-4): the installer wrote a directory
  shape the runtime loader never read.

The P2-9 proposal in the review was "IA convergence: merge the three task
stores, merge the two automation entries, unify the agent concept". This ADR
records the scoped ruling (R2) instead: **converge the information
architecture at the seams, do not merge the stores and do not migrate data.**

## Decision

**R2: The three stores stay separate, each with one declared role; a data
migration is out of scope.** Convergence happens at the two seams where
user intent crosses a store boundary, plus honest naming in the UI.

### D1 — Division of labor (binding for future features)

| Store | Declared role | Authority |
|---|---|---|
| Scheduled routine store | **Persistent recurring work** | the time-series authority: cron/interval/webhook schedules, run history, pause/edit lifecycle |
| Background task table | **In-flight process handles** | volatile by design; no at-rest semantics, nothing to restore after a restart |
| `.claude/tasks/` | **Board / interop format** | the kanban view and the Claude Code interchange surface; compatibility here is a feature, not debt |

New task-shaped features must name which of the three roles they serve
before they pick a store. A feature that needs two roles writes to both
stores explicitly at the seam — never to a "whichever the code happens to
have open" store.

### D2 — Seam rule 1: every user *creation* lands in the presentation layer of the view the user is acting in

- **OPC quick-create → `.claude/tasks/`.** The OPC board renders
  `.claude/tasks/`; a card minted there must be written through
  `update_task`'s adhoc path and refreshed before the success toast fires
  (landed in #178, `OPCKanbanBoard.tsx` + `commands_tasks.rs`).
- **/tasks "New Background Task" → background table, visible on the Runs
  tab in every mode.** A run started from /tasks appears in the
  `BackgroundTasksPanel` on the Runs tab (Simple mode included; landed in
  #178). In-flight work is never parked behind a dev-only tab.
- **/tasks "New Automation" → scheduled store**, housed in the deep-linked
  project when one is active (`working_dir` defaulting, P-U3), so the
  routine is visible in the Routines tab/calendar it was created from.

### D3 — Seam rule 2: every *installation* lands in the runtime-readable form

Installers write what the runtime loader reads, not a parallel shape
(landed in Wave 1 G1, PR #177; recorded here as the accepted seam decision):

- Agent installs (repo collections and single agents) materialize as **flat
  `~/.shannon/agents/<name>.toml`** definitions — the exact shape
  `AgentDefinitionRegistry::load_from_dirs` loads
  (`desktop/src/extensions/agent_installers.rs`). The legacy
  `<plugin>/agent.md` subdirectory shape is converted at startup by
  `migrate_legacy_agent_dirs` (idempotent); it is no longer written.
- Uninstall removes exactly what was materialized, tracked by the
  `.shannon-flat-agents.json` sidecar listing **slugified on-disk file
  names** (the name the loader will see), so partial or odd-cased installs
  cannot leave ghost definitions behind.

### D4 — Naming honesty: UI copy states what an action actually does

Where an action writes a definition but does not start anything, the UI may
not say "Spawn"/"Run". The OPC "Spawn/Create Agent" dialog — which only
writes `.claude/agents/<name>.md` frontmatter via
`create_agent_definition` — is renamed **"Register agent template"** across
all 10 locales, with matching toasts ("Agent template … registered"). Tab
names that ride different systems carry a one-line explanation of trigger
type and storage (Routines vs Pipelines on /tasks), instead of implying a
shared pipeline store.

### D5 — What we are explicitly not doing

- **No merge of the three stores.** No canonical `TaskRecord`, no
  cross-store IDs, no write-behind syncing.
- **No data migration.** Existing `.claude/tasks/` files, scheduled
  routines, and in-memory runs stay exactly where they are. (The one
  migration that did happen — legacy agent directories → flat TOML — is a
  *format* fix inside one store, not a store migration; it converts
  install-time artifacts to the form the same store's loader always
  required.)
- **No merged entry page for the two automation systems.** They stay on
  separate tabs with explanatory copy.

## Consequences

- **Positive**
  - Each store keeps its best property: durability (scheduled), liveness
    (background), interoperability (`.claude/tasks/`).
  - The three walkthrough breakages are closed at the seam (visibility),
    without a risky cutover of live data.
  - Claude Code round-tripping (board files, agent frontmatter) remains a
    differentiator, not a liability to be explained away.
  - Future contributors have a rule to appeal to (D1) instead of
    re-litigating store choice per feature.

- **Negative**
  - "Where is my task?" still has three honest answers; the UI must keep
    carrying explanation copy (D4) because the data model does not collapse
    the question.
  - Cross-store features (e.g. promoting a board card to a recurring
    routine) must implement explicit hand-off writes at the seam; there is
    no free join.
  - The background table's volatility remains user-visible: a restart
    clears in-flight runs by design and nothing restores them.

- **Neutral**
  - The flat-TOML agent decision makes `~/.shannon/agents/` the user-global
    install target while `.claude/agents/`, `.shannon/agents/` (project)
    and built-ins remain loader sources with a defined override order —
    documented in `agent_defs.rs`, unchanged here.

### Known residue (deferred, documentation-natured — tracked in the journey ledger, not fixed in code)

- Triggered-routine concurrency has no in-flight check, and a killed run
  leaves its `running` history row behind — both pre-existing behaviors
  (Wave 1 G2, deferred); they belong in user-facing automation docs as
  known limitations before anyone "fixes" them silently.
- `policy.max_retries = N` means N total attempts (N−1 retries) — core
  semantics kept as-is; the schedule form hint's "retry count" intuition is
  off by one until the core docs state the contract (Wave 2 G2b, deferred).
- The scheduled-worktree module comment covers only the remove path's half
  of the story (Wave 2 G2b, deferred comment fix).
- The shared attachment classifier's exception inheritance
  (`read_attachment`-style allows inherited by other command domains, no
  privilege escalation) deserves a clarifying comment (G3b scoped
  re-review, informational, backlog).

### Migration triggers and sketch (for the day D5 is reversed)

Revisit this ADR when at least one of these appears:

1. **Multi-device / multi-surface sync** of routines or boards is required
   (a second writer to `~/.shannon` that needs merge semantics).
2. **Unified task search / reporting** across stores becomes a top user
   ask that copy cannot satisfy.
3. A third task-shaped store is proposed (the divide-and-declare rule
   stops scaling).

Sketch, if triggered: follow the ADR-0009 pattern (read-path facade first)
— define a projection layer that joins the three stores read-only per
query, ship it behind the existing surfaces, and only then consider a
canonical write path. Never a big-bang table move: each store has a live
writer (scheduler loop, executor, gateway/onboarding seeds) and the
`.claude/tasks/` format must keep round-tripping through Claude Code
during the whole transition.

## Alternatives Considered

- **Full merge into one canonical task store** (the original P2-9 wording).
  Rejected: (a) it breaks the `.claude/tasks/` interop contract that other
  tools read and write, converting a feature into a sync problem;
  (b) migration risk is real — three live writers and one of the stores is
  in-memory by design, so "migrate" means inventing durability semantics
  the product has deliberately not chosen; (c) the user-visible harm
  (created-then-invisible, missing board cards) was fully addressable at
  the seams for a fraction of the cost; the merge buys almost nothing the
  seams do not.
- **Migrate `.claude/tasks/` into the scheduled store**, keeping the board
  as a rendered view. Rejected for (a) above, plus it breaks onboarding
  seeds and Claude Code users' existing boards; the board *is* the
  interchange format.
- **Merge the two automation systems into one entry.** Rejected: hook-event
  triggers and clock schedules have different semantics, different config
  files, and different failure modes; a merged entry hides that. Instead:
  separate tabs with one-line explanations (D4).
- **Keep "Spawn" naming but add a subtitle** ("only creates a definition").
  Rejected: a label that needs a disclaimer is the dishonest form of the
  honest name.

## Implementation References

- `crates/shannon-core/src/scheduled_task_store.rs` — scheduled store
  layout (ADR-0001 format), `default_base_dir()`.
- `desktop/src/commands.rs` — `AppState.background_tasks` (volatile
  table), `start_background_task`.
- `desktop/src/commands_tasks.rs` — `.claude/tasks/` board read/write,
  adhoc path used by OPC quick-create.
- `desktop/src/commands_agents.rs` — `create_agent_definition` (writes
  `.claude/agents/<name>.md`; the "register" semantic).
- `desktop/src/extensions/agent_installers.rs` — flat-TOML install shape,
  `migrate_legacy_agent_dirs`, `.shannon-flat-agents.json` sidecar
  (Wave 1 G1: commits `2fbdaa63`, `32942015`; PR #177).
- `crates/shannon-agents/src/agent_defs.rs` — runtime loader and directory
  override order.
- `desktop/src/scheduled_commands.rs` — triggered-routine registry
  (`~/.shannon/routines.toml` + overrides).
- `desktop/ui/src/components/opc/OPCAgentSwarm.tsx`,
  `desktop/ui/src/components/opc/OPCKanbanBoard.tsx`,
  `desktop/ui/src/components/tasks/BackgroundTasksPanel.tsx`,
  `desktop/ui/src/pages/Tasks.tsx` — seam-rule UI (PR #178 and this ADR's
  copy pass).
- PRs #177 (extensions runtime) and #178 (routine lifecycle + board wiring)
  — the landed seam decisions this ADR records.

## Open Questions

- Should the background table persist a tombstone ("N runs were in flight
  when the app closed") so the volatility is visible without inventing
  durability?
- Does the Runs tab need a per-project filter parity with the Routines tab
  (`?project=` deep link) once cross-store promotion ships?
- If a sync requirement lands, does `.claude/tasks/` become a synced
  *source* (CRDT-merge boards) or stay local-only with routines as the
  synced tier?
