# ADR 0002 — Sprint 5: Deepen MCP Integration

**Status**: Proposed  
**Date**: 2026-06-16  
**Theme**: 深化 MCP 集成 (Deepen MCP Integration)  
**Supersedes**: —  
**Related**: ADR 0001 (scheduled tasks storage)

## Context

Shannon's MCP backend (`crates/shannon-mcp`) is mature: 4 transports (stdio, SSE, streamable HTTP, WebSocket), full protocol coverage (tools, resources, prompts, completion, logging, sampling, elicitation, progress, cancellation), OAuth 2.1 with RFC 9728/8414 discovery and RFC 7591 DCR, auto-restart with backoff, and a `ServerStatus` state machine.

An audit (`Explore` agent, 2026-06-16) found that the **backend capability surface exceeds the user-facing surface**. Several MCP features work at the protocol layer but are invisible or inert in the REPL/TUI. This throttles the value users get from MCP servers that support those features.

## Problem

Concretely, today:

| Capability | Backend | User-facing (REPL/TUI) |
| --- | --- | --- |
| `tools/list` + `tools/call` | ✅ | ✅ auto-discovered as agent tools |
| `resources/list` + `resources/read` | ✅ | ❌ no REPL surface |
| `resources/subscribe` + `notifications/resources/updated` | ✅ | ❌ no live-update UI |
| `prompts/list` + `prompts/get` | ✅ | ❌ not exposed as slash commands |
| `completion/complete` | ✅ | ❌ not wired to REPL autocomplete |
| `logging/setLevel` + `notifications/message` | ✅ | ❌ no log panel |
| `sampling/createMessage` | ✅ provider wired | ⚠️ silent (no user consent UX) |
| `elicitation/create` | ✅ provider scaffolded | ❌ **auto-declines** — `repl/mod.rs:813` |
| `.mcpb` bundle install | ❌ not in CLI | ✅ in shannon-desktop only |

The single worst gap is elicitation: servers that ask the user a question get a hard "no" back with no chance to respond. Second worst is prompts: a server author who publishes a `code-review` prompt has no way for Shannon users to discover or invoke it.

## Decision

Sprint 5 will execute **four user-facing deepening deliverables** that close the highest-value gaps without expanding backend scope. Each is sized to land in one sprint.

### S5-1 — Elicitation TUI wiring (must-have)

Wire `make_elicitation_provider` (`crates/shannon-ui/src/repl/mod.rs:813`) to a TUI input dialog instead of returning auto-decline. Servers will be able to ask the user "Branch name?" / "Risk tolerance?" and get a real answer.

**Why now**: today every elicitation request silently fails. This blocks an entire class of interactive MCP servers.

**Scope**:
- Reuse the existing `input_dialog` widget (or a thin variant).
- Provider returns `ElicitationResult::{Accepted(value), Declined, Cancelled}`.
- Permission gate: first use per server shows a one-time consent prompt.
- Tests: unit-test the provider in isolation; add an MCP fixture server that elicits and verifies round-trip.

**Out of scope**: multi-field forms (only single-line / multi-line text for now).

### S5-2 — MCP Prompts as slash commands (high-value)

Expose `prompts/list` from configured servers as REPL slash commands in the form `/<server>:<prompt-name>`. Invoking runs `prompts/get` and submits the rendered messages to the active conversation.

**Why now**: prompts are the second pillar of MCP after tools, and today they are invisible.

**Scope**:
- New `PromptCommand` source in `shannon-commands` `CommandRegistry` (mirrors how skill plugins register).
- Discovery polls every connected server's `prompts/list` on connect and refreshes on `/mcp refresh`.
- Argument parsing via the prompt's declared `arguments` schema → simple `--key value` mapping.
- Tests: fixture server with a prompt; verify `/myserver:code-review --file foo.rs` resolves and injects messages.

**Out of scope**: prompt picker UI in TUI (relies on tab-completion only for this sprint).

### S5-3 — Completion API wired to REPL input (quality-of-life)

Wire `completion/complete` so that `Tab` inside the REPL input asks the active server (and `/` slash command context) for completions. Falls back to local history when the server declines.

**Why now**: the backend exists; this is pure UX plumbing and removes a visible rough edge.

**Scope**:
- New `CompletionProvider` trait in `shannon-ui` with two impls: `HistoryProvider` (current) and `McpCompletionProvider` (delegates to `pool.completion()`).
- Debounce: 150 ms after last keystroke; cancel in-flight on new keystroke.
- Tests: fixture server that returns two completions; assert they render in the popup.

**Out of scope**: cross-server completion aggregation (single source per request, rotated by recency).

### S5-4 — `.mcpb` bundle install in CLI (parity)

Port the `.mcpb` installer from `shannon-desktop` (`ui/src/lib/mcp-installers/`) to a Rust crate so the CLI can install bundled stdio servers with signature verification.

**Why now**: shannon-desktop already supports `.mcpb`; CLI users currently have to hand-edit `.mcp.json`. This closes a feature gap between the two surfaces.

**Scope**:
- New `crates/shannon-mcp/src/bundle.rs`: parse `.mcpb` (zip), validate manifest, verify signature (reuses `shannon-mcp::auth::verify_signature`), extract to `~/.shannon/mcp-servers/<name>/`, write `mcpServers` entry into `.mcp.json`.
- New `shannon mcp install <path.mcpb>` subcommand.
- Tests: bundle with valid signature → installs and starts; bundle with mismatched signature → rejected with clear error.

**Out of scope**: GUI installer, marketplace browsing (CLI is a thin installer; marketplace UI stays in shannon-desktop).

### Deferred

- **Resources live-update UI** (deferred — backend works, UI is non-trivial and competes with S5-1 for TUI real estate).
- **MCP log panel** (deferred — overlaps with existing tracing layer).
- **Cross-server completion aggregation** (deferred — needs UX research).
- **Multi-field elicitation forms** (deferred — single-line covers ~80% of real servers).

## Consequences

- **Positive**: elicitations stop failing silently; MCP prompts get a discovery surface; REPL autocomplete becomes context-aware; CLI reaches parity with desktop for bundle installs.
- **Negative**: +1 new TUI dialog codepath (elicitation) that must be tested under terminal resize and raw-mode switches; +1 slash-command source that complicates tab-completion ordering.
- **Neutral**: four deliverables will land as four PRs to `dev`, merged to `main` at sprint end via the usual PR workflow.

## Execution plan

| ID | Deliverable | Estimated LOC | Test target |
| --- | --- | --- | --- |
| S5-1 | Elicitation TUI wiring | ~250 | +8 tests |
| S5-2 | MCP Prompts → slash commands | ~350 | +10 tests |
| S5-3 | Completion API → REPL | ~200 | +6 tests |
| S5-4 | `.mcpb` bundle install (CLI) | ~400 | +12 tests |

Total: ~1200 LOC, ~36 new tests, 4 PRs.

Each deliverable is independently shippable; if Sprint 5 runs long, S5-3 and S5-4 can be deferred without blocking S5-1/S5-2.

## Open questions

1. **Elicitation consent UX**: one-time consent per server, or global allow-list in `.shannon/settings.toml`? **Recommendation**: per-server, stored in `~/.shannon/mcp-consent.toml`.
2. **Prompt collision**: if two servers ship a prompt named `code-review`, what happens? **Recommendation**: namespace as `/<server>:<prompt>` always (already the plan), no de-duplication.
3. **Completion ordering**: when both local history and MCP server offer completions, which wins? **Recommendation**: MCP server wins when the cursor is inside a `/`-prefixed token; otherwise history wins.
