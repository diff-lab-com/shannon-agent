# Shannon Code — Feature Module Roadmap

> Generated: 2026-04-12
> Status: Draft, awaiting approval

This document maps out the planned but partially-implemented feature modules,
their current state, dependencies, and proposed implementation phases.

## Current State Summary

All modules listed below are **registered and reachable** from the main binary
(via `shannon-commands/src/builtin.rs` or `shannon-agents/src/lib.rs`), but
contain internal dead code — types, functions, or enum variants that are defined
but not yet exercised by the application flow.

---

## Phase 1: Core REPL Enhancements (Priority: P1)

### 1.1 `/diff` Command — Intelligent Diff Viewer
**File**: `crates/shannon-commands/src/builtin/diff.rs`
**Dead code**: `ChangeCategory` variants, `CategorizedChange` fields,
`DiffAnalysis` methods, `DiffPattern` regex fields

**Work needed**:
- Wire `ChangeCategory` classification into the diff output pipeline
- Implement `DiffAnalysis::summary()` for human-readable change summaries
- Connect `DiffPattern` regexes to actual diff parsing logic
- Add `has_test_changes()` detection for smart commit messages

**Dependencies**: None — standalone command

### 1.2 `/review_pr` Command — AI-Powered PR Review
**File**: `crates/shannon-commands/src/builtin/review_pr.rs`
**Dead code**: `ReviewSeverity` variants, `ReviewCategory` methods,
`ReviewSuggestion` fields, `PRAnalysis` methods

**Work needed**:
- Implement `ReviewCategory::from_str()` for configurable review focus
- Wire `ReviewSuggestion` into the LLM prompt for structured review output
- Connect `PRAnalysis` methods to actual git diff analysis
- Add severity-based filtering and display formatting

**Dependencies**: `diff.rs` (uses categorized diff analysis)

### 1.3 `/export` Command — Session Export
**File**: `crates/shannon-commands/src/builtin/export.rs`
**Dead code**: `ExportFormat` variants, `ExportOptions` fields,
`parse_export_args()`, `export_to_markdown()`, `export_to_json()`

**Work needed**:
- Implement `export_to_markdown()` with proper formatting
- Implement `export_to_json()` with structured output
- Wire `ExportOptions` into the command handler
- Add file attachment handling for exports

**Dependencies**: `session_transcript` module for conversation history

---

## Phase 2: Document Processing (Priority: P2)

### 2.1 `/pdf` Command — PDF Processing
**File**: `crates/shannon-commands/src/builtin/pdf.rs`
**Dead code**: `ImageFormat` variants, `PdfTable` fields and methods,
`PdfPage` extraction methods

**Work needed**:
- Implement `PdfTable::to_text()` for table extraction
- Wire `ImageFormat` into image extraction pipeline
- Add OCR integration for scanned PDFs
- Implement page-range selection via `PdfPage` methods

**Dependencies**: External `pdf` crate or similar PDF library

---

## Phase 3: Debug & Developer Tools (Priority: P2)

### 3.1 `/debug` Command — Debug Instrumentation
**File**: `crates/shannon-commands/src/builtin/debug.rs`
**Dead code**: `DebugCategory` variants, `LogLevel` variants

**Work needed**:
- Wire `DebugCategory` into debug command filtering
- Connect `LogLevel` to the `InternalLogger` in shannon-core
- Add profiling sub-command using timing instrumentation
- Implement log level switching at runtime

**Dependencies**: `internal_logging` module in shannon-core

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
| 2.1 pdf | High | Medium | Medium | 6th |
| 3.1 debug | Low | Medium | Low | 4th |
| 4.1 coordinator | High | High | High | 7th |
| 5.x core enhancements | Variable | Medium | Low | 5th |

---

## Guiding Principles

1. **Bottom-up**: Complete individual commands before building cross-cutting features
2. **Test-first**: Each module should have integration tests before declaring complete
3. **Dead code → living code**: Remove `#[allow(dead_code)]` annotations as modules are wired up
4. **No new dead code**: Each phase should fully connect all defined types before adding new ones
