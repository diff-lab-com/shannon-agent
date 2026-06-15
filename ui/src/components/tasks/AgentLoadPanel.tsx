// AgentLoadPanel — Phase D G10 deliverable.
//
// Real-time agent workload visualization. Derives a per-agent load score from
// AgentInfo: active agents contribute their `progress` (or partial credit if
// no progress), idle/blocked agents contribute 0. Renders a horizontal bar
// chart sorted by load, with summary stats (active count, avg load, peak).

import { useMemo } from 'react'
import type { AgentInfo } from '@/types'

interface AgentLoadPanelProps {
  agents: AgentInfo[]
}

interface AgentLoad {
  id: string
  name: string
  status: string
  load: number // 0..100
  active: boolean
  task?: string
}

function isActiveStatus(status: string): boolean {
  return status === 'active' || status === 'running' || status === 'in_progress'
}

function statusColor(status: string): string {
  if (status === 'failed' || status === 'error') return 'bg-error'
  if (status === 'blocked') return 'bg-warning'
  if (isActiveStatus(status)) return 'bg-primary'
  if (status === 'completed') return 'bg-tertiary'
  return 'bg-outline-variant'
}

function computeLoad(a: AgentInfo): number {
  if (!isActiveStatus(a.status)) return 0
  if (typeof a.progress === 'number' && a.progress > 0) return Math.min(100, a.progress)
  // Active but no progress info: assume in-flight (50%).
  return 50
}

function computeAgentLoads(agents: AgentInfo[]): AgentLoad[] {
  return agents
    .map(a => ({
      id: a.id,
      name: a.name,
      status: a.status,
      load: computeLoad(a),
      active: isActiveStatus(a.status),
      task: a.task,
    }))
    .sort((a, b) => {
      // Active first, then by load desc, then by name.
      if (a.active !== b.active) return a.active ? -1 : 1
      if (a.load !== b.load) return b.load - a.load
      return a.name.localeCompare(b.name)
    })
}

export default function AgentLoadPanel({ agents }: AgentLoadPanelProps) {
  const rows = useMemo(() => computeAgentLoads(agents), [agents])

  const stats = useMemo(() => {
    if (rows.length === 0) return { active: 0, idle: 0, avgLoad: 0, peakLoad: 0 }
    const active = rows.filter(r => r.active).length
    const totalLoad = rows.reduce((sum, r) => sum + r.load, 0)
    const peakLoad = rows.reduce((max, r) => Math.max(max, r.load), 0)
    return {
      active,
      idle: rows.length - active,
      avgLoad: Math.round(totalLoad / rows.length),
      peakLoad,
    }
  }, [rows])

  return (
    <div className="bg-surface-container-lowest rounded-2xl p-xl border border-outline-variant/30 shadow-sm">
      <div className="flex items-center justify-between mb-md">
        <div className="flex items-center gap-2">
          <span className="material-symbols-outlined text-[20px] text-on-surface">speed</span>
          <h3 className="font-headline-md text-[18px] font-bold text-on-surface">Agent Load</h3>
        </div>
        <span className="text-label-sm text-on-surface-variant bg-surface-container px-2 py-0.5 rounded-full border border-outline-variant/20">
          {stats.active} active · {stats.idle} idle
        </span>
      </div>

      {rows.length === 0 ? (
        <p className="text-body-sm text-on-surface-variant text-center py-lg">
          No agents registered. Start a workflow to see real-time load.
        </p>
      ) : (
        <>
          {/* Summary stats */}
          <div className="grid grid-cols-3 gap-sm mb-md">
            <div className="bg-surface-container-low rounded-xl p-sm border border-outline-variant/20 text-center">
              <div className="text-label-sm text-[10px] text-on-surface-variant uppercase tracking-wider">Active</div>
              <div className="font-headline-md text-[18px] font-bold text-primary">{stats.active}</div>
            </div>
            <div className="bg-surface-container-low rounded-xl p-sm border border-outline-variant/20 text-center">
              <div className="text-label-sm text-[10px] text-on-surface-variant uppercase tracking-wider">Avg Load</div>
              <div className="font-headline-md text-[18px] font-bold text-on-surface">{stats.avgLoad}%</div>
            </div>
            <div className="bg-surface-container-low rounded-xl p-sm border border-outline-variant/20 text-center">
              <div className="text-label-sm text-[10px] text-on-surface-variant uppercase tracking-wider">Peak</div>
              <div className="font-headline-md text-[18px] font-bold text-tertiary">{stats.peakLoad}%</div>
            </div>
          </div>

          {/* Per-agent bars */}
          <ol className="space-y-sm">
            {rows.map(r => (
              <li key={r.id}>
                <div className="flex items-center justify-between mb-1">
                  <div className="flex items-center gap-xs min-w-0">
                    <span className="w-2 h-2 rounded-full shrink-0" aria-hidden="true">
                      <span className={`block w-2 h-2 rounded-full ${statusColor(r.status)}`} />
                    </span>
                    <span className="font-label-md text-on-surface truncate">{r.name}</span>
                    {r.task ? (
                      <span className="font-label-sm text-on-surface-variant truncate">
                        · {r.task}
                      </span>
                    ) : null}
                  </div>
                  <span className={`font-label-sm font-bold ${r.active ? 'text-primary' : 'text-on-surface-variant'}`}>
                    {r.load}%
                  </span>
                </div>
                <div
                  className="w-full h-2 bg-outline-variant/20 rounded-full overflow-hidden"
                  role="progressbar"
                  aria-valuenow={r.load}
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-label={`${r.name} load`}
                >
                  <div
                    className={`h-full ${statusColor(r.status)} rounded-full transition-all duration-500 ${r.active ? 'animate-pulse' : ''}`}
                    style={{ width: `${r.load}%` }}
                  />
                </div>
              </li>
            ))}
          </ol>
        </>
      )}
    </div>
  )
}
