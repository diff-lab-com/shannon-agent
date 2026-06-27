import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import DiffDialog from '@/components/diff/DiffDialog'
import * as api from '@/lib/tauri-api'

const sampleDiff = {
  old_content: 'line one\nline two',
  new_content: 'line one\nline two edited\nline three',
  file_name: 'src/app.ts',
  language: 'typescript',
}

describe('DiffDialog', () => {
  beforeEach(() => {
    vi.mocked(api.getFileDiff).mockReset()
    vi.mocked(api.saveTextFile).mockReset()
  })

  afterEach(() => {
    cleanup()
  })

  it('renders nothing when closed', () => {
    render(<DiffDialog open={false} filePath="src/app.ts" onClose={() => {}} />)
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('fetches and renders diff when opened', async () => {
    vi.mocked(api.getFileDiff).mockResolvedValue(sampleDiff)
    render(<DiffDialog open={true} filePath="src/app.ts" onClose={() => {}} />)
    await waitFor(() => expect(screen.getByText('src/app.ts')).toBeInTheDocument())
    expect(api.getFileDiff).toHaveBeenCalledWith('src/app.ts')
    expect(screen.getByText('+2')).toBeInTheDocument()
    expect(screen.getByText('−1')).toBeInTheDocument()
  })

  it('shows error message when fetch fails', async () => {
    vi.mocked(api.getFileDiff).mockRejectedValue(new Error('Network down'))
    render(<DiffDialog open={true} filePath="broken.ts" onClose={() => {}} />)
    await waitFor(() => expect(screen.getByText('Failed to load diff')).toBeInTheDocument())
    expect(screen.getByText('Network down')).toBeInTheDocument()
  })

  it('calls onClose when close button clicked', async () => {
    vi.mocked(api.getFileDiff).mockResolvedValue(sampleDiff)
    const onClose = vi.fn()
    render(<DiffDialog open={true} filePath="src/app.ts" onClose={onClose} />)
    await waitFor(() => expect(screen.getByLabelText('Close diff')).toBeInTheDocument())
    fireEvent.click(screen.getByLabelText('Close diff'))
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('calls onClose on Escape key', async () => {
    vi.mocked(api.getFileDiff).mockResolvedValue(sampleDiff)
    const onClose = vi.fn()
    render(<DiffDialog open={true} filePath="src/app.ts" onClose={onClose} />)
    await waitFor(() => expect(screen.getByRole('dialog')).toBeInTheDocument())
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})

describe('DiffDialog — Apply flow (Day 4-5)', () => {
  beforeEach(() => {
    vi.mocked(api.getFileDiff).mockReset()
    vi.mocked(api.saveTextFile).mockReset()
  })

  afterEach(() => {
    cleanup()
  })

  it('disables Apply button until at least one hunk is accepted', async () => {
    vi.mocked(api.getFileDiff).mockResolvedValue(sampleDiff)
    render(<DiffDialog open={true} filePath="src/app.ts" onClose={() => {}} />)
    await waitFor(() => expect(screen.getByRole('dialog')).toBeInTheDocument())
    const applyBtn = await screen.findByRole('button', { name: /apply accepted hunks to disk/i })
    expect(applyBtn).toBeDisabled()
  })

  it('merges and writes on Apply when all hunks accepted', async () => {
    vi.mocked(api.getFileDiff).mockResolvedValue(sampleDiff)
    vi.mocked(api.saveTextFile).mockResolvedValue(undefined)
    const onClose = vi.fn()
    render(<DiffDialog open={true} filePath="src/app.ts" onClose={onClose} />)
    await waitFor(() => expect(screen.getByRole('dialog')).toBeInTheDocument())

    fireEvent.click(await screen.findByText('Accept all'))

    const applyBtn = screen.getByRole('button', { name: /apply accepted hunks to disk/i })
    expect(applyBtn).not.toBeDisabled()
    fireEvent.click(applyBtn)

    await waitFor(() => expect(api.saveTextFile).toHaveBeenCalledTimes(1))
    const [path, content] = vi.mocked(api.saveTextFile).mock.calls[0]
    expect(path).toBe('src/app.ts')
    // All-accept → merged content equals new_content (plus trailing newline).
    expect(content).toBe(sampleDiff.new_content + '\n')
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('keeps Apply disabled when all hunks are rejected (no accepted writes)', async () => {
    vi.mocked(api.getFileDiff).mockResolvedValue(sampleDiff)
    render(<DiffDialog open={true} filePath="src/app.ts" onClose={() => {}} />)
    await waitFor(() => expect(screen.getByRole('dialog')).toBeInTheDocument())

    fireEvent.click(await screen.findByText('Reject all'))

    const applyBtn = screen.getByRole('button', { name: /apply accepted hunks to disk/i })
    expect(applyBtn).toBeDisabled()
  })

  it('toasts and stays open when save fails', async () => {
    vi.mocked(api.getFileDiff).mockResolvedValue(sampleDiff)
    vi.mocked(api.saveTextFile).mockRejectedValue(new Error('Disk full'))
    const onClose = vi.fn()
    render(<DiffDialog open={true} filePath="src/app.ts" onClose={onClose} />)
    await waitFor(() => expect(screen.getByRole('dialog')).toBeInTheDocument())

    fireEvent.click(await screen.findByText('Accept all'))
    fireEvent.click(screen.getByRole('button', { name: /apply accepted hunks to disk/i }))

    await waitFor(() => expect(api.saveTextFile).toHaveBeenCalledTimes(1))
    // Modal stays open on failure.
    expect(onClose).not.toHaveBeenCalled()
    // Apply button re-enables after failure.
    await waitFor(() => {
      const applyBtn = screen.getByRole('button', { name: /apply accepted hunks to disk/i })
      expect(applyBtn).not.toBeDisabled()
    })
  })

  it('Cancel button closes without saving', async () => {
    vi.mocked(api.getFileDiff).mockResolvedValue(sampleDiff)
    const onClose = vi.fn()
    render(<DiffDialog open={true} filePath="src/app.ts" onClose={onClose} />)
    await waitFor(() => expect(screen.getByRole('dialog')).toBeInTheDocument())

    fireEvent.click(await screen.findByText('Cancel'))

    expect(onClose).toHaveBeenCalledTimes(1)
    expect(api.saveTextFile).not.toHaveBeenCalled()
  })
})
