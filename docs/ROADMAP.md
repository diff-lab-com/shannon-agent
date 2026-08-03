# Shannon Code — Feature Module Roadmap

> Generated: 2026-04-12
> Status: Draft, awaiting approval
> **Update (2026-08-03, P1-1 dead-code cleanup)** — Phase 1.1–1.3 and 2.1
> (`/diff`, `/review_pr`, `/export`, `/debug`) are now wired. See commit
> history on `feat/p1-1-dead-code`. The `/pdf` command was removed (file
> and registration). The remaining dead-code items in this document
> (Phase 4.1 Agent Coordinator, Phase 5.2-5.4) are deferred per the
> improvement plan §P1.

This document maps out the planned but partially-implemented feature modules,
their current state, dependencies, and proposed implementation phases.

## Current State Summary

All modules listed below are **registered and reachable** from the main binary
(via `shannon-commands/src/builtin.rs` or `shannon-agents/src/lib.rs`), but
contain internal dead code — types, functions, or enum variants that are defined
but not yet exercised by the application flow.

Phase 1.1–1.3 and Phase 2.1 below are exceptions: those `/diff`,
`/review_pr`, `/export`, and `/debug` commands are now wired end-to-end
(2026-08-03, P1-1). Each file may still carry `// KEEP:` markers for
forward-compatibility scaffolding; none of the entry points or types are
unreachable. `/pdf` was deleted entirely (file, registration, ROADMAP
entry).

---

## Phase 1: Core REPL Enhancements (Priority: P1)

### 1.1 `/diff` Command — Intelligent Diff Viewer — **WIRED (P1-1)**
**File**: `crates/shannon-commands/src/builtin/diff.rs`
**Status**: Done. `DiffAnalyzer` compiles `patterns::*` regexes and
feeds them through `categorize_line` for every diff line; `DiffAnalysis`
exposes `summary()`, `has_test_changes()`, `has_doc_changes()`,
`has_config_changes()`, and `commit_summary()` (conventional-commit
string). `analyze_diff()` is the canonical end-to-end entry; the
`run_diff_analysis()` REPL output formatter consumes it.

### 1.2 `/review_pr` Command — AI-Powered PR Review — **WIRED (P1-1)**
**File**: `crates/shannon-commands/src/builtin/review_pr.rs`
**Status**: Done. `ReviewCategory::FromStr` and `IssueSeverity::FromStr`
reject unknown inputs with descriptive errors. `ReviewSuggestion` is the
structured-output row (`from_issue` + `to_json`); `REVIEW_PROMPT_SCHEMA_FRAGMENT`
appended to every prompt instructs the model to emit
`{"suggestions": [...]}`. `ReviewResult::filter_by_severity(threshold)`
filters issues; `suggestions_as_json()` groups suggestions for
downstream consumers. `run_pr_analysis()` already wired to
`gh pr view` / `gh pr diff`.

### 1.3 `/export` Command — Session Export — **WIRED (P1-1)**
**File**: `crates/shannon-commands/src/builtin/export.rs`
**Status**: Done. `export_to_markdown()` and `export_to_json()` are
implemented with `ExportOptions` plumbing
(`include_metadata` / `include_timestamps` / `sanitize`). New adapter
functions `export_session_from_transcript`, `export_session_from_store`,
and `maybe_build_session_from_args` pull live history from
`shannon_core::session_transcript::TranscriptStore`. File attachment
handling is left for P2-5c (chat upgrade).

---

## Phase 2: Debug & Developer Tools (Priority: P2)

### 2.1 `/debug` Command — Debug Instrumentation — **WIRED (P1-1)**
**File**: `crates/shannon-commands/src/builtin/debug.rs`
**Status**: Done. `LogLevel` derives `PartialOrd` + `Ord` and `FromStr`;
`to_internal_log_level` bridges to `shannon_core::internal_logging::InternalLogLevel`
(Trace→Debug collapse is deliberate). Process-wide runtime override
via `set_runtime_log_level` / `current_log_level` satisfies "runtime log
level switching at runtime"; resolution order is explicit override →
`SHANNON_LOG_LEVEL` env → `RUST_LOG` hint → `Info` default.
`filter_internal_entries_below` queries the in-memory `InternalLogger`
buffer. The `DebugCategory` enum the plan referenced is named
`DebugSubcommand` in current code (`Log`/`Profile`/`Trace`/`Info`/`Help`);
filtering goes through `parse_debug_subcommand` + `parse_log_level`.

---

## Phase 4: Multi-Agent Coordination (Priority: P3)

### 4.1 Agent Coordinator
**File**: `crates/shannon-agents/src/coordinator.rs`
**Dead code**: `AgentTeam` fields, task assignment methods

**Work needed**:
- Implement `AgentTeam` task distribution logic
- Connect `assignment_index` to the task queue
- Add team-based conversation routing
- Implement parallel execution with result aggregation

**Dependencies**: `team_memory_sync` module, `bridge_service` module

---

## Phase 5: shannon-core Internal Enhancements (Priority: P3)

These are types/methods defined in shannon-core modules that are exported but
not yet called from the application layer:

### 5.1 Query Engine Internal Methods
- `QueryEngine` has analysis methods that aren't invoked from the REPL loop
- Need: Wire these into the conversation processing pipeline

### 5.2 Compact Engine Strategies
- `CompactStrategy` enum variants defined but only default strategy used
- Need: Implement token-based and summary-based compaction strategies

### 5.3 Doctor Command Expansion
- `DoctorError` variants for checks not yet implemented
- Need: Add filesystem, network, and configuration health checks

### 5.4 UI Adapter Integration
- `UiAdapter` trait and `DefaultUiAdapter` defined but not wired into TUI
- Need: Connect to the ratatui-based UI layer

---

## Implementation Priority Matrix

| Phase | Effort | Impact | Risk | Recommended Order |
|-------|--------|--------|------|-------------------|
| 1.1 diff | Medium | High | Low | 1st |
| 1.2 review_pr | Medium | High | Low | 2nd |
| 1.3 export | Low | Medium | Low | 3rd |
| 2.1 debug | Low | Medium | Low | 4th |
| 4.1 coordinator | High | High | High | 7th |
| 5.x core enhancements | Variable | Medium | Low | 5th |

---

## Guiding Principles

1. **Bottom-up**: Complete individual commands before building cross-cutting features
2. **Test-first**: Each module should have integration tests before declaring complete
3. **Dead code → living code**: Remove `#[allow(dead_code)]` annotations as modules are wired up
4. **No new dead code**: Each phase should fully connect all defined types before adding new ones
