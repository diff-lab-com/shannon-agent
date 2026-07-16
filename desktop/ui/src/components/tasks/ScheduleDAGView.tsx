// ScheduleDAGView — Phase D P3.5 deliverable.
//
// Renders scheduled routines as a DAG using SVG edges and HTML nodes, mirroring
// the TaskDAGView pattern. Edge: A → B means "B depends_on A" — A must complete
// before B is allowed to fire. Useful for sequencing chained routines
// (e.g. build → test → notify).
//
// Layout: topological depth from depends_on field. Cycles are broken by
// falling back to depth 0.
//
// Node fill reflects trigger_type (cron=primary, interval=tertiary,
// webhook=secondary, event=error). Strikethrough opacity for disabled routines.

import { useMemo } from 'react'
import { useIntl } from 'react-intl'
import type { ScheduledRoutine } from '@/types'

interface ScheduleDAGViewProps {
  routines: ScheduledRoutine[]
  onSelectRoutine?: (id: string) => void
}

interface PositionedNode {
  routine: ScheduledRoutine
  col: number
  row: number
  x: number
  y: number
}

const NODE_W = 180
const NODE_H = 70
const COL_GAP = 80
const ROW_GAP = 16
const PAD = 24

function topologicalColumns(routines: ScheduledRoutine[]): Map<string, number> {
  const byId = new Map(routines.map(r => [r.id, r]))
  const memo = new Map<string, number>()
  const visiting = new Set<string>()

  const depthOf = (id: string): number => {
    if (memo.has(id)) return memo.get(id)!
    if (visiting.has(id)) return 0
    visiting.add(id)
    const r = byId.get(id)
    if (!r) {
      visiting.delete(id)
      memo.set(id, 0)
      return 0
    }
    const deps = (r.depends_on ?? []).filter(d => byId.has(d))
    const depth = deps.length === 0 ? 0 : Math.max(...deps.map(depthOf)) + 1
    visiting.delete(id)
    memo.set(id, depth)
    return depth
  }
  for (const r of routines) depthOf(r.id)
  return memo
}

function layout(routines: ScheduledRoutine[]): { nodes: PositionedNode[]; width: number; height: number } {
  const cols = topologicalColumns(routines)
  const byCol = new Map<number, ScheduledRoutine[]>()
  for (const r of routines) {
    const c = cols.get(r.id) ?? 0
    if (!byCol.has(c)) byCol.set(c, [])
    byCol.get(c)!.push(r)
  }
  for (const arr of byCol.values()) {
    arr.sort((a, b) => a.name.localeCompare(b.name))
  }
  const nodes: PositionedNode[] = []
  let maxCol = 0
  let maxRow = 0
  for (const [col, arr] of byCol.entries()) {
    if (col > maxCol) maxCol = col
    arr.forEach((routine, row) => {
      if (row > maxRow) maxRow = row
      nodes.push({
        routine,
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

function triggerStyle(triggerType: string, enabled: boolean): { fill: string; stroke: string; icon: string } {
  if (!enabled) return { fill: '#f1f5f9', stroke: '#cbd5e1', icon: 'block' }
  if (triggerType === 'cron') return { fill: '#eef2ff', stroke: '#6366f1', icon: 'schedule' }
  if (triggerType === 'interval') return { fill: '#ecfdf5', stroke: '#10b981', icon: 'timer' }
  if (triggerType === 'webhook') return { fill: '#fef3c7', stroke: '#d97706', icon: 'webhook' }
  if (triggerType === 'event') return { fill: '#fce7f3', stroke: '#db2777', icon: 'bolt' }
  return { fill: '#f1f5f9', stroke: '#cbd5e1', icon: 'circle' }
}

export default function ScheduleDAGView({ routines, onSelectRoutine }: ScheduleDAGViewProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { nodes, width, height } = useMemo(() => layout(routines), [routines])
  const nodeById = useMemo(() => new Map(nodes.map(n => [n.routine.id, n])), [nodes])

  const edges = useMemo(() => {
    const list: { from: PositionedNode; to: PositionedNode }[] = []
    for (const n of nodes) {
      for (const dep of n.routine.depends_on ?? []) {
        const from = nodeById.get(dep)
        if (from) list.push({ from, to: n })
      }
    }
    return list
  }, [nodes, nodeById])

  if (routines.length === 0) {
    return (
      <div className="bg-surface-container-lowest rounded-2xl p-xl border border-outline-variant/30 shadow-sm">
        <div className="flex items-center gap-2 mb-md">
          <span className="material-symbols-outlined icon-md text-on-surface">account_tree</span>
          <h3 className="font-headline-md text-[18px] font-bold text-on-surface">{t('tasks.scheduleDAGView.title')}</h3>
        </div>
        <p className="text-body-sm text-on-surface-variant text-center py-lg">
          {t('tasks.scheduleDAGView.empty')}
        </p>
      </div>
    )
  }

  return (
    <div className="bg-surface-container-lowest rounded-2xl p-lg border border-outline-variant/30 shadow-sm">
      <div className="flex items-center gap-2 mb-md">
        <span className="material-symbols-outlined icon-md text-on-surface">account_tree</span>
        <h3 className="font-headline-md text-[18px] font-bold text-on-surface">{t('tasks.scheduleDAGView.title')}</h3>
        <span className="font-label-sm text-[11px] text-on-surface-variant bg-surface-container-low px-xs py-1 rounded-full">
          {intl.formatMessage({ id: 'tasks.scheduleDAGView.routineCount' }, { count: routines.length })}
        </span>
      </div>

      <div className="overflow-auto">
        <svg width={width} height={height} role="img" aria-label={intl.formatMessage({ id: 'tasks.scheduleDAGView.ariaLabel' })}>
          <defs>
            <marker id="sched-arrow" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse">
              <path d="M 0 0 L 10 5 L 0 10 z" fill="#94a3b8" />
            </marker>
          </defs>

          {edges.map((e, i) => {
            const x1 = e.from.x + NODE_W
            const y1 = e.from.y + NODE_H / 2
            const x2 = e.to.x
            const y2 = e.to.y + NODE_H / 2
            const mx = (x1 + x2) / 2
            return (
              <path
                key={`edge-${i}`}
                d={`M ${x1} ${y1} C ${mx} ${y1}, ${mx} ${y2}, ${x2} ${y2}`}
                stroke="#94a3b8"
                strokeWidth={1.5}
                fill="none"
                markerEnd="url(#sched-arrow)"
              />
            )
          })}

          {nodes.map(n => {
            const style = triggerStyle(n.routine.trigger_type, n.routine.enabled)
            return (
              <g
                key={n.routine.id}
                transform={`translate(${n.x}, ${n.y})`}
                onClick={() => onSelectRoutine?.(n.routine.id)}
                className={onSelectRoutine ? 'cursor-pointer' : ''}
                role="button"
                aria-label={`Routine ${n.routine.name}`}
              >
                <rect
                  width={NODE_W}
                  height={NODE_H}
                  rx={8}
                  fill={style.fill}
                  stroke={style.stroke}
                  strokeWidth={1.5}
                />
                <text x={12} y={20} fontSize={12} fontWeight={600} fill="#0f172a">
                  {n.routine.name.length > 22 ? n.routine.name.slice(0, 20) + '…' : n.routine.name}
                </text>
                <text x={12} y={38} fontSize={10} fill="#475569" fontFamily="ui-monospace, monospace">
                  {n.routine.trigger_type === 'cron'
                    ? n.routine.cron_expr ?? '0 0 * * *'
                    : n.routine.trigger_type === 'interval'
                      ? `every ${Math.round(n.routine.interval_secs / 60)}m`
                      : n.routine.trigger_type}
                </text>
                <text x={12} y={56} fontSize={9} fill={n.routine.enabled ? '#10b981' : '#94a3b8'}>
                  {n.routine.enabled ? '● active' : '○ disabled'}
                  {n.routine.fire_count > 0 ? ` · ${n.routine.fire_count}× fired` : ''}
                </text>
              </g>
            )
          })}
        </svg>
      </div>
    </div>
  )
}
