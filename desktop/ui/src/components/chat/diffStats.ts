// Batch C2 (2026-09-20 delta analysis): "+153 −11" line counts for the
// in-chat diff summary card. Client-side computation over the same
// getFileDiff payload the diff review body uses — one cached IPC per path.
import * as api from '@/lib/tauri-api'
import { computeHunks } from '@/lib/diff-merge'

export interface DiffLineStats {
  additions: number
  deletions: number
}

// Module-level cache: the same file path yields the same stats for the life
// of the window (the review body re-fetches live state on open anyway).
const cache = new Map<string, DiffLineStats>()

export async function fetchDiffLineStats(path: string): Promise<DiffLineStats | null> {
  const hit = cache.get(path)
  if (hit) return hit
  try {
    const diff = await api.getFileDiff(path)
    const hunks = computeHunks(diff.old_content, diff.new_content)
    let additions = 0
    let deletions = 0
    for (const hunk of hunks) {
      for (const line of hunk.lines) {
        if (line.type === 'added') additions++
        else if (line.type === 'removed') deletions++
      }
    }
    const stats = { additions, deletions }
    cache.set(path, stats)
    return stats
  } catch {
    // Diff unavailable (demo mode, deleted file, binary) — the card simply
    // renders without counts.
    return null
  }
}

/** Sum stats for up to `cap` paths; null when nothing counted. */
export async function summarizeDiffLineStats(paths: string[], cap = 6): Promise<DiffLineStats | null> {
  let additions = 0
  let deletions = 0
  let counted = false
  for (const path of paths.slice(0, cap)) {
    const s = await fetchDiffLineStats(path)
    if (s) {
      additions += s.additions
      deletions += s.deletions
      counted = true
    }
  }
  return counted ? { additions, deletions } : null
}

/** B4 P2-20: called on session switch — the "+x −y" snapshot belongs to the
 *  session that produced it, not the life of the window. */
export function clearDiffStatsCache(): void {
  cache.clear()
}
