// Memory graph pure helpers (P2-4): category → semantic color token mapping,
// client-side filter over the backend graph payload, and a deterministic
// layered/clustered layout with a bounded force-relaxation pass (≤60 static
// iterations — no continuous animation; ≤200 entry nodes by contract, so the
// O(iterations · n²) same-cluster pass stays well under a millisecond budget
// per iteration on the entry subset).

import type { MemoryCategory, MemoryGraph, MemoryGraphNode } from '@/lib/tauri-api'

/** Cap on force-relaxation iterations (brief: ≤60, static). */
export const FORCE_ITERATIONS = 60

/** Entries per ring around a category anchor (keeps clusters compact). */
const ENTRIES_PER_RING = 10
/** Base radius of the first entry ring around a category node. */
const RING_BASE_RADIUS = 64
/** Radial gap between consecutive entry rings. */
const RING_GAP = 34

/** Semantic color tokens per memory category (generated theme vars). */
export const CATEGORY_GRAPH_COLOR: Record<MemoryCategory, string> = {
  preference: 'var(--color-primary-container)',
  pattern: 'var(--color-secondary-container)',
  decision: 'var(--color-tertiary-container)',
  error: 'var(--color-error)',
  context: 'var(--color-on-surface-variant)',
}

export function categoryColor(category: string | null | undefined): string {
  if (!category || !(category in CATEGORY_GRAPH_COLOR)) {
    return 'var(--color-on-surface-variant)'
  }
  return CATEGORY_GRAPH_COLOR[category as MemoryCategory]
}

/**
 * Client-side narrowing of the backend graph to the active category/query
 * filters: entry nodes matching are kept; category and project nodes survive
 * only while they still retain at least one kept entry; edges are pruned to
 * the surviving endpoints.
 */
export function filterMemoryGraph(
  graph: MemoryGraph,
  category: string,
  query: string,
): MemoryGraph {
  const q = query.trim().toLowerCase()
  if (category === 'all' && !q) return graph

  const matches = (n: MemoryGraphNode) => {
    if (category !== 'all' && n.category !== category) return false
    if (q && !n.label.toLowerCase().includes(q)) return false
    return true
  }

  const keptEntries = new Set(
    graph.nodes.filter((n) => n.kind === 'entry' && matches(n)).map((n) => n.id),
  )

  // A category survives while it retains ≥1 kept entry (category→entry edge).
  const keptCategories = new Set<string>()
  for (const e of graph.edges) {
    if (e.kind === 'cluster' && keptEntries.has(e.target)) keptCategories.add(e.source)
  }
  // A project root survives while it retains ≥1 kept category (root→category).
  const keptRoots = new Set<string>()
  for (const e of graph.edges) {
    if (e.kind === 'cluster' && keptCategories.has(e.target)) keptRoots.add(e.source)
  }

  const keptEdge = (e: MemoryGraph['edges'][number]) => {
    if (e.kind === 'session') {
      return keptEntries.has(e.source) && keptEntries.has(e.target)
    }
    // cluster: category→entry for kept entries, root→category for kept categories.
    return keptEntries.has(e.target) || keptCategories.has(e.target)
  }

  const nodes = graph.nodes.filter((n) => {
    if (n.kind === 'entry') return keptEntries.has(n.id)
    if (n.kind === 'category') return keptCategories.has(n.id)
    return keptRoots.has(n.id)
  })
  return { ...graph, nodes, edges: graph.edges.filter(keptEdge) }
}

export interface PositionedNode {
  node: MemoryGraphNode
  x: number
  y: number
  radius: number
}

export interface PositionedEdge {
  source: string
  target: string
  kind: string
  x1: number
  y1: number
  x2: number
  y2: number
}

export interface GraphLayout {
  nodes: PositionedNode[]
  edges: PositionedEdge[]
  width: number
  height: number
}

function nodeRadius(n: MemoryGraphNode): number {
  if (n.kind === 'project') return 22
  if (n.kind === 'category') return 16 + Math.min(n.weight, 24) * 0.5
  return 7 + Math.max(0, Math.min(n.weight, 1)) * 7
}

/**
 * Deterministic clustered layout:
 *   row 0 — project root(s), centered over their category columns
 *   row 1 — one cluster per category (anchor node centered on its entries)
 *   rings — entries arranged around their category anchor (deterministic
 *           angles), then ≤FORCE_ITERATIONS static relaxation passes that
 *           push same-cluster entries out of each other's way.
 * No randomness — same input always yields the same positions.
 */
export function layoutMemoryGraph(graph: MemoryGraph, width: number): GraphLayout {
  const W = Math.max(width, 480)
  const projects = graph.nodes.filter((n) => n.kind === 'project')
  const categories = graph.nodes.filter((n) => n.kind === 'category')
  const entries = graph.nodes.filter((n) => n.kind === 'entry')

  const positions = new Map<string, { x: number; y: number }>()
  const radii = new Map<string, number>()
  for (const n of graph.nodes) radii.set(n.id, nodeRadius(n))

  // Column per category keeps clusters separated; roots center over theirs.
  const columnWidth = categories.length > 0 ? W / categories.length : W
  categories.forEach((c, i) => {
    positions.set(c.id, { x: columnWidth * (i + 0.5), y: 170 })
  })
  projects.forEach((p) => {
    const children = graph.edges.filter(
      (e) => e.kind === 'cluster' && e.source === p.id,
    )
    const xs =
      children.length > 0
        ? children.map((e) => positions.get(e.target)?.x ?? W / 2)
        : [W / 2]
    const cx = xs.reduce((a, b) => a + b, 0) / xs.length
    positions.set(p.id, { x: cx, y: 48 })
  })

  // Entries in rings around their category anchor.
  const entriesByCategory = new Map<string, MemoryGraphNode[]>()
  for (const e of entries) {
    const owner = graph.edges.find(
      (ed) => ed.kind === 'cluster' && ed.target === e.id,
    )?.source
    if (!owner) continue
    const list = entriesByCategory.get(owner) ?? []
    list.push(e)
    entriesByCategory.set(owner, list)
  }
  for (const [catId, list] of entriesByCategory) {
    const anchor = positions.get(catId) ?? { x: W / 2, y: 170 }
    list.forEach((e, i) => {
      const ring = Math.floor(i / ENTRIES_PER_RING)
      const inRing = i % ENTRIES_PER_RING
      const count = Math.min(list.length - ring * ENTRIES_PER_RING, ENTRIES_PER_RING)
      const radius = RING_BASE_RADIUS + ring * RING_GAP
      const angle =
        (2 * Math.PI * inRing) / count + (ring % 2 === 1 ? Math.PI / count : 0)
      positions.set(e.id, {
        x: anchor.x + radius * Math.cos(angle),
        y: anchor.y + radius * Math.sin(angle),
      })
    })
  }

  // Static force relaxation — same-cluster entry repulsion only, bounded.
  const relax = (nodes: MemoryGraphNode[], iterations: number) => {
    for (let it = 0; it < iterations; it++) {
      for (let i = 0; i < nodes.length; i++) {
        for (let j = i + 1; j < nodes.length; j++) {
          const a = positions.get(nodes[i].id)
          const b = positions.get(nodes[j].id)
          if (!a || !b) continue
          const dx = b.x - a.x
          const dy = b.y - a.y
          const dist = Math.hypot(dx, dy) || 0.01
          const minDist =
            (radii.get(nodes[i].id) ?? 8) + (radii.get(nodes[j].id) ?? 8) + 6
          if (dist >= minDist) continue
          const push = (minDist - dist) / 2
          const ux = dx / dist
          const uy = dy / dist
          a.x -= ux * push
          a.y -= uy * push
          b.x += ux * push
          b.y += uy * push
        }
      }
    }
  }
  for (const list of entriesByCategory.values()) {
    relax(list, FORCE_ITERATIONS)
  }

  // Clamp inside the canvas.
  const height = computeHeight(graph, positions, W)
  const nodes: PositionedNode[] = graph.nodes.map((n) => {
    const p = positions.get(n.id) ?? { x: W / 2, y: 48 }
    const r = radii.get(n.id) ?? 8
    return {
      node: n,
      x: Math.min(Math.max(p.x, r + 4), W - r - 4),
      y: Math.min(Math.max(p.y, r + 4), height - r - 4),
      radius: r,
    }
  })

  const byId = new Map(nodes.map((n) => [n.node.id, n]))
  const edges: PositionedEdge[] = graph.edges.flatMap((e) => {
    const s = byId.get(e.source)
    const t = byId.get(e.target)
    if (!s || !t) return []
    return [
      {
        source: e.source,
        target: e.target,
        kind: e.kind,
        x1: s.x,
        y1: s.y,
        x2: t.x,
        y2: t.y,
      },
    ]
  })

  return { nodes, edges, width: W, height }
}

function computeHeight(
  graph: MemoryGraph,
  positions: Map<string, { x: number; y: number }>,
  W: number,
): number {
  let max = 240
  for (const n of graph.nodes) {
    if (n.kind !== 'entry') continue
    const p = positions.get(n.id)
    if (p) max = Math.max(max, p.y + 40)
  }
  // No entries at all (roots/categories only) still gets a sane canvas.
  return graph.nodes.length === 0 ? 160 : Math.max(max, W * 0.45)
}
