// GB P2-3 — the 运行 tab's render contract: the four elements off a
// RunProcessState snapshot, plan reuse via the Plan tab's hook, and the
// running/done/failed status surface.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import RunPanel from '@/pages/chat/RunPanel'
import type { RunProcessState } from '@/lib/runProcess'

// RunPanel pulls the plan doc through useSessionPlan → tauri-api.
vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    getSessionPlan: vi.fn().mockResolvedValue(null),
  }
})

// The hook registers Tauri event listeners on mount — unavailable in jsdom.
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockRejectedValue(new Error('no tauri')),
}))

function makeRun(overrides: Partial<RunProcessState> = {}): RunProcessState {
  return {
    status: 'running',
    startedAt: 1,
    endedAt: null,
    sources: ['/w/src/main.rs', '@docs/guide.md'],
    outputs: ['/w/out/report.md'],
    summary: 'compiling 42%…',
    lastTool: 'run_command',
    toolCount: 3,
    ...overrides,
  }
}

function renderPanel(run: RunProcessState = makeRun()) {
  return render(<RunPanel run={run} workingDir="/w" onOpenPlan={vi.fn()} />)
}

describe('RunPanel (GB P2-3 运行 tab)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders the four elements: summary, plan, sources, outputs', () => {
    renderPanel()
    expect(screen.getByTestId('run-summary')).toHaveTextContent('compiling 42%…')
    expect(screen.getByText('Sources referenced this turn')).toBeInTheDocument()
    expect(screen.getByText('Files produced this turn')).toBeInTheDocument()
    expect(screen.getByText('Plan')).toBeInTheDocument()
    const lists = screen.getAllByTestId('run-path-list')
    expect(lists[0]).toHaveTextContent('/w/src/main.rs')
    expect(lists[0]).toHaveTextContent('@docs/guide.md')
    expect(lists[1]).toHaveTextContent('/w/out/report.md')
    expect(screen.getByText('3 tool calls')).toBeInTheDocument()
    expect(screen.getByText('Running')).toBeInTheDocument()
  })

  it('no plan doc → muted placeholder, no "Open plan" button', () => {
    renderPanel()
    expect(screen.getByText('No plan document this turn')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Open plan' })).not.toBeInTheDocument()
  })

  it('done and failed snapshots keep the content and flip the badge', () => {
    const { rerender } = renderPanel(makeRun({ status: 'done', endedAt: 9 }))
    expect(screen.getByText('Finished')).toBeInTheDocument()
    rerender(<RunPanel run={makeRun({ status: 'failed', endedAt: 9 })} workingDir="/w" onOpenPlan={vi.fn()} />)
    expect(screen.getByText('Failed')).toBeInTheDocument()
    // Content survives the settle (「结束保留至下一轮开始」).
    expect(screen.getAllByTestId('run-path-list').length).toBe(2)
  })

  it('empty sources/outputs render their muted placeholders', () => {
    renderPanel(makeRun({ sources: [], outputs: [], summary: null }))
    expect(screen.getByText('No files or references used yet')).toBeInTheDocument()
    expect(screen.getByText('No files written yet')).toBeInTheDocument()
  })
})
