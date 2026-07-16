// Pure-function line diff + selective merge for the Diff review loop (P1.1 M1).
//
// The Rust `apply_diff` command only knows how to blank-out line ranges from
// the current on-disk file — it has no concept of AI-proposed `new_content`.
// Rather than redesign the Rust schema, we compute hunks client-side and
// write the merged result via `save_text_file`, which already exists. Zero
// Rust changes.
//
// Hunk IDs are content-addressed (oldStart-oldEnd-newStart-newEnd) so React
// keys stay stable across re-renders as long as the underlying diff doesn't
// shift.

import { diffArrays, type Change } from 'diff'

export type HunkDecision = 'accept' | 'reject' | 'pending'

export type DiffLineType = 'context' | 'added' | 'removed'

export interface DiffLine {
  type: DiffLineType
  text: string
  oldLineNo: number | null
  newLineNo: number | null
}

export interface Hunk {
  /** Stable content-addressed id — survives re-renders. */
  id: string
  /** 1-indexed, inclusive — matches git convention. */
  oldStart: number
  /** 1-indexed, inclusive. */
  oldEnd: number
  newStart: number
  newEnd: number
  lines: DiffLine[]
}

interface RawLine {
  text: string
  type: DiffLineType
  oldLineNo: number | null
  newLineNo: number | null
}

type ArrayChange = Change & { value: string[] }

function changeToType(change: ArrayChange): DiffLineType {
  if (change.added) return 'added'
  if (change.removed) return 'removed'
  return 'context'
}

function expandChanges(changes: ArrayChange[]): RawLine[] {
  const out: RawLine[] = []
  let oldNo = 1
  let newNo = 1
  for (const change of changes) {
    const type = changeToType(change)
    for (const text of change.value) {
      let oldLineNo: number | null = null
      let newLineNo: number | null = null
      if (type === 'context') {
        oldLineNo = oldNo
        newLineNo = newNo
        oldNo += 1
        newNo += 1
      } else if (type === 'added') {
        newLineNo = newNo
        newNo += 1
      } else {
        oldLineNo = oldNo
        oldNo += 1
      }
      out.push({ text, type, oldLineNo, newLineNo })
    }
  }
  return out
}

/**
 * Group expanded diff lines into hunks. A hunk is a maximal run of
 * consecutive non-context lines, bounded by the context lines around them.
 * Pure-insertion hunks (only `added` lines) and pure-deletion hunks
 * (only `removed` lines) are each their own hunk; a replacement is a
 * single hunk containing both the removed and added lines in order.
 *
 * Anchoring convention (matches `git diff --unified=0`):
 * - Pure insertion after old line N: `oldStart = N+1, oldEnd = N`
 *   (empty range — end < start signals "no old-side lines").
 * - Pure deletion at new line M: `newStart = M+1, newEnd = M`.
 */
function groupHunks(lines: RawLine[]): Hunk[] {
  const hunks: Hunk[] = []
  let i = 0
  let prevOldNo = 0
  let prevNewNo = 0
  while (i < lines.length) {
    const current = lines[i]
    if (current.type === 'context') {
      if (current.oldLineNo !== null) prevOldNo = current.oldLineNo
      if (current.newLineNo !== null) prevNewNo = current.newLineNo
      i += 1
      continue
    }
    const start = i
    while (i < lines.length && lines[i].type !== 'context') {
      i += 1
    }
    const segment = lines.slice(start, i)
    const removedLines = segment.filter(l => l.type === 'removed')
    const addedLines = segment.filter(l => l.type === 'added')

    let oldStart: number
    let oldEnd: number
    if (removedLines.length > 0) {
      oldStart = removedLines[0].oldLineNo!
      oldEnd = removedLines[removedLines.length - 1].oldLineNo!
    } else {
      // Pure insertion: anchor after the last old-side line we saw.
      oldStart = prevOldNo + 1
      oldEnd = prevOldNo
    }

    let newStart: number
    let newEnd: number
    if (addedLines.length > 0) {
      newStart = addedLines[0].newLineNo!
      newEnd = addedLines[addedLines.length - 1].newLineNo!
    } else {
      // Pure deletion: anchor after the last new-side line we saw.
      newStart = prevNewNo + 1
      newEnd = prevNewNo
    }

    if (removedLines.length > 0) prevOldNo = oldEnd
    if (addedLines.length > 0) prevNewNo = newEnd

    hunks.push({
      id: `${oldStart}-${oldEnd}-${newStart}-${newEnd}`,
      oldStart,
      oldEnd,
      newStart,
      newEnd,
      lines: segment.map(l => ({
        type: l.type,
        text: l.text,
        oldLineNo: l.oldLineNo,
        newLineNo: l.newLineNo,
      })),
    })
  }
  return hunks
}

export function computeHunks(oldContent: string, newContent: string): Hunk[] {
  if (oldContent === newContent) return []
  // diffArrays on split lines gives correct line-level context matching,
  // unlike diffLines which can over-group adjacent edits. The trailing \n
  // (if present) becomes an empty trailing string after split; strip it
  // so a 3-line file produces exactly 3 entries.
  const oldLines = splitToLines(oldContent)
  const newLines = splitToLines(newContent)
  const changes = diffArrays(oldLines, newLines) as ArrayChange[]
  const lines = expandChanges(changes)
  return groupHunks(lines)
}

function splitToLines(content: string): string[] {
  if (content === '') return []
  const lines = content.split('\n')
  if (lines.length > 0 && lines[lines.length - 1] === '') lines.pop()
  return lines
}

/**
 * Merge `oldContent` and `newContent` according to per-hunk decisions.
 *
 * - `accept` → emit the hunk's new lines
 * - `reject` → emit the hunk's old lines
 * - `pending` → treated as `reject` (no change until the user commits)
 *
 * Hunks with no entry in `decisions` are treated as `pending` / `reject`.
 *
 * The result is a complete file string ready for `saveTextFile`.
 */
export function mergeFile(
  oldContent: string,
  newContent: string,
  decisions: Map<string, HunkDecision>,
): string {
  const hunks = computeHunks(oldContent, newContent)
  const oldLines = splitToLines(oldContent)
  const newLines = splitToLines(newContent)
  const changes = diffArrays(oldLines, newLines) as ArrayChange[]
  const lines = expandChanges(changes)
  // Walk `lines` linearly. When we cross from context → non-context, we've
  // entered a hunk; look up the decision and emit the chosen side.
  const out: string[] = []
  let i = 0
  while (i < lines.length) {
    const line = lines[i]
    if (line.type === 'context') {
      out.push(line.text)
      i += 1
      continue
    }
    const hunkStartIdx = i
    while (i < lines.length && lines[i].type !== 'context') {
      i += 1
    }
    const segment = lines.slice(hunkStartIdx, i)
    const hunk = hunks.find(h =>
      h.lines.length === segment.length &&
      h.lines.every((hl, idx) =>
        hl.type === segment[idx].type &&
        hl.text === segment[idx].text
      )
    )
    const decision = hunk ? (decisions.get(hunk.id) ?? 'pending') : 'pending'
    const emitNew = decision === 'accept'
    for (const l of segment) {
      if (l.type === 'context') {
        out.push(l.text)
      } else if (l.type === 'added' && emitNew) {
        out.push(l.text)
      } else if (l.type === 'removed' && !emitNew) {
        out.push(l.text)
      }
      // added && reject → drop
      // removed && accept → drop
    }
  }
  return out.length > 0 ? out.join('\n') + '\n' : ''
}
