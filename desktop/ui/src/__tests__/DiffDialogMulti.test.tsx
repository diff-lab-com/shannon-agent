import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { toast } from 'sonner'
import DiffDialogMulti from '@/components/diff/DiffDialogMulti'
import FileDiffList from '@/components/diff/FileDiffList'
import * as api from '@/lib/tauri-api'
import type { FileDiff } from '@/types'

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

const fileA: FileDiff = {
  old_content: 'line one\nline two',
  new_content: 'line one\nline two edited',
  file_name: 'src/a.ts',
  language: 'typescript',
  mtime: '2026-09-26T00:00:00+00:00',
}

const fileB: FileDiff = {
  old_content: 'foo\nbar',
  new_content: 'foo\nBAR\nbaz',
  file_name: 'src/b.md',
  language: 'markdown',
  mtime: '2026-09-26T01:00:00+00:00',
}

const fileC: FileDiff = {
  old_content: 'same',
  new_content: 'same',
  file_name: 'src/c.txt',
  language: 'text',
}

/** A diff whose accept would empty a non-empty file (P1-31 deletion guard). */
const wholeFileDeletion: FileDiff = {
  old_content: 'real content\nmore content',
  new_content: '',
  file_name: 'src/wiped.txt',
  language: 'text',
  mtime: '2026-09-26T02:00:00+00:00',
}

describe('FileDiffList', () => {
  afterEach(() => cleanup())

  it('renders all files with name and +/- counts', () => {
    const diffs = new Map([
      ['src/a.ts', fileA],
      ['src/b.md', fileB],
    ])
    const decisions = new Map()
    render(
      <FileDiffList
        files={['src/a.ts', 'src/b.md']}
        diffs={diffs}
        decisions={decisions}
        currentPath="src/a.ts"
        filter="all"
        onSelectPath={() => {}}
        onFilterChange={() => {}}
      />,
    )
    expect(screen.getByText('src/a.ts')).toBeInTheDocument()
    expect(screen.getByText('src/b.md')).toBeInTheDocument()
  })

  it('calls onSelectPath when a file row is clicked', () => {
    const onSelect = vi.fn()
    render(
      <FileDiffList
        files={['src/a.ts']}
        diffs={new Map([['src/a.ts', fileA]])}
        decisions={new Map()}
        currentPath={null}
        filter="all"
        onSelectPath={onSelect}
        onFilterChange={() => {}}
      />,
    )
    fireEvent.click(screen.getByText('src/a.ts'))
    expect(onSelect).toHaveBeenCalledWith('src/a.ts')
  })

  it('shows Unreviewed status pill for a file with no decisions', () => {
    render(
      <FileDiffList
        files={['src/a.ts']}
        diffs={new Map([['src/a.ts', fileA]])}
        decisions={new Map()}
        currentPath="src/a.ts"
        filter="all"
        onSelectPath={() => {}}
        onFilterChange={() => {}}
      />,
    )
    expect(screen.getByText('Unreviewed')).toBeInTheDocument()
  })
})

describe('DiffDialogMulti', () => {
  beforeEach(() => {
    vi.mocked(api.getFileDiff).mockReset()
    vi.mocked(api.saveTextFile).mockReset()
    vi.mocked(toast.success).mockClear()
    vi.mocked(toast.error).mockClear()
  })

  afterEach(() => cleanup())

  it('renders nothing when closed', () => {
    render(<DiffDialogMulti open={false} filePaths={[]} onClose={() => {}} />)
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('shows empty-state message when filePaths is empty', () => {
    render(<DiffDialogMulti open={true} filePaths={[]} onClose={() => {}} />)
    expect(screen.getAllByText('No files to review.').length).toBeGreaterThan(0)
  })

  it('fetches each file diff in parallel on open', async () => {
    vi.mocked(api.getFileDiff).mockImplementation((path: string) =>
      path === 'src/a.ts' ? Promise.resolve(fileA) : Promise.resolve(fileB),
    )
    render(<DiffDialogMulti open={true} filePaths={['src/a.ts', 'src/b.md']} onClose={() => {}} />)
    await waitFor(() => expect(api.getFileDiff).toHaveBeenCalledTimes(2))
    expect(api.getFileDiff).toHaveBeenCalledWith('src/a.ts')
    expect(api.getFileDiff).toHaveBeenCalledWith('src/b.md')
  })

  it('switches current file when sidebar row is clicked', async () => {
    vi.mocked(api.getFileDiff).mockImplementation((path: string) =>
      path === 'src/a.ts' ? Promise.resolve(fileA) : Promise.resolve(fileB),
    )
    render(<DiffDialogMulti open={true} filePaths={['src/a.ts', 'src/b.md']} onClose={() => {}} />)
    await waitFor(() => expect(screen.getByText('line two edited')).toBeInTheDocument())
    expect(screen.queryByText('BAR')).not.toBeInTheDocument()

    fireEvent.click(screen.getByText('src/b.md'))
    await waitFor(() => expect(screen.getByText('BAR')).toBeInTheDocument())
  })

  it('Apply all is disabled until at least one file has an accepted hunk', async () => {
    vi.mocked(api.getFileDiff).mockResolvedValue(fileA)
    render(<DiffDialogMulti open={true} filePaths={['src/a.ts']} onClose={() => {}} />)
    await waitFor(() => expect(screen.getByRole('dialog')).toBeInTheDocument())
    const applyBtns = screen.getAllByRole('button', { name: /apply accepted hunks to disk/i })
    expect(applyBtns[0]).toBeDisabled()
  })

  it('writes every file with at least one accepted hunk on Apply all', async () => {
    vi.mocked(api.getFileDiff).mockImplementation((path: string) =>
      path === 'src/a.ts' ? Promise.resolve(fileA) : Promise.resolve(fileB),
    )
    vi.mocked(api.saveTextFile).mockResolvedValue(undefined)
    const onClose = vi.fn()
    render(<DiffDialogMulti open={true} filePaths={['src/a.ts', 'src/b.md']} onClose={onClose} />)
    await waitFor(() => expect(screen.getByText('line two edited')).toBeInTheDocument())

    // Accept all in file A (current)
    fireEvent.click(screen.getByText('Accept all'))
    // Switch to file B and accept there too
    fireEvent.click(screen.getByText('src/b.md'))
    await waitFor(() => expect(screen.getByText('BAR')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Accept all'))

    const applyBtn = screen.getByRole('button', { name: /apply accepted hunks to disk/i })
    expect(applyBtn).not.toBeDisabled()
    fireEvent.click(applyBtn)

    await waitFor(() => expect(api.saveTextFile).toHaveBeenCalledTimes(2))
    // B4 P1-31: each write carries the fetch-time mtime for the backend's
    // stale-write conflict check.
    expect(api.saveTextFile).toHaveBeenCalledWith('src/a.ts', expect.any(String), fileA.mtime)
    expect(api.saveTextFile).toHaveBeenCalledWith('src/b.md', expect.any(String), fileB.mtime)
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('reports a partial apply failure with counts and failed file names', async () => {
    vi.mocked(api.getFileDiff).mockImplementation((path: string) =>
      path === 'src/a.ts' ? Promise.resolve(fileA) : Promise.resolve(fileB),
    )
    vi.mocked(api.saveTextFile).mockImplementation((path: string) =>
      path === 'src/a.ts'
        ? Promise.reject(new Error('Disk full'))
        : Promise.resolve(undefined),
    )
    const onClose = vi.fn()
    render(<DiffDialogMulti open={true} filePaths={['src/a.ts', 'src/b.md']} onClose={onClose} />)
    await waitFor(() => expect(screen.getByText('line two edited')).toBeInTheDocument())

    fireEvent.click(screen.getByText('Accept all'))
    fireEvent.click(screen.getByText('src/b.md'))
    await waitFor(() => expect(screen.getByText('BAR')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Accept all'))

    fireEvent.click(screen.getByRole('button', { name: /apply accepted hunks to disk/i }))

    await waitFor(() => expect(api.saveTextFile).toHaveBeenCalledTimes(2))
    // Modal stays open for re-review…
    expect(onClose).not.toHaveBeenCalled()
    // …and the toast says what actually happened (1 written, 1 failed, which).
    await waitFor(() => expect(toast.error).toHaveBeenCalledTimes(1))
    expect(toast.error).toHaveBeenCalledWith(
      'Wrote 1 of 2 files — 1 failed',
      { description: 'Failed files: src/a.ts' },
    )
  })

  it('shows the mtime-conflict copy when every failing write is a conflict', async () => {
    vi.mocked(api.getFileDiff).mockResolvedValue(fileA)
    vi.mocked(api.saveTextFile).mockRejectedValue({
      code: 'mtime_conflict',
      message: 'file changed since it was read',
    })
    render(<DiffDialogMulti open={true} filePaths={['src/a.ts']} onClose={() => {}} />)
    fireEvent.click(await screen.findByText('Accept all'))
    fireEvent.click(screen.getByRole('button', { name: /apply accepted hunks to disk/i }))

    await waitFor(() => expect(toast.error).toHaveBeenCalledTimes(1))
    expect(toast.error).toHaveBeenCalledWith(
      'File changed since the diff was loaded',
      { description: expect.stringContaining('modified while you were reviewing') },
    )
    expect(api.saveTextFile).toHaveBeenCalledWith('src/a.ts', expect.any(String), fileA.mtime)
  })

  it('names the offending files and disables Apply all when a batch diff would empty a file', async () => {
    vi.mocked(api.getFileDiff).mockImplementation((path: string) =>
      path === 'src/a.ts' ? Promise.resolve(fileA) : Promise.resolve(wholeFileDeletion),
    )
    render(<DiffDialogMulti open={true} filePaths={['src/a.ts', 'src/wiped.txt']} onClose={() => {}} />)
    // Accept everything in the visible file — the guard must hold regardless
    // of decisions.
    fireEvent.click(await screen.findByText('Accept all'))
    const alert = screen.getByRole('alert')
    expect(alert).toHaveTextContent(/Apply all is disabled/)
    expect(alert).toHaveTextContent('src/wiped.txt')
    const applyBtn = screen.getByRole('button', { name: /apply accepted hunks to disk/i })
    expect(applyBtn).toBeDisabled()
    expect(api.saveTextFile).not.toHaveBeenCalled()
  })

  it('filter pills narrow the visible file list', async () => {
    vi.mocked(api.getFileDiff).mockImplementation((path: string) =>
      path === 'src/a.ts'
        ? Promise.resolve(fileA)
        : path === 'src/b.md'
          ? Promise.resolve(fileB)
          : Promise.resolve(fileC),
    )
    render(
      <DiffDialogMulti open={true} filePaths={['src/a.ts', 'src/b.md', 'src/c.txt']} onClose={() => {}} />,
    )
    await waitFor(() => expect(screen.getByText('line two edited')).toBeInTheDocument())
    expect(screen.getAllByText('src/a.ts').length).toBeGreaterThan(0)
    expect(screen.getAllByText('src/b.md').length).toBeGreaterThan(0)
    expect(screen.getAllByText('src/c.txt').length).toBeGreaterThan(0)

    // Switch to "accepted" filter — nothing accepted yet, sidebar empties.
    fireEvent.click(screen.getByText('Has accepted'))
    // Sidebar empty state appears (sidebar pill labels match filter pills)
    expect(screen.getAllByText('No files to review.').length).toBeGreaterThan(0)
  })
})
