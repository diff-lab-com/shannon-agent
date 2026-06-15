// TaskDAGView — Phase D G6 deliverable.
//
// Renders tasks as a directed acyclic graph using SVG edges and HTML nodes.
// Edge: A → B means "B blocked_by A" (A must finish before B starts).
//
// Layout: topological depth (longest path from any root) determines the x-axis
// column; ties within a column are stacked vertically. This produces a
// left-to-right pipeline view. Cycles (which shouldn't exist in real data,
// but the parser is lenient) are broken by falling back to insertion order.
//
// No external graph library — pure SVG with click-through to the standard
// task drawer.

import { useMemo } from 'react'
import type { TaskItem } from '@/types'

interface TaskDAGViewProps {
  tasks: TaskItem[]
  onSelectTask?: (id: string) => void
}

interface PositionedNode {
  task: TaskItem
  col: number
  row: number
  x: number
  y: number
}

const NODE_W = 180
const NODE_H = 64
const COL_GAP = 80
const ROW_GAP = 16
const PAD = 24

function topologicalColumns(tasks: TaskItem[]): Map<string, number> {
  // Map id → task for lookup.
  const byId = new Map(tasks.map(t => [t.id, t]))
  const memo = new Map<string, number>()
  const visiting = new Set<string>()

  const depthOf = (id: string): number => {
    if (memo.has(id)) return memo.get(id)!
    // Cycle guard — treat revisited nodes as depth 0 to break loops.
    if (visiting.has(id)) return 0
    visiting.add(id)
    const task = byId.get(id)
    if (!task) {
      visiting.delete(id)
      memo.set(id, 0)
      return 0
    }
    const deps = (task.blocked_by ?? []).filter(d => byId.has(d))
    const depth = deps.length === 0 ? 0 : Math.max(...deps.map(depthOf)) + 1
    visiting.delete(id)
    memo.set(id, depth)
    return depth
  }

  for (const t of tasks) depthOf(t.id)
  return memo
}

function layout(tasks: TaskItem[]): { nodes: PositionedNode[]; width: number; height: number } {
  const cols = topologicalColumns(tasks)
  // Group by column.
  const byCol = new Map<number, TaskItem[]>()
  for (const t of tasks) {
    const c = cols.get(t.id) ?? 0
    if (!byCol.has(c)) byCol.set(c, [])
    byCol.get(c)!.push(t)
  }
  // Sort within column by title for deterministic order.
  for (const arr of byCol.values()) {
    arr.sort((a, b) => a.title.localeCompare(b.title))
  }

  const nodes: PositionedNode[] = []
  let maxCol = 0
  let maxRow = 0
  for (const [col, arr] of byCol.entries()) {
    if (col > maxCol) maxCol = col
    arr.forEach((task, row) => {
      if (row > maxRow) maxRow = row
      nodes.push({
        task,
        col,
        row,
        x: PAD + col * (NODE_W + COL_GAP),
        y: PAD + row * (NODE_H + ROW_GAP),
      })
    })
  }

  const width = PAD * 2 + (maxCol + 1) * NODE_W + maxCol * COL_GAP
  const height = PAD * 2 + (maxRow + 1) * NODE_H + maxRow * ROW_GAP
  return { nodes, width, height }
}

function statusFill(status: string): string {
  if (status === 'completed') return '#10b981' // tertiary
  if (status === 'running' || status === 'in_progress') return '#6366f1' // primary
  if (status === 'failed' || status === 'error') return '#ef4444' // error
  if (status === 'pending') return '#94a3b8' // outline
  return '#cbd5e1'
}

export default function TaskDAGView({ tasks, onSelectTask }: TaskDAGViewProps) {
  const { nodes, width, height } = useMemo(() => layout(tasks), [tasks])
  const nodeById = useMemo(() => new Map(nodes.map(n => [n.task.id, n])), [nodes])

  // Build edges: for each task, draw edge from each blocked_by → this.
  const edges = useMemo(() => {
    const list: { from: PositionedNode; to: PositionedNode }[] = []
    for (const n of nodes) {
      for (const dep of n.task.blocked_by ?? []) {
        const from = nodeById.get(dep)
        if (from) list.push({ from, to: n })
      }
    }
    return list
  }, [nodes, nodeById])

  if (tasks.length === 0) {
    return (
      <div className="bg-surface-container-lowest rounded-2xl p-xl border border-outline-variant/30 shadow-sm">
        <div className="flex items-center gap-2 mb-md">
          <span className="material-symbols-outlined text-[20px] text-on-surface">account_tree</span>
          <h3 className="font-headline-md text-[18px] font-bold text-on-surface">Task Graph</h3>
        </div>
        <p className="text-body-sm text-on-surface-variant text-center py-lg">
          No tasks yet — create tasks with dependencies to see the DAG.
        </p>
      </div>
    )
  }

  return (
    <div className="bg-surface-container-lowest rounded-2xl p-xl border border-outline-variant/30 shadow-sm">
      <div className="flex items-center justify-between mb-md">
        <div className="flex items-center gap-2">
          <span className="material-symbols-outlined text-[20px] text-on-surface">account_tree</span>
          <h3 className="font-headline-md text-[18px] font-bold text-on-surface">Task Graph</h3>
        </div>
        <span className="text-label-sm text-on-surface-variant">
          {tasks.length} tasks · {edges.length} edges
        </span>
      </div>

      <div className="overflow-x-auto">
        <svg
          width={width}
          height={height}
          role="img"
          aria-label="Task dependency graph"
          className="block"
        >
          {/* Arrowhead marker */}
          <defs>
            <marker
              id="dag-arrow"
              viewBox="0 0 10 10"
              refX="9"
              refY="5"
              markerWidth="6"
              markerHeight="6"
              orient="auto-start-reverse"
            >
              <path d="M 0 0 L 10 5 L 0 10 z" fill="#94a3b8" />
            </marker>
          </defs>

          {/* Edges */}
          {edges.map((e, i) => {
            const x1 = e.from.x + NODE_W
            const y1 = e.from.y + NODE_H / 2
            const x2 = e.to.x
            const y2 = e.to.y + NODE_H / 2
            const midX = (x1 + x2) / 2
            // Cubic bezier with horizontal tangents.
            const d = `M ${x1} ${y1} C ${midX} ${y1}, ${midX} ${y2}, ${x2} ${y2}`
            return (
              <path
                key={`edge-${i}`}
                d={d}
                fill="none"
                stroke="#94a3b8"
                strokeWidth={1.5}
                markerEnd="url(#dag-arrow)"
                opacity={0.7}
              />
            )
          })}

          {/* Nodes (foreignObject lets us use HTML/CSS for the card) */}
          {nodes.map(n => {
            const fill = statusFill(n.task.status)
            return (
              <foreignObject
                key={n.task.id}
                x={n.x}
                y={n.y}
                width={NODE_W}
                height={NODE_H}
              >
                <button
                  type="button"
                  onClick={() => onSelectTask?.(n.task.id)}
                  className="w-full h-full text-left bg-surface-container-lowest rounded-lg border-l-4 px-md py-xs shadow-sm hover:shadow-md hover:-translate-y-0.5 transition-all cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
                  style={{ borderLeftColor: fill }}
                  title={n.task.title}
                >
                  <div className="font-label-md text-[13px] font-bold text-on-surface truncate">
                    {n.task.title}
                  </div>
                  <div className="flex items-center gap-xs mt-0.5">
                    <span
                      className="text-[10px] font-bold uppercase tracking-wider px-1.5 py-0.5 rounded text-white"
                      style={{ backgroundColor: fill }}
                    >
                      {n.task.status}
                    </span>
                    {n.task.assignee ? (
                      <span className="text-[11px] text-on-surface-variant truncate">
                        @ {n.task.assignee}
                      </span>
                    ) : null}
                  </div>
                </button>
              </foreignObject>
            )
          })}
        </svg>
      </div>

      {/* Legend */}
      <div className="flex items-center gap-md mt-md pt-md border-t border-outline-variant/20 text-label-sm text-on-surface-variant">
        <span className="flex items-center gap-xs">
          <span className="w-2 h-2 rounded-full" style={{ backgroundColor: statusFill('completed') }} />
          Completed
        </span>
        <span className="flex items-center gap-xs">
          <span className="w-2 h-2 rounded-full" style={{ backgroundColor: statusFill('in_progress') }} />
          Running
        </span>
        <span className="flex items-center gap-xs">
          <span className="w-2 h-2 rounded-full" style={{ backgroundColor: statusFill('pending') }} />
          Pending
        </span>
        <span className="ml-auto text-[11px]">Click any task to view details</span>
      </div>
    </div>
  )
}
