import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import AgentLoadPanel from '@/components/tasks/AgentLoadPanel'
import type { AgentInfo } from '@/types'

function mkAgent(over: Partial<AgentInfo> = {}): AgentInfo {
  return {
    id: 'a1',
    name: 'Lead',
    model: 'claude-sonnet-4-6',
    status: 'active',
    ...over,
  }
}

describe('AgentLoadPanel', () => {
  it('renders empty state when no agents', () => {
    render(<AgentLoadPanel agents={[]} />)
    expect(screen.getByText(/No agents registered/)).toBeInTheDocument()
  })

  it('renders summary stats with active and idle counts', () => {
    render(
      <AgentLoadPanel
        agents={[
          mkAgent({ id: 'a1', name: 'Lead', status: 'active', progress: 80 }),
          mkAgent({ id: 'a2', name: 'Worker', status: 'idle' }),
        ]}
      />,
    )
    expect(screen.getByText('1 active · 1 idle')).toBeInTheDocument()
    expect(screen.getByText('Lead')).toBeInTheDocument()
    expect(screen.getByText('Worker')).toBeInTheDocument()
  })

  it('renders progress as load percentage for active agent', () => {
    render(
      <AgentLoadPanel agents={[mkAgent({ id: 'a1', name: 'Lead', progress: 75 })]} />,
    )
    // Multiple elements (Peak stat, Avg stat, row label) all show 75%.
    expect(screen.getAllByText('75%').length).toBeGreaterThan(0)
  })

  it('defaults to 50% load for active agent without progress', () => {
    render(
      <AgentLoadPanel agents={[mkAgent({ id: 'a1', name: 'Lead', status: 'running', progress: undefined })]} />,
    )
    expect(screen.getAllByText('50%').length).toBeGreaterThan(0)
  })

  it('renders 0% for idle agent', () => {
    render(
      <AgentLoadPanel agents={[mkAgent({ id: 'a1', name: 'Lead', status: 'idle' })]} />,
    )
    expect(screen.getAllByText('0%').length).toBeGreaterThan(0)
  })

  it('sorts active agents above idle', () => {
    const { container } = render(
      <AgentLoadPanel
        agents={[
          mkAgent({ id: 'idle1', name: 'IdleAgent', status: 'idle' }),
          mkAgent({ id: 'active1', name: 'ActiveAgent', status: 'active', progress: 60 }),
        ]}
      />,
    )
    // First progressbar/row should be the active one
    const names = container.querySelectorAll('.font-label-md.text-on-surface.truncate')
    // (name span has these classes; verify the active one appears before idle)
    const textContents = Array.from(names).map(n => n.textContent)
    const activeIdx = textContents.indexOf('ActiveAgent')
    const idleIdx = textContents.indexOf('IdleAgent')
    expect(activeIdx).toBeGreaterThanOrEqual(0)
    expect(idleIdx).toBeGreaterThanOrEqual(0)
    expect(activeIdx).toBeLessThan(idleIdx)
  })

  it('computes average load stat', () => {
    render(
      <AgentLoadPanel
        agents={[
          mkAgent({ id: 'a1', name: 'A', status: 'active', progress: 100 }),
          mkAgent({ id: 'a2', name: 'B', status: 'idle' }),
        ]}
      />,
    )
    // (100 + 0) / 2 = 50%
    expect(screen.getByText('50%')).toBeInTheDocument()
  })
})
