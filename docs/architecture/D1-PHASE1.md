# D1 Phase 1: shannon-core Dependency Map

This document inventories the current shannon-core module structure and
its cross-crate usage. It is the foundation for D1 phases 2 (extract
shannon-engine) and 3 (extract shannon-skill-loop + rename).

## Current state (as of 0.5.5)

shannon-core is a monolithic crate with **86 top-level modules** carrying
roughly 115K LOC. All modules live under `crates/shannon-core/src/` and
share a single dependency block.

## Cross-crate usage inventory

Other workspace crates import these `shannon_core::` paths. Any extraction
must preserve or re-export them.

| Path | Used by | Notes |
|------|---------|-------|
| `api::LlmClientConfig` | shannon-mcp, shannon-agents, shannon-cli | Wire type, no logic. |
| `api::LlmProvider` | shannon-mcp, shannon-agents | Enum. |
| `api::LlmClient` | shannon-mcp, shannon-agents | Client object. |
| `api::{Message, MessageContent, ContentBlock}` | shannon-mcp, shannon-agents, shannon-tools | Wire types. |
| `api::types::*` (full) | shannon-agents | Re-export surface. |
| `permissions::ApprovalMode` | shannon-agents | Enum. |
| `permissions::PermissionChoice` | shannon-mcp | Enum. |
| `permissions::PermissionManager` | shannon-agents | Object with state. |
| `permissions::RiskLevel` | shannon-mcp | Enum. |
| `query_engine::{QueryContext, QueryEvent, QueryEngine}` | shannon-mcp, shannon-agents, shannon-cli | Engine + wire types. |
| `notifier::{Notification, NotificationHandler, Cooldown}` | shannon-mcp, shannon-agents | Trait + helpers. |
| `compact::CompactEngine` | shannon-agents | Object. |
| `hooks::HookManager` | shannon-agents | Object. |
| `memory::MemoryCategory` | shannon-tools | Enum. |
| `state::StateManager` | shannon-agents | Object. |
| `testing::record_replay::*` | (test-only consumers) | Test infra. |

External consumers (shannon-desktop) additionally use:
- `skill_loop::{TaskEvaluation, TaskOutcome, evaluate_task, generate_skill}`
- `scheduled_task_store::ScheduledTaskStore`
- `scheduled_runs::ScheduledRunsStore`
- `triggered_routines::TriggeredRoutineRegistry`
- `plugin::PluginRegistry`

## Dependency entanglement

shannon-core's internal modules cross-reference each other heavily.
Examples observed in `api/types.rs` (2293 LOC):

```text
api/types.rs → api/retry.rs (RetryConfig)
api/types.rs → unified_config.rs (ShannonConfig)
permissions.rs → permission_profile.rs (PermissionProfile, ProfileRules)
query_engine/* → api/*, tools/*, hooks/*, state/*, compact/*
```

A clean extraction of any single module requires either:
1. Also extracting its dep tree (transitive), or
2. Stubbing the dep behind a trait in shannon-types.

## Phase 2 plan — extract shannon-engine

**Scope**: move the query engine + state + permissions into a new
`shannon-engine` crate.

**Modules to move** (estimated):
- `query_engine/` (~5K LOC)
- `state.rs` (~1K LOC)
- `permissions.rs` + `permission_profile.rs` (~4K LOC combined)
- `streaming_tool_executor.rs`
- `compact/` (~3K LOC)
- `hooks/` (~2K LOC)

**Stays in shannon-core** (renamed to `shannon-shell` or similar in phase 3):
- All `[tag]` orchestration that ties engine pieces together
- CLI-specific entry points (already mostly in shannon-cli)

**Estimated effort**: 2-3 days. Mostly mechanical move + import path
rewrites + Cargo.toml updates across the workspace.

## Phase 3 plan — extract shannon-skill-loop + rename

**Scope**: move the skill loop (already lives at
`shannon-core/src/skill_loop/`) into its own crate, then rename what
remains in shannon-core to clarify its role.

**Modules to move**:
- `skill_loop/` (~1.5K LOC, 7 files, 24 tests)
- `extract_memories.rs` + `memory/` (~5K LOC) — natural pairing with skills

**Rename candidates for what stays**:
- `shannon-shell` — if it becomes primarily the REPL/CLI shell
- `shannon-orchestration` — if it remains the integration layer
- `shannon-runtime` — if it provides the runtime context

**Estimated effort**: 1-2 days. Skill loop is already self-contained;
the rename is mechanical but touches every external Cargo.toml pin.

## Phase 1 deliverables (this PR)

What this branch ships:
1. This dependency map (the document you're reading).
2. Confirmation that `shannon-types::events` (D4) is the first successful
   extraction — events moved cleanly because they had zero internal
   deps. That validates the pattern: pure wire types move first,
   logic-heavy modules wait for phase 2.

What this branch does NOT ship:
- Any new crate creation. The risk of a botched mid-extraction commit
  leaving the workspace non-compiling is too high for a single agent
  session. Phase 2 should be a dedicated PR per module family.
- The `#[stable]` annotation pass. Deferred to a follow-up that
  pairs with phase 2 (so we annotate the crates in their final layout).

## Recommended sequencing

1. **D4 merge first** (already on `s2/d-d4-state-sync` branch) — events
   are the cleanest extraction and unblock the EventEnvelope contract.
2. **D3 merge second** (`s2/d-d3-api-semver`) — version align + policy
   doc + advisory CI. No code risk.
3. **D1 phase 2** — dedicated branch per module family. Each PR moves
   ONE module with full test coverage.
4. **D1 phase 3** — only after phase 2 stabilizes.

## Tracking

- D1 umbrella: <https://github.com/shannon-agent/shannon-code/issues/42>
- D3 (semver): <https://github.com/shannon-agent/shannon-code/issues/43>
- D4 (events): <https://github.com/shannon-agent/shannon-code/issues/44>
