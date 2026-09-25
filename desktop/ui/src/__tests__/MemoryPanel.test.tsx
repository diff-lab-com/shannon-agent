import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent, within } from '@testing-library/react'
import MemoryPanel from '@/components/memory/MemoryPanel'
import * as api from '@/lib/tauri-api'

const dreamProposal = {
  id: 'proposal-1',
  project: 'web-app',
  created_at: '2026-09-24T02:00:00Z',
  actions: [
    {
      id: 'a1',
      kind: 'merge',
      entry_ids: ['m1', 'm2'],
      add_entry: null,
      rationale: 'Two entries say the same thing about deploys',
    },
    {
      id: 'a2',
      kind: 'remove',
      entry_ids: ['m3'],
      add_entry: null,
      rationale: 'Outdated note about the staging URL',
    },
    {
      id: 'a3',
      kind: 'add',
      entry_ids: [],
      add_entry: {
        category: 'preference',
        content: 'Prefer pnpm over npm',
        confidence: 0.8,
        source_session_ids: ['s1'],
        verified: true,
      },
      rationale: 'Repeatedly expressed package-manager preference',
    },
  ],
}

describe('MemoryPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders empty state when no memories', async () => {
    vi.mocked(api.getMemoryStats).mockResolvedValue({
      total: 0, by_category: {}, by_project: {}, most_recent_at: null,
    })
    vi.mocked(api.listMemories).mockResolvedValue([])
    vi.mocked(api.listMemoryProjects).mockResolvedValue([])

    render(<MemoryPanel />)
    await waitFor(() => {
      expect(screen.queryByText(/Loading/)).not.toBeInTheDocument()
    })

    // "Create your first memory" button only appears in empty state
    expect(screen.getByRole('button', { name: /first memory|创建第一条/i })).toBeInTheDocument()
  })

  it('renders memory cards when entries exist', async () => {
    vi.mocked(api.getMemoryStats).mockResolvedValue({
      total: 2,
      by_category: { preference: 1, decision: 1 },
      by_project: { 'web-app': 2 },
      most_recent_at: null,
    })
    vi.mocked(api.listMemories).mockResolvedValue([
      {
        id: 'm1',
        project: 'web-app',
        category: 'preference',
        content: 'Use tabs not spaces',
        tags: ['indent'],
        confidence: 0.9,
        created_at: '2026-06-01T00:00:00Z',
        accessed_at: '2026-06-01T00:00:00Z',
        access_count: 3,
      },
      {
        id: 'm2',
        project: 'web-app',
        category: 'decision',
        content: 'Adopt React Query for server state',
        tags: ['react', 'data'],
        confidence: 1.0,
        created_at: '2026-06-02T00:00:00Z',
        accessed_at: '2026-06-02T00:00:00Z',
        access_count: 0,
      },
    ])
    vi.mocked(api.listMemoryProjects).mockResolvedValue(['web-app'])

    render(<MemoryPanel />)
    await waitFor(() => {
      expect(screen.getByText('Use tabs not spaces')).toBeInTheDocument()
    })

    expect(screen.getByText('Adopt React Query for server state')).toBeInTheDocument()
    expect(screen.getByText('#indent')).toBeInTheDocument()
    expect(screen.getByText(/Used 3/)).toBeInTheDocument()
  })

  it('opens editor when Create button clicked', async () => {
    vi.mocked(api.getMemoryStats).mockResolvedValue({
      total: 0, by_category: {}, by_project: {}, most_recent_at: null,
    })
    vi.mocked(api.listMemories).mockResolvedValue([])
    vi.mocked(api.listMemoryProjects).mockResolvedValue([])

    render(<MemoryPanel />)
    await waitFor(() => {
      expect(screen.queryByText(/Loading/)).not.toBeInTheDocument()
    })

    // There are two create buttons in empty state — header + empty CTA.
    const createButtons = screen.getAllByRole('button', { name: /first memory|New memory|新建记忆|创建第一条/i })
    fireEvent.click(createButtons[0])

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Save|保存/i })).toBeInTheDocument()
    })
  })

  it('deletes memory after confirm', async () => {
    vi.mocked(api.deleteMemory).mockResolvedValue(true)
    vi.mocked(api.getMemoryStats).mockResolvedValue({
      total: 1,
      by_category: { preference: 1 },
      by_project: { '.': 1 },
      most_recent_at: null,
    })
    vi.mocked(api.listMemories).mockResolvedValue([
      {
        id: 'm1',
        project: '.',
        category: 'preference',
        content: 'Delete me',
        tags: [],
        confidence: 1.0,
        created_at: '2026-06-01T00:00:00Z',
        accessed_at: '2026-06-01T00:00:00Z',
        access_count: 0,
      },
    ])
    vi.mocked(api.listMemoryProjects).mockResolvedValue(['.'])

    render(<MemoryPanel />)
    await waitFor(() => {
      expect(screen.getByText('Delete me')).toBeInTheDocument()
    })

    const deleteBtn = screen.getByRole('button', { name: 'Delete' })
    fireEvent.click(deleteBtn)

    const dialog = await screen.findByRole('alertdialog')
    const confirmBtn = within(dialog).getByRole('button', { name: /^Delete$/i })
    fireEvent.click(confirmBtn)

    await waitFor(() => {
      expect(api.deleteMemory).toHaveBeenCalledWith('m1')
    })
  })
})

describe('MemoryPanel — dream distillation section', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.getMemoryStats).mockResolvedValue({
      total: 0, by_category: {}, by_project: {}, most_recent_at: null,
    })
    vi.mocked(api.listMemories).mockResolvedValue([])
    vi.mocked(api.listMemoryProjects).mockResolvedValue([])
  })

  it('renders the dream section empty state when no proposals are pending', async () => {
    vi.mocked(api.listDreamProposals).mockResolvedValue([])

    render(<MemoryPanel />)

    expect(screen.getByTestId('dream-panel')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Run one dream distillation pass now' })).toBeInTheDocument()
    expect(await screen.findByText('No pending proposals')).toBeInTheDocument()
  })

  it('renders a proposal with grouped actions and content summaries', async () => {
    vi.mocked(api.listDreamProposals).mockResolvedValue([dreamProposal])

    render(<MemoryPanel />)

    // Grouped kind badges (merge / remove / add counts).
    expect(await screen.findByText('Merge 1')).toBeInTheDocument()
    expect(screen.getByText('Remove 1')).toBeInTheDocument()
    expect(screen.getByText('Add 1')).toBeInTheDocument()
    // Per-action rationale + summaries.
    expect(screen.getByText('Two entries say the same thing about deploys')).toBeInTheDocument()
    expect(screen.getByText('Outdated note about the staging URL')).toBeInTheDocument()
    expect(screen.getByText('2 memories')).toBeInTheDocument()
    expect(screen.getByText('Repeatedly expressed package-manager preference')).toBeInTheDocument()
    expect(screen.getByText('Prefer pnpm over npm')).toBeInTheDocument()
    // Footer actions + selection count (all selected by default).
    expect(screen.getByRole('button', { name: /Apply selected/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Discard all/ })).toBeInTheDocument()
    expect(screen.getByText('3/3 selected')).toBeInTheDocument()
  })

  it('applies only the still-selected action ids and refreshes the list', async () => {
    vi.mocked(api.listDreamProposals).mockResolvedValue([dreamProposal])
    vi.mocked(api.applyDreamProposal).mockResolvedValue({ applied: ['a1', 'a3'], skipped: [] })

    render(<MemoryPanel />)

    // Uncheck the remove action → apply must send only the remaining ids.
    await screen.findByText('Merge 1')
    fireEvent.click(screen.getByLabelText('Select action a2'))
    expect(screen.getByText('2/3 selected')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /Apply selected/ }))

    await waitFor(() => {
      expect(api.applyDreamProposal).toHaveBeenCalledTimes(1)
    })
    expect(api.applyDreamProposal).toHaveBeenCalledWith('proposal-1', ['a1', 'a3'])
    // The review list is re-fetched after the apply.
    await waitFor(() => {
      expect(api.listDreamProposals).toHaveBeenCalledTimes(2)
    })
  })
})
