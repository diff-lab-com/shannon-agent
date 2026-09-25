// Dream panel review-fix coverage: the report viewer distinguishes a failed
// fetch from the empty state (finding #8), and the apply flow surfaces how
// many actions were skipped because their targets vanished (finding #9).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import DreamPanel from '@/components/memory/DreamPanel'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'
import type { DreamProposal } from '@/lib/tauri-api'

vi.mock('@/lib/tauri-api', () => ({
  listDreamProposals: vi.fn(),
  readDreamReport: vi.fn(),
  runDreamPass: vi.fn(),
  applyDreamProposal: vi.fn(),
  discardDreamProposal: vi.fn(),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn(), message: vi.fn() },
}))

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <I18nProvider>{children}</I18nProvider>
)

function makeProposal(): DreamProposal {
  return {
    id: 'proposal-1000',
    project: '/work/app',
    created_at: '2026-09-25T00:00:00+00:00',
    actions: [
      {
        id: 'action-1',
        kind: 'remove',
        entry_ids: ['entry-1'],
        add_entry: null,
        rationale: 'Stale entry',
      },
      {
        id: 'action-2',
        kind: 'remove',
        entry_ids: ['entry-2'],
        add_entry: null,
        rationale: 'Ghost entry',
      },
    ],
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(api.listDreamProposals).mockResolvedValue([])
})

describe('DreamPanel — report viewer error vs empty state (finding #8)', () => {
  it('shows an error body, not the empty-state text, when reading the report fails', async () => {
    vi.mocked(api.readDreamReport).mockRejectedValue('no report file')
    render(<DreamPanel />, { wrapper })

    fireEvent.click(screen.getByRole('button', { name: /View last report/ }))

    await waitFor(() => {
      expect(
        screen.getByText('The report could not be loaded — details are in the notification.'),
      ).toBeInTheDocument()
    })
    expect(screen.queryByText('No report yet — run a pass first.')).not.toBeInTheDocument()
    expect(toast.error).toHaveBeenCalledWith(
      'Could not load the report',
      expect.objectContaining({ description: 'no report file' }),
    )
  })

  it('keeps the empty-state text when there is genuinely no report error', async () => {
    vi.mocked(api.readDreamReport).mockResolvedValue('# Dream Pass Report\n')
    render(<DreamPanel />, { wrapper })

    fireEvent.click(screen.getByRole('button', { name: /View last report/ }))

    await waitFor(() => {
      expect(screen.getByText(/Dream Pass Report/)).toBeInTheDocument()
    })
    expect(screen.queryByText(/could not be loaded/)).not.toBeInTheDocument()
  })
})

describe('DreamPanel — apply flow skipped toast (finding #9)', () => {
  it('extends the success toast with the skipped count when targets vanished', async () => {
    vi.mocked(api.listDreamProposals).mockResolvedValue([makeProposal()])
    vi.mocked(api.applyDreamProposal).mockResolvedValue({
      applied: ['action-1'],
      skipped: ['action-2'],
    })
    render(<DreamPanel />, { wrapper })

    fireEvent.click(await screen.findByRole('button', { name: /Apply selected/ }))

    await waitFor(() => {
      expect(api.applyDreamProposal).toHaveBeenCalledWith('proposal-1000', [
        'action-1',
        'action-2',
      ])
    })
    await waitFor(() => {
      expect(toast.success).toHaveBeenCalledWith(
        expect.stringContaining('1 action applied'),
        expect.objectContaining({
          description: '1 action was skipped — its target memory entries no longer exist.',
        }),
      )
    })
  })

  it('adds no skipped note when every selected action landed', async () => {
    vi.mocked(api.listDreamProposals).mockResolvedValue([makeProposal()])
    vi.mocked(api.applyDreamProposal).mockResolvedValue({
      applied: ['action-1', 'action-2'],
      skipped: [],
    })
    render(<DreamPanel />, { wrapper })

    fireEvent.click(await screen.findByRole('button', { name: /Apply selected/ }))

    await waitFor(() => {
      expect(toast.success).toHaveBeenCalledWith(
        expect.stringContaining('2 actions applied'),
        { description: undefined },
      )
    })
  })
})
