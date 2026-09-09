// P2-4 — memory graph view, provenance badge visibility, view switching.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { MemoryGraphView } from '@/components/memory/MemoryGraphView'
import { filterMemoryGraph, layoutMemoryGraph, FORCE_ITERATIONS } from '@/components/memory/graphUtils'
import MemoryPanel from '@/components/memory/MemoryPanel'
import Memory from '@/pages/Memory'
import * as api from '@/lib/tauri-api'
import type { MemoryGraph, MemoryEntry } from '@/lib/tauri-api'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { SessionContext } from '@/context/SessionContext'

const SESSION_A = '0a1960aa-9c1d-4a5e-b8f0-31f2d1c4091a'

function makeGraph(): MemoryGraph {
  return {
    project: 'my-startup',
    entryCount: 3,
    maxEntries: 200,
    truncated: false,
    nodes: [
      { id: 'project:my-startup', kind: 'project', label: 'my-startup', weight: 3, category: null, sourceKind: null, sourceSessionId: null },
      { id: 'category:my-startup|preference', kind: 'category', label: 'preference', category: 'preference', weight: 1, sourceKind: null, sourceSessionId: null },
      { id: 'category:my-startup|decision', kind: 'category', label: 'decision', category: 'decision', weight: 2, sourceKind: null, sourceSessionId: null },
      { id: 'entry:m1', kind: 'entry', label: 'Prefers concise responses', category: 'preference', weight: 0.9, tags: ['style', 'tone'], sourceKind: 'auto-extract', sourceSessionId: SESSION_A },
      { id: 'entry:m2', kind: 'entry', label: 'Use Postgres for billing', category: 'decision', weight: 0.85, tags: ['db'], sourceKind: 'import', sourceSessionId: null },
      { id: 'entry:m3', kind: 'entry', label: 'Deep-work block 9–12', category: 'decision', weight: 0.78, sourceKind: 'auto-extract', sourceSessionId: SESSION_A },
    ],
    edges: [
      { source: 'project:my-startup', target: 'category:my-startup|preference', kind: 'cluster' },
      { source: 'project:my-startup', target: 'category:my-startup|decision', kind: 'cluster' },
      { source: 'category:my-startup|preference', target: 'entry:m1', kind: 'cluster' },
      { source: 'category:my-startup|decision', target: 'entry:m2', kind: 'cluster' },
      { source: 'category:my-startup|decision', target: 'entry:m3', kind: 'cluster' },
      { source: 'entry:m1', target: 'entry:m3', kind: 'session' },
    ],
  }
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('graphUtils (pure)', () => {
  it('produces deterministic positions inside the canvas', () => {
    const g = makeGraph()
    const l1 = layoutMemoryGraph(g, 900)
    const l2 = layoutMemoryGraph(g, 900)
    expect(l1.nodes.map((n) => [n.node.id, n.x, n.y])).toEqual(
      l2.nodes.map((n) => [n.node.id, n.x, n.y]),
    )
    for (const n of l1.nodes) {
      expect(n.x).toBeGreaterThanOrEqual(0)
      expect(n.x).toBeLessThanOrEqual(l1.width)
      expect(n.y).toBeGreaterThanOrEqual(0)
      expect(n.y).toBeLessThanOrEqual(l1.height)
    }
    expect(l1.nodes).toHaveLength(g.nodes.length)
    expect(l1.edges).toHaveLength(g.edges.length)
  })

  it('separates same-cluster entries that start overlapped (bounded iterations)', () => {
    const g = makeGraph()
    expect(FORCE_ITERATIONS).toBeLessThanOrEqual(60)
    const layout = layoutMemoryGraph(g, 900)
    // Every pair of entries keeps a minimum clearance after relaxation.
    const entries = layout.nodes.filter((n) => n.node.kind === 'entry')
    for (let i = 0; i < entries.length; i++) {
      for (let j = i + 1; j < entries.length; j++) {
        const d = Math.hypot(entries[i].x - entries[j].x, entries[i].y - entries[j].y)
        expect(d).toBeGreaterThan(6)
      }
    }
  })

  it('filter narrows entries and prunes orphan clusters and edges', () => {
    const g = makeGraph()
    const f = filterMemoryGraph(g, 'decision', '')
    const entryIds = f.nodes.filter((n) => n.kind === 'entry').map((n) => n.id)
    expect(entryIds).toEqual(['entry:m2', 'entry:m3'])
    // preference category node and its cluster edge are gone.
    expect(f.nodes.some((n) => n.id === 'category:my-startup|preference')).toBe(false)
    expect(f.edges.some((e) => e.target === 'category:my-startup|preference')).toBe(false)
    // session edge m1→m3 dropped (m1 filtered out).
    expect(f.edges.some((e) => e.kind === 'session')).toBe(false)
    // no-op when no filters active.
    expect(filterMemoryGraph(g, 'all', '  ')).toBe(g)
  })
})

describe('MemoryGraphView', () => {
  it('renders the node and edge counts of the payload', () => {
    render(<MemoryGraphView graph={makeGraph()} category="all" query="" />)
    const svg = screen.getByTestId('memory-graph-svg')
    expect(svg.getAttribute('aria-label')).toMatch(/3/)
    expect(svg.querySelectorAll('[data-node-kind="entry"]')).toHaveLength(3)
    expect(svg.querySelectorAll('[data-node-kind="category"]')).toHaveLength(2)
    expect(svg.querySelectorAll('[data-node-kind="project"]')).toHaveLength(1)
    expect(svg.querySelectorAll('[data-edge-kind="cluster"]')).toHaveLength(5)
    expect(svg.querySelectorAll('[data-edge-kind="session"]')).toHaveLength(1)
  })

  it('opens the detail popover on entry click with source badge and jump', () => {
    const onOpenMemorySource = vi.fn()
    render(
      <MemoryGraphView graph={makeGraph()} category="all" query="" onOpenMemorySource={onOpenMemorySource} />,
    )
    const svg = screen.getByTestId('memory-graph-svg')
    fireEvent.click(svg.querySelector('[data-node-id="entry:m1"]')!)
    const detail = screen.getByTestId('memory-graph-detail')
    expect(detail).toHaveTextContent('Prefers concise responses')
    expect(detail).toHaveTextContent('#style')
    expect(screen.getByTestId('memory-graph-source-badge')).toHaveTextContent(/auto-extract/i)
    fireEvent.click(screen.getByRole('button', { name: /open chat|跳转会话/i }))
    expect(onOpenMemorySource).toHaveBeenCalledWith('m1', SESSION_A)
  })

  it('hides the source badge and jump for entries without a source session', () => {
    render(<MemoryGraphView graph={makeGraph()} category="all" query="" />)
    const svg = screen.getByTestId('memory-graph-svg')
    fireEvent.click(svg.querySelector('[data-node-id="entry:m2"]')!)
    expect(screen.getByTestId('memory-graph-detail')).toHaveTextContent('Use Postgres for billing')
    expect(screen.queryByTestId('memory-graph-source-badge')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /open chat|跳转会话/i })).not.toBeInTheDocument()
  })

  it('shows the truncated-state hint when over the entry cap', () => {
    const g = { ...makeGraph(), entryCount: 260, truncated: true }
    render(<MemoryGraphView graph={g} category="all" query="" />)
    expect(screen.getByRole('status')).toHaveTextContent(/260/)
  })

  it('renders the empty state for a payload without entries', () => {
    render(
      <MemoryGraphView
        graph={{ project: null, nodes: [], edges: [], entryCount: 0, maxEntries: 200, truncated: false }}
        category="all"
        query=""
      />,
    )
    expect(screen.getByTestId('memory-graph-empty')).toBeInTheDocument()
  })

  it('applies the category filter client-side', () => {
    render(<MemoryGraphView graph={makeGraph()} category="decision" query="" />)
    const svg = screen.getByTestId('memory-graph-svg')
    expect(svg.querySelectorAll('[data-node-kind="entry"]')).toHaveLength(2)
    expect(svg.querySelectorAll('[data-edge-kind="session"]')).toHaveLength(0)
  })
})

// Shared fixtures for panel/page suites (module scope — both describes use them).
const entryWithSource: MemoryEntry = {
  id: 'm1',
  project: 'web-app',
  category: 'decision',
  content: 'Adopt React Query for server state',
  tags: ['react'],
  confidence: 0.9,
  created_at: '2026-06-02T00:00:00Z',
  accessed_at: '2026-06-02T00:00:00Z',
  access_count: 0,
  source_kind: 'auto-extract',
  source_session_id: SESSION_A,
}
const entryWithoutSource: MemoryEntry = { ...entryWithSource, id: 'm2', content: 'Plain note', source_kind: null, source_session_id: null }

function mockList(rows: MemoryEntry[]) {
  vi.mocked(api.getMemoryStats).mockResolvedValue({
    total: rows.length, by_category: {}, by_project: {}, most_recent_at: null,
  })
  vi.mocked(api.listMemories).mockResolvedValue(rows)
  vi.mocked(api.listMemoryProjects).mockResolvedValue(['web-app'])
}

describe('MemoryPanel view switching + provenance badge', () => {
  it('shows the list view by default and switches to the graph view', async () => {
    mockList([entryWithSource])
    vi.mocked(api.getMemoryGraph).mockResolvedValue(makeGraph())
    render(<MemoryPanel />)

    // default tab: list
    expect(screen.getByRole('tab', { name: /list|列表/i })).toHaveAttribute('aria-selected', 'true')
    expect(await screen.findByText('Adopt React Query for server state')).toBeInTheDocument()
    expect(screen.queryByTestId('memory-graph-svg')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('tab', { name: /graph|图谱/i }))
    await waitFor(() => {
      expect(api.getMemoryGraph).toHaveBeenCalled()
    })
    await waitFor(() => {
      expect(screen.getByTestId('memory-graph-svg')).toBeInTheDocument()
    })
  })

  it('shows the source badge only for entries carrying a source session', async () => {
    mockList([entryWithSource, entryWithoutSource])
    render(<MemoryPanel onOpenMemorySource={vi.fn()} />)
    const badges = await screen.findAllByTestId('memory-source-badge')
    expect(badges).toHaveLength(1)
    expect(badges[0]).toHaveTextContent(/auto-extract/i)
    // jump button rendered once — only next to the sourced entry
    expect(screen.getAllByRole('button', { name: /open chat|跳转会话/i })).toHaveLength(1)
  })

  it('hides the jump button when no source handler is provided (bare render)', async () => {
    mockList([entryWithSource])
    render(<MemoryPanel />)
    expect(await screen.findByTestId('memory-source-badge')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /open chat|跳转会话/i })).not.toBeInTheDocument()
  })
})

describe('Memory page provenance jump', () => {
  // MemoryRouter never touches window.location — probe useLocation instead.
  let currentPath = '/'
  function LocationProbe() {
    currentPath = useLocation().pathname
    return null
  }
  function renderPage(switchSession: (id: string) => Promise<void>) {
    currentPath = '/'
    return render(
      <MemoryRouter>
        <SessionContext.Provider
          value={{
            sessions: [], currentSessionId: null, windowSessionId: null,
            createSession: vi.fn(), createSessionInWorktree: vi.fn(), switchSession,
            deleteSession: vi.fn(), renameSession: vi.fn(), refreshSessions: vi.fn(),
          }}
        >
          <Memory />
          <LocationProbe />
        </SessionContext.Provider>
      </MemoryRouter>,
    )
  }

  it('resolves the source via get_memory_source then switches session and navigates to /chat', async () => {
    mockList([entryWithSource])
    vi.mocked(api.getMemorySource).mockResolvedValue({ sessionId: SESSION_A })
    const switchSession = vi.fn().mockResolvedValue(undefined)
    renderPage(switchSession)
    const jump = await screen.findAllByRole('button', { name: /open chat|跳转会话/i })
    fireEvent.click(jump[0])
    await waitFor(() => {
      expect(api.getMemorySource).toHaveBeenCalledWith(null, 'm1')
      expect(switchSession).toHaveBeenCalledWith(SESSION_A)
      expect(currentPath).toBe('/chat')
    })
  })

  it('stays put when the memory has no resolvable source session', async () => {
    mockList([entryWithSource])
    vi.mocked(api.getMemorySource).mockResolvedValue(null)
    const switchSession = vi.fn().mockResolvedValue(undefined)
    renderPage(switchSession)
    const jump = await screen.findAllByRole('button', { name: /open chat|跳转会话/i })
    fireEvent.click(jump[0])
    await waitFor(() => {
      expect(api.getMemorySource).toHaveBeenCalled()
    })
    expect(switchSession).not.toHaveBeenCalled()
    expect(currentPath).not.toBe('/chat')
  })
})
