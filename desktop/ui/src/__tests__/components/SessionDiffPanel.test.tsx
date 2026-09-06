// P1-5 C-2 — SessionDiffPanel: reuses the get_session_git_diff data flow
// and the existing DiffViewer, read-only, with calm empty states.

import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import * as api from '@/lib/tauri-api'
import { SessionDiffPanel } from '@/components/workspace/SessionDiffPanel'

vi.mock('@/lib/tauri-api', () => ({
  getSessionGitDiff: vi.fn(),
  getFileDiff: vi.fn(),
}))

const SUMMARY = {
  is_repo: true,
  files: [
    { path: 'src/a.ts', insertions: 3, deletions: 1 },
    { path: 'src/b.ts', insertions: 0, deletions: 2 },
  ],
  patch: '',
  truncated: false,
}

const FILE_DIFF = {
  path: 'src/a.ts',
  old_content: 'const a = 1\n',
  new_content: 'const a = 2\n',
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('SessionDiffPanel', () => {
  it('loads the session diff and renders the selected file read-only', async () => {
    vi.mocked(api.getSessionGitDiff).mockResolvedValue(SUMMARY)
    vi.mocked(api.getFileDiff).mockResolvedValue(FILE_DIFF)
    render(<SessionDiffPanel workingDir="/proj" />)

    await waitFor(() => expect(screen.getByRole('tab', { name: /src\/a\.ts/ })).toBeInTheDocument())
    // First file is auto-selected; its content is fetched with an absolute path.
    await waitFor(() => expect(api.getFileDiff).toHaveBeenCalledWith('/proj/src/a.ts'))
    expect(await screen.findByText('const a = 2')).toBeInTheDocument()
  })

  it('switches files on click', async () => {
    vi.mocked(api.getSessionGitDiff).mockResolvedValue(SUMMARY)
    vi.mocked(api.getFileDiff).mockResolvedValue({
      path: 'src/b.ts', old_content: 'b\n', new_content: '',
    })
    render(<SessionDiffPanel workingDir="/proj" />)
    await screen.findByRole('tab', { name: /src\/b\.ts/ })
    fireEvent.click(screen.getByRole('tab', { name: /src\/b\.ts/ }))
    await waitFor(() => expect(api.getFileDiff).toHaveBeenCalledWith('/proj/src/b.ts'))
  })

  it('shows the not-a-repo empty state', async () => {
    vi.mocked(api.getSessionGitDiff).mockResolvedValue({ is_repo: false, files: [], patch: '', truncated: false })
    render(<SessionDiffPanel workingDir="/proj" />)
    expect(await screen.findByText('The working directory is not a git repository')).toBeInTheDocument()
    expect(api.getFileDiff).not.toHaveBeenCalled()
  })

  it('shows the no-changes empty state', async () => {
    vi.mocked(api.getSessionGitDiff).mockResolvedValue({ is_repo: true, files: [], patch: '', truncated: false })
    render(<SessionDiffPanel workingDir="/proj" />)
    expect(await screen.findByText("No file changes in this session's working tree")).toBeInTheDocument()
  })

  it('shows a failure state when the diff cannot load', async () => {
    vi.mocked(api.getSessionGitDiff).mockRejectedValue(new Error('git missing'))
    render(<SessionDiffPanel workingDir="/proj" />)
    expect(await screen.findByText('Could not load the session diff')).toBeInTheDocument()
  })

  it('shows a per-file failure state without losing the file list', async () => {
    vi.mocked(api.getSessionGitDiff).mockResolvedValue(SUMMARY)
    vi.mocked(api.getFileDiff).mockRejectedValue(new Error('outside workspace'))
    render(<SessionDiffPanel workingDir="/proj" />)
    await waitFor(() => expect(screen.getByText("Could not load this file's diff")).toBeInTheDocument())
    expect(screen.getByRole('tab', { name: /src\/b\.ts/ })).toBeInTheDocument()
  })

  it('renders nothing fetched without a working directory', () => {
    render(<SessionDiffPanel workingDir={null} />)
    expect(api.getSessionGitDiff).not.toHaveBeenCalled()
  })
})
