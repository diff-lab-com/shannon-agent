# Diff review loop

How Shannon Desktop's diff review loop works, how to use it, and how the
pieces fit together.

## What it is

When Shannon proposes a file edit, the chat tool result shows a "View diff"
button. Clicking it opens a modal that shows the proposed change as a
unified diff and lets you decide **per hunk** whether to accept, reject,
or leave it undecided.

The decision cycle on each hunk header is:

```
Undecided → Accepted → Rejected → Undecided → …
```

- **Undecided** (default) — hunk will NOT be applied. Treated as reject.
- **Accepted** — hunk WILL be written to disk on Apply.
- **Rejected** — hunk will NOT be written. Same effect as Undecided but
  signals you've reviewed it.

## Bulk controls

The review toolbar above the diff has three bulk actions:

- **Accept all** — set every hunk to Accepted.
- **Reject all** — set every hunk to Rejected. Apply button disables
  because no hunk will write.
- **Reset** — clear all decisions back to Undecided.

The `decided / total` counter shows progress.

## Apply

The footer's `Apply {N}` button (where N = accepted hunk count):

1. Computes the merged file content client-side via
   `lib/diff-merge.ts::mergeFile(old, new, decisions)`.
2. Writes the result via the existing `save_text_file` Tauri command.
3. Toasts success/failure via `sonner`.
4. Closes the modal on success.

Apply is **disabled** until at least one hunk is Accepted. If all hunks
are Rejected/Undecided, there's nothing to write — Cancel instead.

## Keyboard

- **Esc** — close the modal (same as Cancel / click outside).
- (M3, planned: j/k to navigate hunks, a/r/u to accept/reject/undecided,
  Enter to Apply.)

## Why client-side merge

The Rust `apply_diff` command has different semantics — it blanks out
rejected line ranges from the on-disk file but has no concept of an
AI-proposed `new_content`. Redesigning the Rust schema would have
ballooned M1 scope.

Instead, the desktop shell:

1. Asks the engine for a `FileDiff` (`old_content` + `new_content`).
2. Computes hunks client-side via the `diff` npm package (`diffArrays`
   for correct line-level context — `diffLines` over-groups adjacent
   edits).
3. Lets the user pick per hunk.
4. Writes the merged result with `save_text_file`.

Zero Rust changes. When the engine pin bumps past P1 and `apply_diff` is
redesigned, the merge logic may move into Rust. Until then,
`lib/diff-merge.ts` is the source of truth.

## Hunk IDs

Hunk IDs are content-addressed: `${oldStart}-${oldEnd}-${newStart}-${newEnd}`.
They survive re-renders as long as the underlying diff doesn't shift.
Git-style anchoring: pure insertion after old line N has
`oldStart = N+1, oldEnd = N` (empty range, end < start).

## Files

- `ui/src/lib/diff-merge.ts` — pure functions: `computeHunks`, `mergeFile`.
- `ui/src/components/diff/DiffViewer.tsx` — presentational controlled
  component.
- `ui/src/components/diff/DiffDialog.tsx` — owns decisions state, wires
  Apply, toasts.
- `ui/src/__tests__/diff-merge.test.ts` — 18 unit tests.
- `ui/src/__tests__/DiffViewer.test.tsx` — 12 component tests.
- `ui/src/__tests__/DiffDialog.test.tsx` — 10 integration tests including
  Apply flow.

## Roadmap

- **M1** (shipped 2026-06-23, PR pending): single-file review, per-hunk
  controls, Apply flow.
- **M2** (next): multi-file batch review with file list sidebar.
- **M3**: chat wiring, keyboard shortcuts, context collapse, a11y audit.
- **M4**: Playwright E2E + this doc.
