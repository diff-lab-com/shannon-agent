// Tests for the Extensions → Pending page (IA X1): section layout, the
// errors section (W1-7: real MCP failures from `list_mcp_servers`, with the
// empty state as the true "nothing failed" fallback), and the one-shot
// candidate focus handed over by the Triage card's 「去审查」 jump (router
// state drained).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Routes, Route, useLocation } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import Pending from '@/components/extensions/Pending'
import type { SkillCandidate } from '@/lib/tauri-api'

vi.mock('@/hooks/usePendingSkillCandidates', () => ({
  usePendingSkillCandidates: () => hookSpy(),
}))

const hookSpy = vi.hoisted(() => vi.fn())
const listMcpServers = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  approveSkillCandidate: vi.fn(),
  rejectSkillCandidate: vi.fn(),
  listMcpServers: (...a: unknown[]) => listMcpServers(...a),
  skillLoop: {
    listProposals: vi.fn().mockResolvedValue([]),
    approve: vi.fn(),
    reject: vi.fn(),
  },
}))

const candidate: SkillCandidate = {
  id: 'cand-1',
  detected_at: '2026-09-20T10:00:00Z',
  occurrence_count: 2,
  example_session_ids: [],
  proposed_name: 'Deploy helper',
  proposed_trigger: 'when deploying',
  procedure: ['step'],
  source_tool_calls: [],
}

function LocationCapture() {
  const location = useLocation()
  return <div data-testid="location" data-state={JSON.stringify(location.state ?? null)}>{location.pathname}</div>
}

function renderPage(initialEntry: string | { pathname: string; state?: unknown } = '/extensions/pending') {
  return render(
    <I18nProvider>
      <MemoryRouter initialEntries={[initialEntry]}>
        <Routes>
          <Route path="*" element={<><Pending /><LocationCapture /></>} />
        </Routes>
      </MemoryRouter>
    </I18nProvider>,
  )
}

beforeEach(() => {
  hookSpy.mockReset()
  hookSpy.mockReturnValue({ candidates: [], loading: false, refetch: vi.fn() })
  listMcpServers.mockReset()
  // W1-7: default to a healthy fleet so the errors section shows its
  // true-empty fallback unless a test says otherwise.
  listMcpServers.mockResolvedValue([])
})

describe('Extensions → Pending page (IA X1)', () => {
  it('renders the skill-review section and the errors empty-state placeholder', async () => {
    renderPage()
    expect(screen.getByRole('heading', { name: 'Skill review' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Errors' })).toBeInTheDocument()
    // No subscribable MCP/install error source yet — empty state placeholder.
    expect(screen.getByText('No errors')).toBeInTheDocument()
    expect(await screen.findByText('Nothing waiting for review')).toBeInTheDocument()
  })

  it('shows the candidate queue content', async () => {
    hookSpy.mockReturnValue({ candidates: [candidate], loading: false, refetch: vi.fn() })
    renderPage()
    expect(await screen.findByText('Deploy helper')).toBeInTheDocument()
  })

  it('focuses the candidate handed over via router state and drains the state', async () => {
    hookSpy.mockReturnValue({
      candidates: [candidate, { ...candidate, id: 'cand-2', proposed_name: 'Other helper' }],
      loading: false,
      refetch: vi.fn(),
    })
    renderPage({ pathname: '/extensions/pending', state: { skillCandidateId: 'cand-2' } })

    await screen.findByText('Deploy helper')
    await waitFor(() => {
      const focused = document.querySelector('[data-focus-candidate="true"]')
      expect(focused).not.toBeNull()
      expect(focused!.textContent).toContain('Other helper')
    })
    // One-shot: the router state was consumed on mount.
    expect(JSON.parse(screen.getByTestId('location').getAttribute('data-state')!)).toBeNull()
  })

  // W1-7 (R2-P1-6): real MCP pool failures render as an error list.
  it('lists MCP servers that reported a connection error', async () => {
    listMcpServers.mockResolvedValue([
      {
        name: 'broken-fs',
        command: 'npx',
        enabled: true,
        connected: false,
        tool_count: 0,
        tools: [],
        last_connected: null,
        last_error: 'spawn /nonexistent ENOENT',
      },
      {
        name: 'healthy',
        command: 'npx',
        enabled: true,
        connected: true,
        tool_count: 2,
        tools: [],
        last_connected: 1_760_000_000_000,
      },
    ])
    renderPage()
    const list = await screen.findByTestId('mcp-error-list')
    expect(list).toBeInTheDocument()
    expect(screen.getByText('broken-fs')).toBeInTheDocument()
    expect(screen.getByText('spawn /nonexistent ENOENT')).toBeInTheDocument()
    // Healthy servers are not failures — they must not appear.
    expect(screen.queryByText('healthy')).not.toBeInTheDocument()
    expect(screen.queryByText('No errors')).not.toBeInTheDocument()
  })

  it('keeps the empty state when no MCP server failed', async () => {
    listMcpServers.mockResolvedValue([
      {
        name: 'healthy',
        command: 'npx',
        enabled: true,
        connected: true,
        tool_count: 1,
        tools: [],
        last_connected: 1_760_000_000_000,
      },
    ])
    renderPage()
    await waitFor(() => {
      expect(listMcpServers).toHaveBeenCalled()
    })
    expect(await screen.findByText('No errors')).toBeInTheDocument()
  })

  it('shows the load-failed message when the MCP list cannot be read', async () => {
    listMcpServers.mockRejectedValue(new Error('backend down'))
    renderPage()
    expect(await screen.findByText("Couldn't load MCP errors")).toBeInTheDocument()
    // A failed read must never render as "no errors".
    expect(screen.queryByText('No errors')).not.toBeInTheDocument()
  })
})
