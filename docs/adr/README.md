# Architecture Decision Records (ADRs)

This directory records decisions that shape Shannon Code's architecture.
Each ADR is a short Markdown file following the template below.

## When to write an ADR

Write an ADR when a decision:

- Has **cross-crate impact** (affects multiple modules or all callers)
- Is **hard to reverse** (data format, public API, build dependency)
- Has **plausible alternatives** that future maintainers would otherwise
  re-litigate without context
- Resolves a **disagreement** worth recording for posterity

Do **not** write an ADR for:

- Bug fixes (the commit message is the record)
- Routine refactors (clear from diff)
- Internal cleanups with one obvious approach

## Template

```markdown
# ADR-NNNN: Title

**Date**: YYYY-MM-DD
**Status**: Proposed | Accepted | Rejected | Superseded by ADR-MMMM
**Sprint**: Sprint N (or "continuous")

## Context

Why this decision is being made now. What constraint, problem, or
opportunity triggers it?

## Decision

What we decided. Be concrete: file paths, type names, behavioral
contracts. Future maintainers should be able to read this and know
exactly what was chosen.

## Consequences

- **Positive**: What we gain
- **Negative**: What we give up or risk
- **Neutral**: Side effects of note

## Alternatives Considered

Each alternative, with a one-line reason for rejection.

## Implementation References

Links to the code (files, symbols) where the decision lives.

## Open Questions

Things deferred to future work.
```

## Index

| ADR | Title | Status | Date |
|---|---|---|---|
| [0001](0001-scheduled-tasks-storage.md) | Scheduled Tasks Storage — SKILL.md + JSONL Partitioned History | Accepted | 2026-06-13 |

## Numbering

ADRs are zero-padded sequential (`0001`, `0002`, ...). Once accepted, an
ADR is immutable — if a decision is later reversed, mark it `Superseded
by ADR-MMMM` and write a new ADR rather than editing the old one.
