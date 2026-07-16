# Phase 1 (Q3 2026) progress

Last updated: 2026-06-23

Phase 1 is the "consolidate home ground" quarter — Code & Dev 2→3,
Workflow stays 3/3. Six tasks (P1.1-P1.6), milestone v0.5.0 (2026-09).

Source: `.omc/research/roadmap-2026-06/03-shannon-gap-roadmap.md` §5.1.

## Status

| ID | Task | Capability | Effort | Status | Branch / PR |
|----|------|------------|--------|--------|-------------|
| P1.1 | Diff review loop UI | Code & Dev | 3 wk | **M1 shipped, M2-M4 in progress** | `s2/p1.1-diff-review-m1` (M1 PR pending), `s2/p1.1-diff-review-m2` (M2+ branch) |
| P1.2 | Workflow streaming | Workflow | 2 wk | TODO | — |
| P1.3 | Slack/Telegram outbound | Workflow | 2 wk | TODO | — |
| P1.4 | Routine templates library | Workflow | 2 wk | TODO | — |
| P1.5 | MCP-remote OAuth loopback | Code & Dev | 2 wk | TODO | — |
| P1.6 | First-run wizard | Global UX | 2 wk | **Shipped 2026-06-23** | `s2/p1.6-first-run-wizard` (PR pending merge) |

## P1.1 milestones

| Milestone | Scope | Status |
|-----------|-------|--------|
| M1 — single-file | Hunk compute + per-hunk controls + Apply | ✅ Shipped (commits `f023a57`, `530cef0`) |
| M2 — multi-file | File list sidebar + bulk ops + Apply all | 🔄 In progress |
| M3 — polish | Chat wiring + persistence + keyboard + a11y | TODO |
| M4 — verification | Playwright E2E + docs | TODO |

## Dependencies

- M2+ branches include M1 changes via `git merge origin/s2/p1.1-diff-review-m1`.
- P1.5 (MCP OAuth) requires a real OAuth provider for end-to-end tests —
  plan to mock in CI and manual-test against a sample MCP server.
- P1.2 (streaming) touches `scheduled_commands.rs` which is also being
  modified by ongoing `commands.rs` split work — coordinate branch
  ordering.

## Risks

- **PR merge lag**: M1 and P1.6 are both pending PR merge to `dev`.
  M2 work proceeds on a separate branch with M1 merged in; rebasing
  may be needed after merge.
- **CI can't reach github.com**: All Rust gates run via
  `scripts/local-check.sh` (pre-push hook); CI is UI-only.
- **`tea` CLI auth**: Currently can't create PRs programmatically. PRs
  opened via web UI. Fix: configure `tea login --token`.

## Out of scope for Phase 1

- Multi-file workspace edits (P2+).
- Diff for binary files.
- Inline diff in chat (vs modal).
- Cross-session decision persistence.
