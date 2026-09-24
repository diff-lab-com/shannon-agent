// Tests for the Extensions → Pending page (IA X1): section layout, the
// errors empty-state placeholder, and the one-shot candidate focus handed
// over by the Triage card's 「去审查」 jump (router state drained).

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

vi.mock('@/lib/tauri-api', () => ({
  approveSkillCandidate: vi.fn(),
  rejectSkillCandidate: vi.fn(),
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
})
