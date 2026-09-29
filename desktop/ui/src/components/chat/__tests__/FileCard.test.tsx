import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { save } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'

import { FileCard } from '../FileCard'

// Per-file tauri-api mock (same pattern as artifact/__tests__/HtmlRenderer):
// the factory replaces the global setup mock for this file, so the three
// wrappers FileCard touches are directly assertable. `save` keeps the global
// setup mock (@tauri-apps/plugin-dialog) — default resolution is null, i.e.
// the user cancels the dialog.
const { openWithDefaultApp, revealInFolder, copyFile } = vi.hoisted(() => ({
  openWithDefaultApp: vi.fn(),
  revealInFolder: vi.fn(),
  copyFile: vi.fn(),
}))

vi.mock('@/lib/tauri-api', () => ({
  openWithDefaultApp,
  revealInFolder,
  copyFile,
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn(), message: vi.fn() },
}))

const mockedSave = vi.mocked(save)

const PROPS = { name: 'report.docx', path: '/tmp/shannon/report.docx', sizeBytes: 2048 }

beforeEach(() => {
  vi.clearAllMocks()
  mockedSave.mockResolvedValue(null)
  openWithDefaultApp.mockResolvedValue(undefined)
  revealInFolder.mockResolvedValue(undefined)
  copyFile.mockResolvedValue(undefined)
})

afterEach(() => {
  cleanup()
})

describe('FileCard', () => {
  it('renders the file name, a human-readable size and the three actions', () => {
    render(<FileCard {...PROPS} />)
    expect(screen.getByText('report.docx')).toBeInTheDocument()
    expect(screen.getByText('2.0 KB')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Open attachment' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Show in folder' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Save as…' })).toBeInTheDocument()
  })

  it('omits the size label when sizeBytes is not provided', () => {
    render(<FileCard name="notes.txt" path="/tmp/notes.txt" />)
    expect(screen.getByText('notes.txt')).toBeInTheDocument()
    expect(screen.queryByText(/KB/)).not.toBeInTheDocument()
  })

  it('open hands the path to openWithDefaultApp', () => {
    render(<FileCard {...PROPS} />)
    fireEvent.click(screen.getByRole('button', { name: 'Open attachment' }))
    expect(openWithDefaultApp).toHaveBeenCalledWith('/tmp/shannon/report.docx')
  })

  it('reveal hands the path to revealInFolder', () => {
    render(<FileCard {...PROPS} />)
    fireEvent.click(screen.getByRole('button', { name: 'Show in folder' }))
    expect(revealInFolder).toHaveBeenCalledWith('/tmp/shannon/report.docx')
  })

  it('save as copies to the chosen destination and toasts success', async () => {
    mockedSave.mockResolvedValueOnce('/Users/ed/Downloads/report-final.docx')
    render(<FileCard {...PROPS} />)
    fireEvent.click(screen.getByRole('button', { name: 'Save as…' }))
    await waitFor(() => {
      expect(copyFile).toHaveBeenCalledWith('/tmp/shannon/report.docx', '/Users/ed/Downloads/report-final.docx')
    })
    await waitFor(() => {
      expect(toast.success).toHaveBeenCalledWith('Saved to /Users/ed/Downloads/report-final.docx')
    })
  })

  it('a cancelled save dialog never copies and stays silent', async () => {
    render(<FileCard {...PROPS} />)
    fireEvent.click(screen.getByRole('button', { name: 'Save as…' }))
    await waitFor(() => expect(mockedSave).toHaveBeenCalledTimes(1))
    expect(copyFile).not.toHaveBeenCalled()
    expect(toast.success).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('a failed copy toasts the failure message with the error cause', async () => {
    copyFile.mockRejectedValueOnce(new Error('disk full'))
    mockedSave.mockResolvedValueOnce('/tmp/dest/report.docx')
    render(<FileCard {...PROPS} />)
    fireEvent.click(screen.getByRole('button', { name: 'Save as…' }))
    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith('Save failed', { description: 'disk full' })
    })
    expect(toast.success).not.toHaveBeenCalled()
  })
})
