// 缓期批 2 — the Settings → 通用 数据 card's「导出全部会话」button.
//
// Pins the dest-picker flow (same shape as the diagnostics export: native
// save dialog → exportAllSessions(dest) → success toast carrying the
// written path), the disabled/working state while the zip is written, the
// cancel no-op, and the honest failure toast (the backend rejects a dest
// whose parent dir does not exist and overwrites an existing one).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { AppProvider } from '@/context/AppContext'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import GeneralPane from '@/pages/settings/GeneralPane'
import * as api from '@/lib/tauri-api'
import { save as saveDialog } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'

vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: vi.fn(),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}))

vi.mock('@/lib/tauri-api', async (importOriginal) => {
  const actual = await importOriginal<object>()
  return {
    ...actual,
    exportAllSessions: vi.fn(),
    configure: vi.fn().mockResolvedValue(undefined),
    // GeneralSettings' feedback card reads the real list command; the core
    // invoke mock resolves undefined and the card expects an array.
    listFeedbackSessions: vi.fn().mockResolvedValue([]),
  }
})

function wrap(ui: React.ReactElement) {
  // Same harness as SettingsPanes.test.tsx — GeneralPane (and the
  // GeneralSettings/SessionSettings it stacks) read the CatalogContext.
  return (
    <AppProvider>
      <MemoryRouter>
        <ArtifactProvider>{ui}</ArtifactProvider>
      </MemoryRouter>
    </AppProvider>
  )
}

beforeEach(() => {
  vi.mocked(saveDialog).mockReset().mockResolvedValue(null)
  vi.mocked(api.exportAllSessions).mockReset()
  vi.mocked(toast.success).mockClear()
  vi.mocked(toast.error).mockClear()
})

describe('GeneralPane 数据 card — export all sessions (缓期批 2)', () => {
  it('renders the real button in place of the retired placeholder badge', async () => {
    render(wrap(<GeneralPane />))
    const button = await screen.findByTestId('export-all-sessions')
    expect(button).toHaveTextContent('Export all sessions')
    expect(button).not.toBeDisabled()
    expect(screen.queryByTestId('data-export-full-badge')).not.toBeInTheDocument()
  })

  it('calls export_all_sessions with the dialog-picked path and toasts it', async () => {
    vi.mocked(saveDialog).mockResolvedValue('/home/tester/exports/shannon-sessions-2026-10-08.zip')
    vi.mocked(api.exportAllSessions).mockResolvedValue({
      path: '/home/tester/exports/shannon-sessions-2026-10-08.zip',
      session_count: 42,
    })
    render(wrap(<GeneralPane />))
    fireEvent.click(await screen.findByTestId('export-all-sessions'))
    await waitFor(() => expect(api.exportAllSessions).toHaveBeenCalledWith(
      '/home/tester/exports/shannon-sessions-2026-10-08.zip',
    ))
    await waitFor(() => expect(toast.success).toHaveBeenCalled())
    expect(String(vi.mocked(toast.success).mock.calls[0][0])).toContain(
      '/home/tester/exports/shannon-sessions-2026-10-08.zip',
    )
  })

  it('shows a working state and disables the button while exporting', async () => {
    let resolveExport: (v: { path: string; session_count: number }) => void = () => {}
    vi.mocked(saveDialog).mockResolvedValue('/tmp/all.zip')
    vi.mocked(api.exportAllSessions).mockImplementation(
      () => new Promise((resolve) => { resolveExport = resolve }),
    )
    render(wrap(<GeneralPane />))
    fireEvent.click(await screen.findByTestId('export-all-sessions'))
    await waitFor(() => {
      expect(screen.getByTestId('export-all-sessions')).toHaveTextContent('Exporting…')
    })
    expect(screen.getByTestId('export-all-sessions')).toBeDisabled()
    resolveExport({ path: '/tmp/all.zip', session_count: 3 })
    await waitFor(() => {
      expect(screen.getByTestId('export-all-sessions')).toHaveTextContent('Export all sessions')
    })
    expect(screen.getByTestId('export-all-sessions')).not.toBeDisabled()
  })

  it('never calls the backend when the user cancels the save dialog', async () => {
    vi.mocked(saveDialog).mockResolvedValue(null)
    render(wrap(<GeneralPane />))
    fireEvent.click(await screen.findByTestId('export-all-sessions'))
    expect(api.exportAllSessions).not.toHaveBeenCalled()
    expect(toast.success).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('toasts the honest failure (e.g. missing parent dir) when the export rejects', async () => {
    vi.mocked(saveDialog).mockResolvedValue('/nonexistent-parent/all.zip')
    vi.mocked(api.exportAllSessions).mockRejectedValue('dest parent directory does not exist')
    render(wrap(<GeneralPane />))
    fireEvent.click(await screen.findByTestId('export-all-sessions'))
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
    expect(toast.success).not.toHaveBeenCalled()
    // The backend's cause surfaces in the toast description.
    expect(String(vi.mocked(toast.error).mock.calls[0][1]?.description)).toContain(
      'dest parent directory does not exist',
    )
  })

  it('toasts the honest failure when the save dialog itself throws', async () => {
    vi.mocked(saveDialog).mockRejectedValue('dialog denied')
    render(wrap(<GeneralPane />))
    fireEvent.click(await screen.findByTestId('export-all-sessions'))
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
    expect(api.exportAllSessions).not.toHaveBeenCalled()
  })
})
