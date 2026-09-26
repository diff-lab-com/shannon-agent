import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import DiffReviewBody from '@/components/diff/DiffReviewBody'
import * as api from '@/lib/tauri-api'

const sampleDiff = {
  old_content: 'line one\nline two',
  new_content: 'line one\nline two edited\nline three',
  file_name: 'src/app.ts',
  language: 'typescript',
  mtime: '2026-09-26T00:00:00+00:00',
}

describe('DiffReviewBody — apply guards (B0 P0-3 / P0-4)', () => {
  beforeEach(() => {
    vi.mocked(api.getFileDiff).mockReset()
    vi.mocked(api.saveTextFile).mockReset()
  })

  afterEach(() => {
    cleanup()
  })

  it('passes the fetch-time mtime to save_text_file', async () => {
    vi.mocked(api.getFileDiff).mockResolvedValue(sampleDiff)
    vi.mocked(api.saveTextFile).mockResolvedValue(undefined)
    render(<DiffReviewBody filePath="src/app.ts" onClose={() => {}} active />)
    fireEvent.click(await screen.findByText('Accept all'))
    fireEvent.click(screen.getByRole('button', { name: /apply accepted hunks to disk/i }))
    await waitFor(() => expect(api.saveTextFile).toHaveBeenCalledTimes(1))
    const [, , expectedMtime] = vi.mocked(api.saveTextFile).mock.calls[0]
    expect(expectedMtime).toBe(sampleDiff.mtime)
  })

  it('collapses double Enter into a single apply round-trip', async () => {
    vi.mocked(api.getFileDiff).mockResolvedValue(sampleDiff)
    // Never-resolving save keeps the first apply in flight, so a second
    // Enter must be swallowed by the applying re-entry guard.
    vi.mocked(api.saveTextFile).mockReturnValue(new Promise(() => {}))
    const onClose = vi.fn()
    const { container } = render(
      <DiffReviewBody filePath="src/app.ts" onClose={onClose} active />,
    )
    fireEvent.click(await screen.findByText('Accept all'))

    // Both Enters land inside the keyboard gate in the same tick.
    const root = container.firstElementChild as HTMLElement
    fireEvent.keyDown(root, { key: 'Enter' })
    fireEvent.keyDown(root, { key: 'Enter' })

    await waitFor(() => expect(api.saveTextFile).toHaveBeenCalledTimes(1))
    expect(onClose).not.toHaveBeenCalled()
  })

  it('renders the binary-file guard message when the backend rejects the read', async () => {
    vi.mocked(api.getFileDiff).mockRejectedValue({ code: 'binary_file', message: 'binary file' })
    render(<DiffReviewBody filePath="img.png" onClose={() => {}} active />)
    await waitFor(() =>
      expect(screen.getByText('Binary file — text diff review is not available for this file.'))
        .toBeInTheDocument(),
    )
    expect(screen.queryByRole('button', { name: /apply accepted hunks to disk/i })).not.toBeInTheDocument()
  })

  it('warns and disables Apply when the diff would blank a non-empty file', async () => {
    vi.mocked(api.getFileDiff).mockResolvedValue({
      ...sampleDiff,
      old_content: 'precious\ncontent\n',
      new_content: '',
    })
    render(<DiffReviewBody filePath="keep.txt" onClose={() => {}} active />)
    await waitFor(() => expect(screen.getByRole('alert')).toBeInTheDocument())
    fireEvent.click(await screen.findByText('Accept all'))
    const applyBtn = screen.getByRole('button', { name: /apply accepted hunks to disk/i })
    expect(applyBtn).toBeDisabled()
  })

  it('shows the conflict message and stays open when the mtime check rejects the save', async () => {
    vi.mocked(api.getFileDiff).mockResolvedValue(sampleDiff)
    vi.mocked(api.saveTextFile).mockRejectedValue({
      code: 'mtime_conflict',
      message: 'file changed since it was read',
    })
    const onClose = vi.fn()
    render(<DiffReviewBody filePath="src/app.ts" onClose={onClose} active />)
    fireEvent.click(await screen.findByText('Accept all'))
    fireEvent.click(screen.getByRole('button', { name: /apply accepted hunks to disk/i }))
    await waitFor(() => expect(api.saveTextFile).toHaveBeenCalledTimes(1))
    // Conflict → the dialog stays open so the user can re-review.
    expect(onClose).not.toHaveBeenCalled()
  })
})
