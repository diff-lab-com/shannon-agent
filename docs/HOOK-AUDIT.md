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
has ~18" claim was outdated. Both systems are now at parity in count,
but Shannon has 5 **dead events** (defined but never emitted in
production code) that should be either wired or removed.

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
| `UserPromptExpansion` | ⚠️ dead | ✓ | Defined but never emitted |
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
| `ConfigChange` | ⚠️ dead | ✓ | Defined but never emitted |
| `InstructionsLoaded` | ⚠️ dead | ✓ | Defined but never emitted |
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

These five events are defined in `HookEventType` and have `HookEvent`
variants but **no production code path emits them** (verified by
`grep -rhoE 'HookEvent::[A-Z][a-zA-Z]+'` across all production source).

| Event | Where it should fire | Effort to wire |
|-------|---------------------|----------------|
| `UserPromptExpansion` | `shannon-skills` and `shannon-commands` after command template expansion (`$ARGUMENTS`, `$FILE_PATH`, etc. resolved) | Low (~1h) — single emit point in the template expander |
| `ConfigChange` | `shannon-core::config::Config` reload path when `.shannon.toml` changes on disk | Medium (~3h) — needs file watcher + reload trigger |
| `InstructionsLoaded` | `shannon-core::instructions::InstructionsLoader` after CLAUDE.md / `.claude/rules/*.md` are parsed and merged | Low (~1h) — already loaded, just emit |
| `Elicitation` | `shannon-mcp::process_pool` when an MCP server sends `elicitation/create` | Medium (~4h) — MCP elicitation flow needs UI bridge |
| `ElicitationResult` | Same flow, after user responds | Same as above |

**Recommendation:** wire `UserPromptExpansion`, `InstructionsLoaded`,
`ConfigChange` in this audit pass (low effort, no new infra). Track
`Elicitation` / `ElicitationResult` as a follow-up since they require
the MCP elicitation UI bridge which doesn't exist yet.

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
- [ ] Wire `UserPromptExpansion` in `shannon-skills` template expander (follow-up)
- [ ] Wire `InstructionsLoaded` in `shannon-core` instruction loader (follow-up)
- [ ] Wire `ConfigChange` in config reload path (follow-up)
- [ ] Evaluate `Setup` event for Shannon's `--prompt` mode (follow-up)
- [ ] Defer `MessageDisplay` — needs rendering refactor
- [ ] Defer `Elicitation`/`ElicitationResult` — needs MCP UI bridge

## Sources

- [Claude Code Hooks Reference](https://code.claude.com/docs/en/hooks)
- [Claude Code Hooks Guide](https://code.claude.com/docs/en/hooks-guide)
- Shannon source: `crates/shannon-core/src/hooks/events.rs`
