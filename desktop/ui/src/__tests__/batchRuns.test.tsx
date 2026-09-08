// Tests for the P1-2 best-of-N batch UI: form count dispatch, batch card
// rendering + actions, the compare dialog's column selection, and the adopt
// confirm flow (including the conflict path).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from "@testing-library/react"
import { I18nProvider } from '@/i18n'
import BatchForm, { BATCH_COUNT_CHOICES } from '@/components/tasks/BatchForm'
import BatchRunPanel, { BatchRunCard } from '@/components/tasks/BatchRunPanel'
import BatchDiffCompare from '@/components/tasks/BatchDiffCompare'
import { useBatchRuns } from '@/hooks/batchRuns'
import * as api from '@/lib/tauri-api'
import type { BatchRunDto } from '@/types'

vi.mock('@/lib/tauri-api', () => ({
  listBatchRuns: vi.fn(),
  startBatchRun: vi.fn(),
  getBatchBranchDiff: vi.fn(),
  adoptBatchBranch: vi.fn(),
  discardBatchRun: vi.fn(),
}))

vi.mock('@/hooks/batchRuns', () => ({
  useBatchRuns: vi.fn(),
  isBatchTerminal: (run: { status: string }) => run.status !== 'running',
  adoptableBranches: (run: BatchRunDto) => run.branches.filter(b => b.status === 'completed'),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}))

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <I18nProvider>{children}</I18nProvider>
)

function mockHook(runs: BatchRunDto[]) {
  vi.mocked(useBatchRuns).mockReturnValue({
    runs,
    loading: false,
    error: null,
    refresh: async () => {},
    start: async () => 'batch-1',
    adopt: async () => ({ merged: true, conflicts: null }),
    discard: async () => ({ removed: 1, skipped: [] }),
  })
}

function makeBranch(i: number, status: 'running' | 'completed' | 'failed' = 'completed') {
  return {
    index: i,
    branchName: `batch-abcd1234-${i}`,
    worktreePath: `/repo/.shannon/scheduled-worktrees/batch-abcd1234-${i}`,
    status,
    error: status === 'failed' ? 'provider exploded' : null,
    summary:
      status === 'completed' ? { filesChanged: 3 + i, additions: 10, deletions: 2 } : null,
    spentUsd: 0.25 + 0.25 * i,
  }
}

function makeBatch(o: Partial<BatchRunDto>): BatchRunDto {
  return {
    batchId: 'batch-1',
    title: 'Speed up the search box',
    prompt: 'Make search fast',
    count: 2,
    status: 'completed',
    createdAtMs: 1_700_000_000_000,
    branches: [makeBranch(0), makeBranch(1)],
    adoptedIndex: null,
    ...o,
  }
}

beforeEach(() => {
  vi.mocked(api.listBatchRuns).mockResolvedValue([])
  vi.mocked(api.getBatchBranchDiff).mockResolvedValue({ diff: '' })
  mockHook([])
})

// ── BatchForm ────────────────────────────────────────────────────────────

describe('BatchForm', () => {
  it('offers exactly 2/3/4 attempts', () => {
    render(<BatchForm sessionId="sess-1" onSubmit={() => {}} onCancel={() => {}} />, { wrapper })
    expect(BATCH_COUNT_CHOICES).toEqual([2, 3, 4])
    for (const n of BATCH_COUNT_CHOICES) {
      expect(screen.getByRole('radio', { name: String(n) })).toBeTruthy()
    }
  })

  it('dispatches the chosen count with prompt, title and session', () => {
    const onSubmit = vi.fn()
    render(<BatchForm sessionId="sess-9" onSubmit={onSubmit} onCancel={() => {}} />, { wrapper })

    fireEvent.change(screen.getByLabelText(/batch prompt|批次提示词/i), {
      target: { value: 'Fix the flaky test' },
    })
    fireEvent.change(screen.getByLabelText(/batch title|批次标题/i), {
      target: { value: 'Flaky fix' },
    })
    fireEvent.click(screen.getByRole('radio', { name: '3' }))
    fireEvent.click(screen.getByText(/start 3 attempts|启动 3 个方案/i))

    expect(onSubmit).toHaveBeenCalledTimes(1)
    expect(onSubmit).toHaveBeenCalledWith({
      title: 'Flaky fix',
      prompt: 'Fix the flaky test',
      count: 3,
      sessionId: 'sess-9',
    })
  })

  it('defaults to 2 attempts and refuses an empty prompt', () => {
    const onSubmit = vi.fn()
    render(<BatchForm sessionId={null} onSubmit={onSubmit} onCancel={() => {}} />, { wrapper })
    const start = screen.getByText(/start 2 attempts|启动 2 个方案/i)
    expect((start.closest('button') ?? (start as HTMLButtonElement)).disabled).toBe(true)
    expect(onSubmit).not.toHaveBeenCalled()

    // Fill the prompt; submit dispatches with count 2 and null session.
    fireEvent.change(screen.getByLabelText(/batch prompt|批次提示词/i), {
      target: { value: 'go' },
    })
    fireEvent.click(screen.getByText(/start 2 attempts|启动 2 个方案/i))
    expect(onSubmit).toHaveBeenCalledWith({
      title: '',
      prompt: 'go',
      count: 2,
      sessionId: null,
    })
  })
})

// ── BatchRunCard ─────────────────────────────────────────────────────────

describe('BatchRunCard', () => {
  const noop = () => {}

  it('renders the title, per-branch chips (files + spent) and total spend', () => {
    render(<BatchRunCard run={makeBatch({})} onCompare={noop} onDiscard={noop} />, { wrapper })

    expect(screen.getByText('Speed up the search box')).toBeTruthy()
    const chips = screen.getAllByTestId(/^batch-branch-chip-/)
    expect(chips.length).toBe(2)
    // Branch 1 chip shows its filesChanged (3 + 1).
    expect(screen.getByTestId('batch-branch-chip-1').textContent).toContain('4')
    // Terminal (completed) batch: Compare + Discard available.
    expect(screen.getByRole('button', { name: /compare batch branches/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /discard this batch/i })).toBeTruthy()
  })

  it('hides Compare/Discard while the batch is still running', () => {
    render(
      <BatchRunCard
        run={makeBatch({
          status: 'running',
          branches: [makeBranch(0, 'running'), makeBranch(1, 'running')],
        })}
        onCompare={noop}
        onDiscard={noop}
      />,
      { wrapper },
    )
    expect(screen.queryByRole('button', { name: /compare batch branches/i })).toBeNull()
    expect(screen.queryByRole('button', { name: /discard this batch/i })).toBeNull()
  })

  it('shows the failed branch error on its chip; partial batches can compare', () => {
    render(
      <BatchRunCard
        run={makeBatch({
          status: 'partially_failed',
          branches: [makeBranch(0), makeBranch(1, 'failed')],
        })}
        onCompare={noop}
        onDiscard={noop}
      />,
      { wrapper },
    )
    const failedChip = screen.getByTestId('batch-branch-chip-1')
    expect(failedChip.getAttribute('title')).toContain('provider exploded')
    expect(screen.getByRole('button', { name: /compare batch branches/i })).toBeTruthy()
  })
})

// ── BatchRunPanel (list + actions) ───────────────────────────────────────

describe('BatchRunPanel', () => {
  it('renders the hook runs and dispatches discard after confirmation', async () => {
    const discard = vi.fn().mockResolvedValue({ removed: 1, skipped: [] })
    vi.mocked(useBatchRuns).mockReturnValue({
      runs: [makeBatch({})],
      loading: false,
      error: null,
      refresh: async () => {},
      start: async () => 'batch-1',
      adopt: async () => ({ merged: true, conflicts: null }),
      discard,
    })

    render(<BatchRunPanel />, { wrapper })
    expect(screen.getByText('Speed up the search box')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /discard this batch/i }))
    // Guarded by a destructive confirm — nothing removed before confirming.
    expect(discard).not.toHaveBeenCalled()
    fireEvent.click(await screen.findByRole('button', { name: /discard batch|放弃批次/i }))
    await waitFor(() => expect(discard).toHaveBeenCalledWith('batch-1'))
  })

  it('opens the compare dialog and fetches a diff per branch', async () => {
    mockHook([makeBatch({})])
    vi.mocked(api.getBatchBranchDiff).mockResolvedValue({
      diff: 'diff --git a/f b/f\n+++ b/f\n@@ -1 +1 @@\n-old\n+new\n',
    })

    render(<BatchRunPanel />, { wrapper })
    fireEvent.click(await screen.findByRole('button', { name: /compare batch branches/i }))

    await waitFor(() =>
      expect(vi.mocked(api.getBatchBranchDiff)).toHaveBeenCalledWith('batch-1', 0),
    )
    expect(vi.mocked(api.getBatchBranchDiff)).toHaveBeenCalledWith('batch-1', 1)
    await waitFor(() => expect(screen.getAllByText(/\+new/).length).toBe(2))
  })

  it('is hidden when there are no runs', () => {
    render(<BatchRunPanel />, { wrapper })
    expect(screen.queryByTestId('batch-run-panel')).toBeNull()
  })
})

// ── BatchDiffCompare (columns + adopt confirm + conflicts) ───────────────

describe('BatchDiffCompare', () => {
  it('hides itself without a run', () => {
    render(<BatchDiffCompare run={null} onClose={() => {}} onAdopt={async () => null} />, {
      wrapper,
    })
    expect(screen.queryByTestId('batch-diff-columns')).toBeNull()
  })

  it('defaults the selection to completed branches and renders their diffs', async () => {
    vi.mocked(api.getBatchBranchDiff).mockImplementation(async (_id, index) => ({
      diff: `patch for branch ${index}`,
    }))
    render(
      <BatchDiffCompare
        run={makeBatch({
          branches: [makeBranch(0), makeBranch(1), makeBranch(2, 'failed')],
        })}
        onClose={() => {}}
        onAdopt={async () => ({ merged: true, conflicts: null })}
      />,
      { wrapper },
    )

    await waitFor(() => screen.getByTestId('batch-diff-column-0'))
    // Completed branches selected by default; the failed branch is not.
    expect(screen.getByTestId('batch-diff-column-0')).toBeTruthy()
    expect(screen.getByTestId('batch-diff-column-1')).toBeTruthy()
    expect(screen.queryByTestId('batch-diff-column-2')).toBeNull()
    expect(vi.mocked(api.getBatchBranchDiff)).toHaveBeenCalledWith('batch-1', 0)
    expect(vi.mocked(api.getBatchBranchDiff)).toHaveBeenCalledWith('batch-1', 1)
    expect(screen.getByText(/patch for branch 0/)).toBeTruthy()
  })

  it('adopts through the confirm dialog and closes on success', async () => {
    const onAdopt = vi.fn().mockResolvedValue({ merged: true, conflicts: null })
    const onClose = vi.fn()
    render(<BatchDiffCompare run={makeBatch({})} onClose={onClose} onAdopt={onAdopt} />, {
      wrapper,
    })

    fireEvent.click(await screen.findByRole('button', { name: /adopt branch #0|采纳分支 #0/i }))
    // Confirm dialog explains merge + cleanup of the others.
    expect(
      screen.getByText(/merges the branch into your current branch|将把该分支合并/i),
    ).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /merge & clean up|合并并清理/i }))

    await waitFor(() => expect(onAdopt).toHaveBeenCalledWith('batch-1', 0))
    await waitFor(() => expect(onClose).toHaveBeenCalled())
  })

  it('keeps the dialog open and lists conflicts when the merge conflicts', async () => {
    const onAdopt = vi.fn().mockResolvedValue({ merged: false, conflicts: ['src/search.ts'] })
    render(<BatchDiffCompare run={makeBatch({})} onClose={() => {}} onAdopt={onAdopt} />, {
      wrapper,
    })

    fireEvent.click(await screen.findByRole('button', { name: /adopt branch #1|采纳分支 #1/i }))
    fireEvent.click(screen.getByRole('button', { name: /merge & clean up|合并并清理/i }))

    await waitFor(() => expect(onAdopt).toHaveBeenCalledWith('batch-1', 1))
    const banner = await screen.findByTestId('batch-conflict-banner')
    expect(banner.textContent).toContain('src/search.ts')
    expect(banner.textContent).toMatch(/kept for manual handling|原样保留供人工处理/)
    // The dialog stays open — nothing was merged or deleted.
    expect(screen.getByTestId('batch-diff-columns')).toBeTruthy()
  })

  it('marks the adopted column and hides adopt buttons once adopted', async () => {
    render(
      <BatchDiffCompare
        run={makeBatch({ status: 'adopted', adoptedIndex: 1 })}
        onClose={() => {}}
        onAdopt={async () => null}
      />,
      { wrapper },
    )
    await waitFor(() => screen.getByTestId('batch-diff-column-0'))
    expect(screen.getByText(/adopted|已采纳/i)).toBeTruthy()
    expect(screen.queryByRole('button', { name: /adopt branch #/i })).toBeNull()
  })
})
