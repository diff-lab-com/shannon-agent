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
