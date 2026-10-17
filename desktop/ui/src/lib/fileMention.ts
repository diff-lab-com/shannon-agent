// fileMention — GB P2-10b: the @ file-reference popover helpers.
//
// Typing `@token` at the composer (start of input or after whitespace —
// never mid-word, so email addresses stay untouched) opens a fuzzy file
// picker over the session's file universe: the working-dir tree
// (`get_file_tree`, backend-bounded at depth 12 / 5000 entries) plus the
// session file index (`list_file_index` — previously attached/favorited
// paths). Picking inserts the path as PLAIN TEXT (`@relative/path `) — the
// TUI's @-reference semantics, no new attachment state, the model simply
// reads the path in the message.

import type { FileNode } from '@/types'

/** A live `@query` in the composer text before the caret. */
export interface MentionQuery {
  /** The text after `@`, no `@` itself. */
  token: string
  /** Index of the `@` character in the full value. */
  startPos: number
  /** Exclusive end of the query (= caret position when detected). */
  endPos: number
}

// `@` only opens the menu at input start or after whitespace — mid-word `@`
// (user@example.com) never triggers.
const MENTION_RE = /(^|\s)@([^\s@]*)$/

/**
 * Detect the active `@query` in the text BEFORE the caret. Returns null
 * when no mention is being typed.
 */
export function activeMentionQuery(textBeforeCaret: string): MentionQuery | null {
  const m = MENTION_RE.exec(textBeforeCaret)
  if (!m) return null
  return {
    token: m[2],
    startPos: m.index + m[1].length,
    endPos: textBeforeCaret.length,
  }
}

/**
 * Replace the active `@query` with the picked path (kept `@`-prefixed, one
 * trailing space so the next keystroke starts fresh — skipped when the text
 * after the caret already begins with whitespace, so `@token file` never
 * becomes `@path  file`). Everything after the caret survives.
 */
export function insertMention(
  value: string,
  query: MentionQuery,
  path: string,
  workingDir: string | null | undefined,
): string {
  const rest = value.slice(query.endPos)
  const separator = rest.length > 0 && /^\s/.test(rest) ? '' : ' '
  const insertion = `@${relativeToWorkingDir(path, workingDir)}${separator}`
  return value.slice(0, query.startPos) + insertion + rest
}

/** Position of the caret after an insert — `@` + relative path + space. */
export function caretAfterMentionInsert(query: MentionQuery, path: string, workingDir: string | null | undefined): number {
  return query.startPos + 1 + relativeToWorkingDir(path, workingDir).length + 1
}

/**
 * Insert a DIRECTORY mention (AI-chat-ux C4): `@rel/dir/` with NO trailing
 * space, so the whitespace-anchored query keeps matching and the menu
 * drills straight into the folder's children (the Cursor @Folders move).
 */
export function insertDirectoryMention(
  value: string,
  query: MentionQuery,
  path: string,
  workingDir: string | null | undefined,
): string {
  const rest = value.slice(query.endPos)
  const rel = relativeToWorkingDir(path, workingDir)
  const insertion = `@${rel}${rel.endsWith('/') ? '' : '/'}`
  return value.slice(0, query.startPos) + insertion + rest
}

/** Caret position after a directory mention insert (inside the trailing `/`). */
export function caretAfterDirectoryMentionInsert(query: MentionQuery, path: string, workingDir: string | null | undefined): number {
  const rel = relativeToWorkingDir(path, workingDir)
  return query.startPos + 1 + rel.length + (rel.endsWith('/') ? 0 : 1)
}

// ── AI-chat-ux C2/C3 — committed mentions as structured references ──────

/** One `@token` occurrence in the composer text. `start`/`end` bound the
 *  `@token` span itself (excluding the anchor whitespace). */
export interface MentionToken {
  raw: string
  start: number
  end: number
}

const MENTION_TOKEN_RE = /(^|\s)@([^\s@]+)/g

/**
 * Every @-mention token in the composer text (global form of
 * `activeMentionQuery`). The whitespace anchor keeps emails out, exactly
 * like the live menu trigger. Powers the composer's reference chips and
 * the send-time deterministic attachment merge (C3).
 */
export function mentionTokensIn(value: string): MentionToken[] {
  const out: MentionToken[] = []
  for (const m of value.matchAll(MENTION_TOKEN_RE)) {
    const start = m.index + m[1].length
    out.push({ raw: m[2], start, end: start + 1 + m[2].length })
  }
  return out
}

/**
 * Resolve a mention token to a real candidate path. Tokens are stored
 * relative to the working dir when possible (`insertMention`), so both the
 * verbatim candidate and its relative form match; absolute tokens match
 * directly.
 */
export function resolveMentionToken(
  token: string,
  candidates: string[],
  workingDir: string | null | undefined,
): string | null {
  for (const c of candidates) {
    if (c === token) return c
    if (relativeToWorkingDir(c, workingDir) === token) return c
  }
  return null
}

/**
 * Remove a mention token (the span INCLUDES its `@`), swallowing ONE
 * adjacent space (preceding side preferred, else trailing) so removal
 * never leaves double whitespace behind.
 */
export function removeMentionToken(value: string, token: MentionToken): string {
  let start = token.start
  let end = token.end
  if (start > 0 && value[start - 1] === ' ') start -= 1
  else if (end < value.length && value[end] === ' ') end += 1
  return value.slice(0, start) + value.slice(end)
}

/** Relative form when the path lives under the working dir, else verbatim. */
export function relativeToWorkingDir(path: string, workingDir: string | null | undefined): string {
  if (!workingDir) return path
  const root = workingDir.replace(/\\/g, '/').replace(/\/+$/, '')
  const norm = path.replace(/\\/g, '/')
  if (norm === root) return '.'
  if (norm.startsWith(`${root}/`)) return norm.slice(root.length + 1)
  return path
}

/** Directories whose subtrees are noise for a pick-list (the backend walk
 *  already bounds depth/entries; these keep the remaining budget on code). */
const IGNORED_DIRS = new Set(['node_modules', '.git', 'dist', 'build', 'target', 'coverage', '.next'])

/**
 * Flatten a `get_file_tree` response into file paths (pre-order). Accepts
 * the array shape the backend actually returns (root entries with nested
 * children) and tolerates a single root node. Directories in IGNORED_DIRS
 * are pruned; `cap` bounds the result defensively on top of the backend's
 * own walk budget.
 */
export function flattenFileTree(root: FileNode[] | FileNode | null | undefined, cap = 600): string[] {
  const out: string[] = []
  if (!root) return out
  const stack = Array.isArray(root) ? [...root].reverse() : [root]
  while (stack.length > 0 && out.length < cap) {
    const node = stack.pop()!
    if (node.type === 'directory') {
      const base = node.name.split('/').pop() ?? node.name
      if (IGNORED_DIRS.has(base)) continue
      const children = node.children ?? []
      for (let i = children.length - 1; i >= 0; i--) stack.push(children[i])
    } else {
      out.push(node.path)
    }
  }
  return out
}

/**
 * AI-chat-ux C4 — the DIRECTORY paths of the same walk (parent dirs first),
 * feeding the `@folders` half of the mention menu. Same pruning/cap
 * contract as [`flattenFileTree`]; the walk itself stays files-first
 * (`flattenFileTree` runs its own traversal), so the two lists are
 * independent.
 */
export function flattenFileTreeDirs(root: FileNode[] | FileNode | null | undefined, cap = 200): string[] {
  const out: string[] = []
  if (!root) return out
  const stack = Array.isArray(root) ? [...root].reverse() : [root]
  while (stack.length > 0 && out.length < cap) {
    const node = stack.pop()!
    if (node.type !== 'directory') continue
    const base = node.name.split('/').pop() ?? node.name
    if (IGNORED_DIRS.has(base)) continue
    out.push(node.path)
    const children = node.children ?? []
    for (let i = children.length - 1; i >= 0; i--) stack.push(children[i])
  }
  return out
}

/** Contiguous-free fuzzy check: does `q` appear as an in-order subsequence
 *  of `s`? Returns the index of the FIRST character match of the earliest
 *  match, or −1. */
export function subsequenceStart(s: string, q: string): number {
  if (q.length === 0) return 0
  const first = q[0]
  for (let start = s.indexOf(first); start !== -1 && start <= s.length - q.length; start = s.indexOf(first, start + 1)) {
    let si = start
    let qi = 0
    while (si < s.length && qi < q.length) {
      if (s[si] === q[qi]) qi++
      si++
    }
    if (qi === q.length) return start
  }
  return -1
}

/**
 * Rank paths against the query: basename prefix > basename substring >
 * anywhere substring > subsequence; shorter names win ties, then
 * alphabetical. Empty query = everything (index order). Result is capped.
 *
 * AI-chat-ux C4 — `recency` optionally boosts recently-touched paths
 * (higher weight = more recent; the composer feeds it from the session
 * file index's registration time). Recency never outranks a basename
 * prefix — it only reorders within/across the weaker bands (a recency
 * bump of 50 cannot lift a subsequence hit over a basename substring).
 */
export function filterMentionCandidates(
  paths: string[],
  query: string,
  limit = 8,
  recency?: Map<string, number>,
): string[] {
  const q = query.toLowerCase()
  if (q === '') {
    if (!recency || recency.size === 0) return paths.slice(0, limit)
    return [...paths]
      .sort((a, b) => (recency.get(b) ?? 0) - (recency.get(a) ?? 0))
      .slice(0, limit)
  }
  const scored: { path: string; score: number }[] = []
  for (const path of paths) {
    const norm = path.replace(/\\/g, '/')
    const base = norm.split('/').pop() ?? norm
    const bl = base.toLowerCase()
    const nl = norm.toLowerCase()
    let score = -1
    if (bl.startsWith(q)) score = 400 - bl.length
    else if (bl.includes(q)) score = 300 - bl.length
    else if (nl.includes(q)) score = 200 - Math.min(nl.length, 120)
    else {
      const seq = subsequenceStart(bl, q)
      if (seq !== -1) score = 100 - seq
    }
    if (score >= 0) {
      // Recency tiebreaker: capped below the band gap (100) so it reorders
      // within a band instead of across bands.
      const boost = Math.min(50, recency?.get(path) ?? 0)
      scored.push({ path, score: score + boost })
    }
  }
  scored.sort((a, b) => b.score - a.score || a.path.localeCompare(b.path))
  return scored.slice(0, limit).map(s => s.path)
}
