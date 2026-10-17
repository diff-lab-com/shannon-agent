# Hook Event Dispatch Semantics (P0-2)

**Machine source of truth:** `crates/shannon-engine/src/hooks/semantics.rs` —
the table below is its human-readable projection. **Consistency is enforced by
a test** (`semantics.rs` `#[cfg(test)]` smoke test: the file must exist and
name all 30 events; the table itself is compile-checked via an exhaustive
`match` with no wildcard arm, so a new `HookEventType` variant without a row
fails the build).

**Scope / status:** documentation only — this table records actual behavior as
of `feat/p0-harness-contracts` (post P0-1). It changes nothing; where the
behavior looks wrong, that is recorded in the Notes column instead of being
"corrected" here.

---

## Reading the columns

- **Regime** — how the event reaches `HookManager::run_hooks`:
  - `EnforcedInline`: run synchronously inside the tool guard pipeline
    (`PreToolUseHookNode`, stage `tool/guard/pre-tool-use-hooks`); the only
    event whose decisions gate execution.
  - `BusTriggerAdvisory`: a producer publishes a routing-only trigger on the
    session bus (`publish_hook_trigger`); the `HookManagerAdapter`
    subscription decodes it and runs hooks advisory — decisions only land in
    `hook/fired` audit rows, never enforced.
  - `DirectCall`: a producer constructs the `HookEvent` and calls
    `HookManager::run_hooks` directly (REPL / compact / coordinator /
    config-watcher / skills / project-instructions paths).
  - `NoProducer`: no construction site in the repo (9 variants after P0-1
    gave `PostToolUseFailure` its first producer).
- **ShortCircuit** — what a hook *decision* can actually do here:
  `DenyModify` (deny + rewrite tool input), `DenyOnly`, `Observe` (hooks run,
  outcomes logged, decisions dropped), `None` (no hook result is consumed at
  all).
- **CcCompat** — `Standard` = one of the 9 Claude Code standard events
  (PreToolUse, PostToolUse, SessionStart, SessionEnd, Notification,
  UserPromptSubmit, Stop, SubagentStop, PreCompact); `Extension` = Shannon
  extension.
- **Producers** — producing modules (file paths, no line numbers).
- **FailureSemantics / Notes** — per-event failure behavior and gotchas.

## Shared protocol (all events, `hooks/manager.rs`)

- Exit-code protocol (command/prompt hooks): exit 0 = parse first stdout line
  as JSON decision (default Allow); **exit 2 = Deny with stderr as reason**;
  any other exit = non-blocking error (Allow). HTTP hooks: 2xx parses the
  body as a decision; other statuses / transport failures decide Deny.
- Blocking command/prompt/HTTP hook **timeout = `Err(HookError::Timeout)`
  propagates out of `run_hooks`**; every caller warns/logs and continues — a
  timed-out hook never blocks or denies. **LLM hooks are the exception**:
  missing `LlmClient`, API error, or timeout decide **Deny** inside the
  manager.
- `resolve_results`: the **first Deny short-circuits** (returned
  immediately), the **last Modify wins**, otherwise Allow.
- Non-blocking (`blocking: false`) command/HTTP hooks are spawned detached,
  bounded by their configured timeout (`kill_on_drop`), and produce no
  result.
- Matcher gotcha: patterns hit the **anchored regex branch first**
  (`^{matcher}$`), so `"Edit|Write"` behaves as regex with anchor precedence
  (`^Edit` **or** `Write$` — matches `EditFoo`/`fooWrite`), not as the exact
  pipe-split alternation; the pipe-split branch is only reachable for
  patterns that are invalid regexes.
- `HookConfig.if_condition` is deserialized but **never evaluated**
  (`matches()` ignores it).

---

## The table

| Event | Regime | ShortCircuit | FailureSemantics | CcCompat | Producers | Notes |
|---|---|---|---|---|---|---|
| PreToolUse | EnforcedInline | DenyModify | exit2=Deny(blocks the call); blocking timeout=Err→node warn+Allow (a timed-out hook neither blocks nor denies); resolve_results: first Deny short-circuits, last Modify(input) wins, else Allow | Standard | guard_nodes.rs | only enforced event: PreToolUseHookNode (pipeline stage `tool/guard/pre-tool-use-hooks`) sets ctx.hook_deny and rewrites ctx.input; additionally mirrored as an advisory bus trigger (HookManagerAdapter decodes `PreToolUse`, no engine publisher yet); matcher regex is tried before pipe-split; if_condition parsed but never evaluated |
| PostToolUse | BusTriggerAdvisory | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Standard | agent_loop.rs | trigger published by the shared post-execute helper with is_error flag; adapter runs hooks and writes hook/fired audit rows only — decisions unused |
| SessionStart | BusTriggerAdvisory | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Standard | agent_loop.rs, repl/mod.rs | engine fires once per session at the first query; the REPL fires it at startup and pre-marks the flag so the engine stays silent (no double-fire); the REPL path is a direct run_hooks call whose results are dropped |
| SessionEnd | BusTriggerAdvisory | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Standard | repl/mod.rs | REPL fires it directly during shutdown before the sidecar save; results dropped; the adapter decodes this trigger type so future bus producers route advisory |
| Notification | NoProducer | None | no producer: nothing runs | Standard | (none) | CC standard event; enumerated but never constructed in-repo |
| UserPromptSubmit | BusTriggerAdvisory | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Standard | agent_loop.rs | fired per user message right after the L0 turn start; results unused (CC's stdout-fed-back-to-model behavior is not implemented) |
| TeamTaskCreated | DirectCall | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Extension | coordinator.rs | coordinator fire_hook (spawned, non-blocking); variant doc claims exit2=rollback but fire_hook only logs it — not enforced |
| TeamTaskCompleted | DirectCall | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Extension | coordinator.rs | coordinator fire_hook (spawned, non-blocking); variant doc claims exit2=prevent completion but fire_hook only logs it — not enforced |
| TeammateIdle | DirectCall | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Extension | coordinator.rs | coordinator fire_hook (spawned, non-blocking); variant doc claims exit2=feedback+keep working but fire_hook only logs it — not enforced |
| PreCompact | DirectCall | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Standard | compact/engine.rs | fired on a spawned task before compaction — completion is NOT awaited, so it can race the compaction it precedes; compact proceeds regardless |
| SubagentStart | DirectCall | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Extension | coordinator.rs | spawned fire-and-forget before teammate work starts |
| SubagentStop | DirectCall | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Standard | coordinator.rs | spawned fire-and-forget on the success/timeout/failure paths with result_summary |
| PermissionDenied | NoProducer | None | no producer: nothing runs | Extension | (none) | |
| Stop | BusTriggerAdvisory | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Standard | agent_loop.rs | published immediately before QueryEvent::Completed; should_continue is hardcoded false, so the documented exit-2-force-continue semantics are unreachable |
| PostToolUseFailure | BusTriggerAdvisory | None | producer publishes the trigger, but decode_trigger has no arm for this type yet — the trigger is dropped at decode and run_hooks never executes | Extension | agent_loop.rs | P0-1 gave this event its first producer (tool Err path); the payload mirrors the PostToolUse shape (output + is_error=true), not the variant's `error` field, and `decode_trigger` (guard_nodes.rs) cannot decode it yet — hooks configured on this event do not execute; no hook result is ever consumed (hence None) |
| PostCompact | DirectCall | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Extension | compact/engine.rs | spawned fire-and-forget after successful compaction (before/after counts + tokens_freed) |
| StopFailure | NoProducer | None | no producer: nothing runs | Extension | (none) | |
| FileChanged | NoProducer | None | no producer: nothing runs | Extension | (none) | |
| CwdChanged | NoProducer | None | no producer: nothing runs | Extension | (none) | |
| PermissionRequest | NoProducer | None | no producer: nothing runs | Extension | (none) | |
| UserPromptExpansion | DirectCall | None | dormant wiring: no hook result is ever consumed while no emitter is attached | Extension | skills/executor.rs | executor emits the serialized event to an optional HookEmitter after template substitution; no production host attaches one, so run_hooks is never reached |
| PostToolBatch | NoProducer | None | no producer: nothing runs | Extension | (none) | |
| ConfigChange | DirectCall | None | dormant wiring: no hook result is ever consumed while no callback is attached | Extension | config_watcher.rs | watcher converts debounced detections via to_hook_event for a run_hooks-style callback; `watch_local_toml` has no production caller — dormant |
| InstructionsLoaded | DirectCall | None | dormant wiring: no hook result is ever consumed while no emitter is installed | Extension | project_instructions.rs | global INSTRUCTION_HOOK_EMITTER (OnceLock) fires after the instruction merge; the default is a no-op and only tests install an emitter — dormant in production |
| WorktreeCreate | DirectCall | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Extension | coordinator.rs | coordinator fire_hook (spawned, non-blocking); exit 2 logged only |
| WorktreeRemove | DirectCall | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Extension | coordinator.rs | coordinator fire_hook (spawned, non-blocking); exit 2 logged only |
| Elicitation | NoProducer | None | no producer: nothing runs | Extension | (none) | |
| ElicitationResult | NoProducer | None | no producer: nothing runs | Extension | (none) | |
| TaskCreated | DirectCall | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Extension | coordinator.rs | events.rs doc calls this a CC standard name, but it is classified Extension per the plan's fixed 9-event CC set |
| TaskCompleted | DirectCall | Observe | exit0=parse stdout JSON; exit2=Deny(stderr); other=nonblocking error(Allow); blocking timeout=Err propagates, caller warns+continues | Extension | coordinator.rs | events.rs doc calls this a CC standard name, but it is classified Extension per the plan's fixed 9-event CC set |

---

## Cross-reference

- Variant/enum definitions: `crates/shannon-engine/src/hooks/events.rs`
- Execution engine (`run_hooks`, exit-code protocol, timeouts,
  `resolve_results`): `crates/shannon-engine/src/hooks/manager.rs`
- Matcher / `if_condition` / decision parsing:
  `crates/shannon-engine/src/hooks/config.rs`
- Enforced-inline node + bus adapter (`PreToolUseHookNode`,
  `HookManagerAdapter`, `publish_hook_trigger`, `decode_trigger`):
  `crates/shannon-core/src/query_engine/guard_nodes.rs`
- Audit of Shannon-vs-CC event coverage: `docs/HOOK-AUDIT.md`
