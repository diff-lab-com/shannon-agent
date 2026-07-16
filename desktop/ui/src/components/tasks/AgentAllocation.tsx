// Agent workload breakdown — shows top agents by tool usage share.
//
// Load is computed from AgentInfo.tools_used (real workload proxy).
// Agents with no recorded tool usage fall back to an equal share of
// the remaining budget so the bar isn't empty on fresh sessions.
//
// MD3 tokens only. Renders nothing when no agents are present.

import { useIntl } from 'react-intl'
import type { AgentInfo } from '@/types'

interface AgentAllocationProps {
  agents: AgentInfo[]
}

interface AgentAlloc {
  name: string
  pct: number
  color: string
  textColor: string
}

function computeAllocs(agents: AgentInfo[]): AgentAlloc[] {
  if (agents.length === 0) return []
  const top = agents.slice(0, 3)
  const total = top.reduce((sum, a) => sum + (a.tools_used ?? 0), 0)
  const colors = ['bg-primary', 'bg-secondary', 'bg-tertiary']
  const textColors = ['text-primary', 'text-secondary', 'text-tertiary']
  if (total === 0) {
    const evenShare = Math.round(100 / top.length)
    return top.map((a, i) => ({
      name: a.name,
      pct: evenShare,
      color: colors[i] ?? 'bg-primary',
      textColor: textColors[i] ?? 'text-primary',
    }))
  }
  return top.map((a, i) => ({
    name: a.name,
    pct: Math.round(((a.tools_used ?? 0) / total) * 100),
    color: colors[i] ?? 'bg-primary',
    textColor: textColors[i] ?? 'text-primary',
  }))
}

export default function AgentAllocation({ agents }: AgentAllocationProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const allocs = computeAllocs(agents)
  if (allocs.length === 0) return null
  return (
    <div className="bg-surface-container-low rounded-2xl p-lg border border-outline-variant/20">
      <h4 className="font-headline-md text-[16px] text-on-surface mb-md">{t('tasks.agentAllocation.title')}</h4>
      <div className="space-y-sm">
        {allocs.map(a => (
          <div key={a.name}>
            <div className="flex items-center justify-between mb-1">
              <span className="text-body-sm text-on-surface-variant">{a.name}</span>
              <span className={`font-label-md ${a.textColor}`}>{a.pct}%</span>
            </div>
            <div className="w-full h-1 bg-outline-variant/30 rounded-full">
              <div className={`h-full ${a.color} rounded-full`} style={{ width: `${a.pct}%` }} />
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
