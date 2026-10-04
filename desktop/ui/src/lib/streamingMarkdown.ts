// streamingMarkdown — incremental split for the live-streaming Markdown
// renderer (docs/plans/2026-10-05-chat-r3-improvement-plan.md §三 P1-6, §六 B3-2).
//
// StreamingResponse used to hand the WHOLE accumulated `streamingText` to
// `<Markdown>` on every ~50ms flush, re-running remark-gfm + remark-math +
// rehype-highlight + sanitize + rehype-katex over the full text each time —
// O(n²) cumulative work that grows with reply length. The fix: cut the text
// at the last safe block boundary into a **finalized prefix** (stable across
// flushes, so a memoized component skips it entirely) and an **active tail**
// (small, re-parsed per flush).
//
// Soundness of a cut rests on one markdown property: a blank line always
// ends the current block, so content after a blank run cannot modify how a
// preceding block parses — except for the constructs this scanner tracks and
// the boundary rules below reject:
//  - fenced code blocks (``` / ~~~) may CONTAIN blank lines → a run inside
//    an open fence is never a cut;
//  - `$$` flow math may span blank lines too (remark-math's mathFlow, proven
//    by probe) → tracked exactly like fences;
//  - a `>`-only line keeps a blockquote open (`> a` / `>` / `> b` is ONE
//    quote with two paragraphs) — such lines are content, not blanks, so
//    they simply never form a boundary;
//  - loose lists / list-item continuation blocks put blank lines *inside*
//    one list → boundaries with list markers or indented continuations on
//    both sides are rejected;
//  - indented code blocks may contain blank lines → a blank run between two
//    ≥4-space-indented lines is rejected;
//  - GFM tables cannot contain blank lines at all, so a boundary never
//    lands inside one — a mid-construction table simply stays wholly in the
//    tail until its terminator blank arrives (tests pin this).
//
// Residual (accepted, streaming-only, resolved once the finalized
// MessageBubble re-renders the full text): cross-block references — a link
// reference definition or footnote definition arriving in the tail cannot
// retroactively link tokens already parsed in the prefix.
//
// The invariant `prefix + tail === text` holds for every input, and the
// prefix is append-stable: it only ever grows as the stream appends.

export interface StreamingMarkdownSplit {
  /** Finalized leading blocks (possibly ''). Ends with the blank run when
   *  non-empty, so re-joining with `tail` reproduces the input verbatim. */
  prefix: string
  /** The active remainder being appended to; re-parsed on every flush. */
  tail: string
}

/** A `-`/`*`/`+`/`1.`-style list-item line (marker + space, or bare marker). */
function isListItemLine(line: string): boolean {
  return /^ {0,3}(?:[-+*]|\d{1,9}[.)])(?:[ \t].*)?$/.test(line)
}

/**
 * Is a cut between `prev` (last line before the blank run) and `next` (first
 * line after it) guaranteed to be a true block boundary? Rejects the two
 * blank-line-inside-a-block shapes markdown has outside fences: loose-list
 * items / continuation blocks, and indented-code interiors.
 */
function isSafeBoundary(prev: string, next: string): boolean {
  const prevMarker = isListItemLine(prev)
  if (prevMarker) {
    // `1. item` / blank / `2. item` — one loose list; and `1. item` / blank
    // / `   indented` — a multi-block list item. Both must stay whole.
    if (isListItemLine(next) || /^[ \t]/.test(next)) return false
  }
  // Interior of an indented (4-space) code block, which may span blank lines.
  if (!prevMarker && /^[ \t]{4}/.test(prev) && /^[ \t]{4}/.test(next)) return false
  return true
}

/**
 * Split accumulated streaming markdown into its finalized prefix and the
 * active tail, cutting at the LAST safe blank-line boundary (largest prefix,
 * smallest tail). With no safe boundary yet — single paragraph so far, or an
 * open fence/math/table swallowing everything — the whole text stays in the
 * tail. Always returns `{ prefix: '', tail: text }`-shaped results with
 * `prefix + tail === text`.
 */
export function splitStreamingMarkdown(text: string): StreamingMarkdownSplit {
  if (text === '') return { prefix: '', tail: '' }

  const lines = text.split('\n')
  // Start offset of every line, so cuts slice the original text exactly.
  const starts: number[] = new Array(lines.length)
  let offset = 0
  for (let i = 0; i < lines.length; i++) {
    starts[i] = offset
    offset += lines[i].length + 1
  }

  // Scanner state: open ``` / ~~~ fence (char + minimum close length), and
  // open `$$` flow math (same length rule — probes confirmed remark-math's
  // mathFlow closes on a same-or-longer `$` fence line, ≤3 spaces indented).
  let fence: { ch: string; len: number } | null = null
  let mathLen = 0
  let bestCut = -1

  let i = 0
  while (i < lines.length) {
    const line = lines[i]

    if (fence) {
      const close = /^ {0,3}(`{3,}|~{3,})\s*$/.exec(line)
      if (close && close[1][0] === fence.ch && close[1].length >= fence.len) fence = null
      i++
      continue
    }
    if (mathLen > 0) {
      const close = /^ {0,3}(\$+)\s*$/.exec(line)
      if (close && close[1].length >= mathLen) mathLen = 0
      i++
      continue
    }

    if (/^\s*$/.test(line)) {
      // Blank run [b0..b1]; the candidate cut sits before the first line
      // after it. A `>`-only line is NOT blank (it keeps a blockquote open),
      // so quote interiors never produce a run to begin with.
      const b0 = i
      let b1 = i
      while (b1 + 1 < lines.length && /^\s*$/.test(lines[b1 + 1])) b1++
      const next = b1 + 1
      if (next < lines.length && isSafeBoundary(b0 > 0 ? lines[b0 - 1] : '', lines[next])) {
        bestCut = starts[next]
      }
      i = next
      continue
    }

    // Fence opener. Backtick fences additionally require a backtick-free
    // info string (CommonMark) — treating ```` ```a`b ```` as a fence would
    // desynchronize the scanner from the real parser and risk a cut inside
    // what remark still considers live paragraph text.
    const backtick = /^ {0,3}(`{3,})[^`]*$/.exec(line)
    if (backtick) {
      fence = { ch: '`', len: backtick[1].length }
      i++
      continue
    }
    const tilde = /^ {0,3}(~{3,})/.exec(line)
    if (tilde) {
      fence = { ch: '~', len: tilde[1].length }
      i++
      continue
    }

    // `$$` flow math (two or more `$` — a single leading `$` is just a
    // paragraph, probe-verified): a line whose leading `$` run is never
    // closed later on the same line opens a block that may span blank lines
    // (`$$x$$` on one line is self-contained inline math — not flow).
    const math = /^ {0,3}(\$\$+)/.exec(line)
    if (math && !line.slice(math[1].length).includes('$$')) {
      mathLen = math[1].length
      i++
      continue
    }

    i++
  }

  if (bestCut < 0) return { prefix: '', tail: text }
  return { prefix: text.slice(0, bestCut), tail: text.slice(bestCut) }
}
