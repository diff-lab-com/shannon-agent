# Hook Event Audit — Shannon vs Claude Code

**Date:** 2026-06-14
**Phase:** E4 (`PHASE-E-ROADMAP.md`)
**Scope:** Enumerate Shannon's hook events, cross-reference against Claude
Code's official documentation, identify gaps, and document dead events.

---

## Summary

Shannon's `HookEventType` enum defines **30** events. Claude Code's
official hooks reference (`https://code.claude.com/docs/en/hooks`) also
lists **30** events. **28 events are shared**; the remaining 2 in each
direction are differentiators.

Audit headline: the roadmap's "Shannon has 32 hook events; Claude Code
has ~18" claim was outdated. Both systems are now at parity in count.
After the P1-2 pass (commit `b2b28f9b`), Shannon has **2 dead events**
(`Elicitation`, `ElicitationResult`) that should be either wired or
removed; the other 3 previously-dead events are now emitted (see
[Dead events](#dead-events-production-emit-gaps) below).

---

## Cross-reference matrix

### Shared events (28)

These events exist in both Shannon and Claude Code with equivalent
semantics.

| Event | Shannon | Claude Code | Notes |
|-------|---------|-------------|-------|
| `PreToolUse` | ✓ | ✓ | Identical |
| `PostToolUse` | ✓ | ✓ | Identical |
| `PostToolUseFailure` | ✓ | ✓ | Identical |
| `PostToolBatch` | ✓ | ✓ | Identical |
| `PermissionRequest` | ✓ | ✓ | Identical |
| `PermissionDenied` | ✓ | ✓ | Identical |
| `UserPromptSubmit` | ✓ | ✓ | Identical |
| `UserPromptExpansion` | ✓ | ✓ | Wired in `shannon-skills` executor after template substitution (P1-2a, b2b28f9b) |
| `SessionStart` | ✓ | ✓ | Identical |
| `SessionEnd` | ✓ | ✓ | Identical |
| `Stop` | ✓ | ✓ | Identical |
| `StopFailure` | ✓ | ✓ | Identical |
| `SubagentStart` | ✓ | ✓ | Identical |
| `SubagentStop` | ✓ | ✓ | Identical |
| `Notification` | ✓ | ✓ | Identical |
| `PreCompact` | ✓ | ✓ | Identical |
| `PostCompact` | ✓ | ✓ | Identical |
| `FileChanged` | ✓ | ✓ | Identical |
| `CwdChanged` | ✓ | ✓ | Identical |
| `ConfigChange` | ✓ | ✓ | Wired via `shannon-core::config_watcher` (notify v7, parent-dir watch, atomic-replace safe) (P1-2c, b2b28f9b) |
| `InstructionsLoaded` | ✓ | ✓ | Wired in `shannon-core::project_instructions` after CLAUDE.md + rules merge (P1-2b, b2b28f9b) |
| `WorktreeCreate` | ✓ | ✓ | Identical |
| `WorktreeRemove` | ✓ | ✓ | Identical |
| `Elicitation` | ⚠️ dead | ✓ | Defined but never emitted |
| `ElicitationResult` | ⚠️ dead | ✓ | Defined but never emitted |
| `TeammateIdle` | ✓ | ✓ | Identical |
| `TaskCreated` | ✓ | ✓ | Identical |
| `TaskCompleted` | ✓ | ✓ | Identical |

### Claude Code only (2)

| Event | What it does | Shannon recommendation |
|-------|-------------|------------------------|
| `Setup` | Fires when Claude starts with `--init-only` / `--init` / `--maintenance` in `-p` mode. One-time preparation in CI/scripts. | **Add.** Shannon has `--prompt` non-interactive mode. Adding `Setup` would emit before the first prompt round-trip in CI. Low effort, useful for env checks. |
| `MessageDisplay` | Fires while assistant message text is being displayed. Useful for transcript logging / UI sync. | **Defer.** Shannon's TUI does not have a notion of "display lifecycle" separate from message production; would require re-architecting rendering hooks. |

### Shannon only (2)

| Event | What it does | Claude Code parallel |
|-------|-------------|----------------------|
| `TeamTaskCreated` | Fires before a team task is committed; exit code 2 = rollback. | Claude Code has plain `TaskCreated` only; Shannon's team variant is a stricter gate. **Keep** — differentiator. |
| `TeamTaskCompleted` | Fires before a team task is marked complete; exit code 2 = revert to `in_progress`. | Same as above. **Keep**. |

---

## Dead events (production-emit gaps)

After the P1-2 pass landed in commit `b2b28f9b`, three of the five
previously-dead events are now wired:

| Event | Status | Emit location |
|-------|--------|----------------|
| `UserPromptExpansion` | ✓ wired | `crates/shannon-skills/src/executor.rs::emit_user_prompt_expansion` — fires after `substitute_arguments` + `substitute_named_arguments` + `substitute_variables` resolve `$ARGUMENTS`, `${0}`, `${CLAUDE_SESSION_ID}`, etc. Optional `HookEmitter` trait; serializes the event to JSON and hands it to the attached emitter (default: `NoopHookEmitter`). |
| `InstructionsLoaded` | ✓ wired | `crates/shannon-core/src/project_instructions.rs::emit_instructions_loaded` — fires after the CLAUDE.md / AGENTS.md / `.claude/rules/*.md` merge completes. Routed through a global `OnceLock` so `project_instructions` does not need a hard dep on `shannon-engine`; `ToolExecutionContext::install_instructions_emitter` registers the actual hook manager. |
| `ConfigChange` | ✓ wired | `crates/shannon-core/src/config_watcher.rs::ConfigWatcher` — wraps `notify::RecommendedWatcher` and watches the *parent directory* (not the file) so atomic-replace (rename / unlink + create) edits still surface. `ConfigWatcher::start(path, callback)` returns `None` on missing parent or notify failure (sandboxed CI), and `ConfigChange::into_hook_event()` converts the detection into `HookEvent::ConfigChange`. Reachable from the call site as `unified_config.watch_local_toml(callback)`. |
| `Elicitation` | ⚠️ still dead | `shannon-mcp::process_pool` when an MCP server sends `elicitation/create` — needs the MCP UI bridge (out of scope for P1-2) |
| `ElicitationResult` | ⚠️ still dead | Same flow, after user responds |

---

## Test coverage

### Unit (`crates/shannon-core/src/hooks/tests.rs`)

Existing unit tests cover:
- `HookEventType` enum (from_str_lossy, display, serialization)
- `HookEvent` variants — `event_type()`, `match_subject()`, JSON
  roundtrip (sample of 8 variants: Pre/PostToolUse, SessionStart/End,
  Notification, UserPromptSubmit, PreCompact, Stop, FileChanged,
  CwdChanged, PermissionDenied, PostToolBatch, Team*)
- `HookDecision` parsing / serialization
- `HookDef` / `HookConfig` matcher rules
- `HooksFile` load/merge/serialize

### Integration (`crates/shannon-core/tests/hooks_system_tests.rs`)

~1978 lines covering manager dispatch, async hooks, HTTP hooks,
timeout behavior, exit-code semantics (block / continue / deny).

### Gap: no fixture exercising every variant

Before this audit, no single test iterated over **all** `HookEvent`
variants to prove:
1. Every variant produces the correct `HookEventType`
2. Every variant has a non-empty `match_subject()`
3. Every variant round-trips through JSON

A new fixture test in
`crates/shannon-core/src/hooks/events.rs::tests::every_variant_round_trips`
was added to lock this in. Any future addition to `HookEvent` must
extend the fixture or the test will fail.

---

## Action items

- [x] Enumerate Shannon's `HookEventType`
- [x] Cross-reference Claude Code's official docs
- [x] Identify Claude-only events (`Setup`, `MessageDisplay`)
- [x] Identify Shannon-only events (`TeamTaskCreated/Completed`)
- [x] Identify dead events (5 listed above)
- [x] Add fixture test exercising all 30 variants
- [x] Wire `UserPromptExpansion` in `shannon-skills` template expander (P1-2a, b2b28f9b)
- [x] Wire `InstructionsLoaded` in `shannon-core` instruction loader (P1-2b, b2b28f9b)
- [x] Wire `ConfigChange` in config reload path via `notify` v7 watcher (P1-2c, b2b28f9b)
- [ ] Evaluate `Setup` event for Shannon's `--prompt` mode (follow-up)
- [ ] Defer `MessageDisplay` — needs rendering refactor
- [ ] Defer `Elicitation`/`ElicitationResult` — needs MCP UI bridge

## Sources

- [Claude Code Hooks Reference](https://code.claude.com/docs/en/hooks)
- [Claude Code Hooks Guide](https://code.claude.com/docs/en/hooks-guide)
- Shannon source: `crates/shannon-core/src/hooks/events.rs`
