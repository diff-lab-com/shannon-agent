import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent, within } from '@testing-library/react'
import MemoryPanel from '@/components/memory/MemoryPanel'
import * as api from '@/lib/tauri-api'

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
