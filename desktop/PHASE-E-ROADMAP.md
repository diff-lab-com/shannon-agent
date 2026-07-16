# Phase E Roadmap

Status: **Draft, June 2026** — captures remaining Tier-3 / future work after Phase D P1–P4 shipped.

## Phase D status recap

P1/P2 deep features (subagent configs, permission profiles, routines, IDE
extension scaffold) and P3/P4 differentiators (NL cron, schedule templates,
DAG, hook→task pipeline, OPC analytics, LSP quick-fix panel) are merged.
562 UI tests pass. Rust backend clean.

## E1 — Code editor with diagnostic squiggles

**Problem.** The `/quickfix` launcher requires users to manually paste a file
path + line + message. Real UX needs a code viewer that:
1. Renders source with syntax highlighting
2. Surfaces diagnostics inline (rustc / clippy / tsc / eslint)
3. Wires LspQuickFixPanel to each squiggle on click

**Approach.**
- Adopt Monaco or CodeMirror 6 (MIT, lightweight)
- Subscribe to `shannon_core::lsp::DiagnosticRegistry` (already exists)
- Render diagnostic markers as gutter decorations
- Click handler opens `<LspQuickFixPanel diagnostic={...} />` in a side drawer

**Scope.** ~3-5 days. Single-file viewer first; multi-tab later.

**Risk.** Monaco bundle is ~3MB. CodeMirror 6 is ~150KB. Recommend CM6.

## E2 — macOS Accessibility API computer-use

**Problem.** Current `ComputerUseTool` uses screenshots + pixel coordinates
(`xcap` + `enigo`). The OpenAI Codex desktop app (via Sky acquisition) uses
the macOS Accessibility API instead: precise element targeting, independent
virtual cursors per agent (no focus stealing), lock-screen-protected
execution.

**Approach.**
- Audit `accessibility::UIElement` Rust crates (`accessibility-sys`,
  `accessibility` by Matt Brenton)
- Prototype `click_element(role, label)` action that hits AX API
- Compare latency / accuracy vs screenshot path
- Gate behind `computer-use-ax` feature flag

**Scope.** ~8-12 weeks (per CLAUDE.md estimate).

**Risk.** macOS-only, conflicts with Shannon's cross-platform positioning.
Need to assess ROI after P3/P4 adoption data — recommend revisiting Q4 2026.

**Recommendation: defer.**

## E3 — VS Code extension polish

**Problem.** Current scaffold has WebView chat panel + NDJSON subprocess
comms with `shannon --prompt`. Lacks LSP-style language features and
inline completions.

**Approach.**
- Migrate WebView to native VS Code Chat API (proposed API, requires
  pre-release track)
- Add inline ghost completions via `InlineCompletionItemProvider`
- Wire diagnostic code actions to VS Code's `CodeActionProvider` so fixes
  surface in the native lightbulb menu
- Implement workspace symbol provider backed by Shannon's MCP tools

**Scope.** ~2 weeks for native Chat API migration; ~1 week each for
inline completions and code action wiring.

**Risk.** Proposed API churn — VS Code may break us on update. Mitigation:
pin engine version, ship via OpenVSX first.

## E4 — Hook event coverage audit

**Problem.** Shannon has 32 hook events; Claude Code has ~18. We claim
broader coverage but no formal audit confirms parity.

**Approach.**
- Enumerate every hook event in `shannon_core::hooks::HookEventType`
- Cross-reference Claude Code's documented hook list
- Identify gaps in either direction
- Add fixture tests that fire each hook event with realistic payloads

**Scope.** ~3 days.

## E5 — Performance instrumentation

**Problem.** Benchmarks for `get_opc_metrics` (52ms) and `lsp_code_actions`
(46ms) are well under budget, but only measured on 168 tasks. Real users
may have larger workspaces, slower disks, or weaker CPUs.

**Approach.**
- Add `tracing` spans to every Tauri command
- Wire `tracing-subscriber` with a JSON exporter for offline analysis
- Synthetic load tests: 1000 tasks, 10k lines of code, 50 concurrent
  background tasks

**Scope.** ~1 week.

## Priority ordering

| # | Item | Effort | Impact | Recommendation |
|---|------|--------|--------|----------------|
| E1 | Code editor + diagnostics | 3-5d | High | **Start here** |
| E4 | Hook event audit | 3d | Medium | Do alongside E1 |
| E3 | VS Code extension polish | 2-3w | Medium | After E1 |
| E5 | Performance instrumentation | 1w | Low | When adoption grows |
| E2 | macOS AX computer-use | 8-12w | Speculative | Defer to Q4 2026 |

## Non-goals

These were considered and rejected:
- **Bundle LSP servers** — out of scope; users install rust-analyzer etc.
- **Mobile app** — Shannon is desktop-first
- **Web-only mode** — Tauri is core to architecture
- **LLM training/fine-tuning** — provider-agnostic by design
