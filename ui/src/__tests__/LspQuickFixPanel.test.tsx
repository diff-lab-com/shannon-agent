// LspQuickFixPanel tests.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import LspQuickFixPanel from '@/components/lsp/LspQuickFixPanel'
import type { LspQuickFixDiagnostic } from '@/components/lsp/LspQuickFixPanel'

const lspCodeActions = vi.hoisted(() => vi.fn())
const applyCodeAction = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  lspCodeActions: (...args: unknown[]) => lspCodeActions(...args),
  applyCodeAction: (...args: unknown[]) => applyCodeAction(...args),
}))

const diag: LspQuickFixDiagnostic = {
  file_path: '/tmp/src/lib.rs',
  start_line: 12,
  start_character: 4,
  end_line: 12,
  end_character: 8,
  message: 'unused variable: `x`',
  language_id: 'rust',
}

beforeEach(() => {
  lspCodeActions.mockReset()
  applyCodeAction.mockReset()
  lspCodeActions.mockResolvedValue({ actions: [] })
  applyCodeAction.mockResolvedValue(1)
})

describe('LspQuickFixPanel', () => {
  it('spawns the rust-analyzer server for rust language', async () => {
    render(<LspQuickFixPanel diagnostic={diag} />)
    await waitFor(() => expect(lspCodeActions).toHaveBeenCalled())
    expect(lspCodeActions).toHaveBeenCalledWith(expect.objectContaining({
      server_cmd: 'rust-analyzer',
      server_args: [],
      language_id: 'rust',
    }))
  })

  it('shows diagnostic file name, line, and message', async () => {
    render(<LspQuickFixPanel diagnostic={diag} />)
    await waitFor(() => expect(lspCodeActions).toHaveBeenCalled())
    expect(screen.getByText(/lib\.rs/)).toBeInTheDocument()
    expect(screen.getByText(/unused variable: `x`/)).toBeInTheDocument()
    expect(screen.getByText(/:13:5/)).toBeInTheDocument()
  })

  it('renders an action button per returned action', async () => {
    lspCodeActions.mockResolvedValue({
      actions: [
        { title: 'Prefix with _', kind: 'quickfix.prefix', is_preferred: true, edit: { changes: {} } },
        { title: 'Remove variable', kind: 'quickfix.remove', is_preferred: false, edit: null },
      ],
    })
    render(<LspQuickFixPanel diagnostic={diag} />)
    expect(await screen.findByText('Prefix with _')).toBeInTheDocument()
    expect(screen.getByText('Remove variable')).toBeInTheDocument()
  })

  it('applies a chosen action and shows confirmation', async () => {
    const onApplied = vi.fn()
    lspCodeActions.mockResolvedValue({
      actions: [
        { title: 'Prefix with _', kind: 'quickfix', is_preferred: true, edit: { changes: { 'file:///tmp/src/lib.rs': [] } } },
      ],
    })
    applyCodeAction.mockResolvedValue(2)
    render(<LspQuickFixPanel diagnostic={diag} onApplied={onApplied} />)
    const button = await screen.findByText('Prefix with _')
    fireEvent.click(button)
    await waitFor(() => expect(applyCodeAction).toHaveBeenCalledTimes(1))
    expect(onApplied).toHaveBeenCalled()
    expect(await screen.findByText(/Applied: Prefix with _ \(2 edits\)/)).toBeInTheDocument()
  })

  it('shows error when applyCodeAction rejects', async () => {
    lspCodeActions.mockResolvedValue({
      actions: [
        { title: 'Prefix with _', is_preferred: false, edit: { changes: {} } },
      ],
    })
    applyCodeAction.mockRejectedValue('disk full')
    render(<LspQuickFixPanel diagnostic={diag} />)
    fireEvent.click(await screen.findByText('Prefix with _'))
    expect(await screen.findByText(/disk full/)).toBeInTheDocument()
  })

  it('shows error when fetch rejects', async () => {
    lspCodeActions.mockRejectedValue('rust-analyzer not installed')
    render(<LspQuickFixPanel diagnostic={diag} />)
    expect(await screen.findByText(/rust-analyzer not installed/)).toBeInTheDocument()
  })

  it('shows empty state when no actions returned', async () => {
    lspCodeActions.mockResolvedValue({ actions: [] })
    render(<LspQuickFixPanel diagnostic={diag} />)
    expect(await screen.findByText(/No quick fixes available/i)).toBeInTheDocument()
  })

  it('shows error when language has no default server', async () => {
    render(
      <LspQuickFixPanel
        diagnostic={{ ...diag, language_id: 'cobol' }}
      />,
    )
    expect(await screen.findByText(/No LSP server configured/i)).toBeInTheDocument()
  })

  it('uses custom server_cmd prop when provided', async () => {
    render(<LspQuickFixPanel diagnostic={diag} server_cmd="custom-lsp" server_args={["--stdio"]} />)
    await waitFor(() => expect(lspCodeActions).toHaveBeenCalled())
    expect(lspCodeActions).toHaveBeenCalledWith(expect.objectContaining({
      server_cmd: 'custom-lsp',
      server_args: ['--stdio'],
    }))
  })

  it('refresh button re-fetches actions', async () => {
    render(<LspQuickFixPanel diagnostic={diag} />)
    await waitFor(() => expect(lspCodeActions).toHaveBeenCalledTimes(1))
    const refresh = screen.getByLabelText('Re-fetch quick fixes') as HTMLButtonElement
    await waitFor(() => expect(refresh.disabled).toBe(false))
    fireEvent.click(refresh)
    await waitFor(() => expect(lspCodeActions).toHaveBeenCalledTimes(2))
  })

  it('close button fires onClose', async () => {
    const onClose = vi.fn()
    render(<LspQuickFixPanel diagnostic={diag} onClose={onClose} />)
    await waitFor(() => expect(lspCodeActions).toHaveBeenCalled())
    fireEvent.click(screen.getByLabelText('Close quick-fix panel'))
    expect(onClose).toHaveBeenCalled()
  })
})
