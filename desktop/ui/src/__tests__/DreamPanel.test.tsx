// Dream panel review-fix coverage: the report viewer distinguishes a failed
// fetch from the empty state (finding #8), and the apply flow surfaces how
// many actions were skipped because their targets vanished (finding #9).
// 卡C adds the cold-start persisted stats line (read_dream_state read-back)
// and the fetchProposals selection-key pruning.

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
  readDreamState: vi.fn(),
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
  vi.mocked(api.readDreamState).mockResolvedValue({ last_dream_at: null, last_stats: null })
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

describe('DreamPanel — cold-start persisted stats line (卡C)', () => {
  it('shows 「Last distilled」 with the persisted stats on mount', async () => {
    vi.mocked(api.readDreamState).mockResolvedValue({
      last_dream_at: '2026-09-25T03:00:00+00:00',
      last_stats: {
        scanned_sessions: 3,
        merge_proposed: 1,
        remove_proposed: 0,
        add_proposed: 2,
        candidates_detected: 4,
      },
    })
    render(<DreamPanel />, { wrapper })

    // One combined line: time · stats summary (nested ICU message).
    await waitFor(() => {
      expect(screen.getByText(/Last distilled:/)).toBeInTheDocument()
    })
    expect(screen.getByText(/3 session\(s\) scanned/)).toBeInTheDocument()
    expect(screen.getByText(/2 addition\(s\)/)).toBeInTheDocument()
  })

  it('falls back to the timestamp-only wording when no stats were persisted', async () => {
    vi.mocked(api.readDreamState).mockResolvedValue({
      last_dream_at: '2026-09-25T03:00:00+00:00',
      last_stats: null,
    })
    render(<DreamPanel />, { wrapper })

    await waitFor(() => {
      expect(screen.getByText(/Last distilled:/)).toBeInTheDocument()
    })
    expect(screen.queryByText(/session\(s\) scanned/)).not.toBeInTheDocument()
  })

  it('shows no line at all when no pass has ever run', async () => {
    vi.mocked(api.readDreamState).mockResolvedValue({ last_dream_at: null, last_stats: null })
    render(<DreamPanel />, { wrapper })

    await waitFor(() => {
      expect(api.readDreamState).toHaveBeenCalled()
    })
    expect(screen.queryByText(/Last distilled:/)).not.toBeInTheDocument()
  })
})

describe('DreamPanel — fetchProposals prunes consumed selection keys (卡C)', () => {
  function makeProposalWithThirdAction(): DreamProposal {
    const p = makeProposal()
    return {
      ...p,
      actions: [
        ...p.actions,
        { id: 'action-3', kind: 'merge', entry_ids: ['e-3', 'e-4'], add_entry: null, rationale: 'Dupes' },
      ],
    }
  }

  it('gives a re-appearing proposal id fresh all-selected defaults', async () => {
    // A consumed proposal's selection state must be pruned: if the same id
    // reappears later (new run, same ms tick), a stale set would leave the
    // new actions unchecked. ids are ms timestamps, so this is reachable.
    vi.mocked(api.listDreamProposals).mockResolvedValue([makeProposal()])
    vi.mocked(api.applyDreamProposal).mockResolvedValue({
      applied: ['action-2'],
      skipped: [],
    })
    vi.mocked(api.runDreamPass).mockResolvedValue({
      skipped_reason: null,
      scanned_sessions: 0,
      projects: [],
      merge_proposed: 0,
      remove_proposed: 0,
      add_proposed: 0,
      candidates_detected: 0,
      candidates_refined: 0,
      proposal_ids: [],
      report_path: null,
      duration_ms: 0,
    })
    render(<DreamPanel />, { wrapper })

    // Deselect action-1 → stored selection = {action-2} (1/2).
    fireEvent.click((await screen.findAllByRole('checkbox'))[0])
    expect(await screen.findByText('1/2 selected')).toBeInTheDocument()

    // The apply consumes the proposal server-side: the refetch it triggers
    // must see an empty list.
    vi.mocked(api.listDreamProposals).mockResolvedValue([])
    fireEvent.click(screen.getByRole('button', { name: /Apply selected/ }))
    await waitFor(() => {
      expect(api.applyDreamProposal).toHaveBeenCalledWith('proposal-1000', ['action-2'])
    })
    expect(await screen.findByText('No pending proposals')).toBeInTheDocument()

    // A new proposal with the same id but one extra action shows up.
    vi.mocked(api.listDreamProposals).mockResolvedValue([makeProposalWithThirdAction()])
    fireEvent.click(screen.getByRole('button', { name: /Run one dream distillation pass now/ }))

    // With pruning the key was dropped → fresh defaults → 3/3. Without it
    // the stale {action-2} set would render 2/3 (action-3 unchecked).
    expect(await screen.findByText('3/3 selected')).toBeInTheDocument()
  })
})
