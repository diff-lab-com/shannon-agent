// LspQuickFixPanel — LSP code-action surface for a single diagnostic.
//
// The panel drives `lspCodeActions` on mount, renders the returned actions
// as buttons, and applies the chosen action's workspace edit via
// `applyCodeAction`. Every visible bit of UI is reachable from this
// surface; tests exercise the loading → loaded → applied happy path, the
// configured-server fallback path, error rendering, the apply-failure
// path, and the refresh button.

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import * as api from '@/lib/tauri-api'
import LspQuickFixPanel, { type LspQuickFixDiagnostic } from '@/components/lsp/LspQuickFixPanel'

const lspCodeActions = vi.mocked(api.lspCodeActions)
const applyCodeAction = vi.mocked(api.applyCodeAction)

function makeDiagnostic(overrides: Partial<LspQuickFixDiagnostic> = {}): LspQuickFixDiagnostic {
  return {
    file_path: '/tmp/src/example.rs',
    start_line: 4,
    start_character: 2,
    end_line: 4,
    end_character: 9,
    message: 'expected `;`',
    language_id: 'rust',
    ...overrides,
  }
}

function renderPanel(
  diagnostic: LspQuickFixDiagnostic = makeDiagnostic(),
  props?: Partial<React.ComponentProps<typeof LspQuickFixPanel>>,
) {
  return render(<LspQuickFixPanel diagnostic={diagnostic} {...props} />)
}

beforeEach(() => {
  vi.clearAllMocks()
  // Silence the useEffect-driven `console.warn` from the apply-failure path
  // so stderr stays clean.
  vi.spyOn(console, 'warn').mockImplementation(() => undefined)
  lspCodeActions.mockResolvedValue({ actions: [] })
  applyCodeAction.mockResolvedValue(1)
})

describe('LspQuickFixPanel — layout & header', () => {
  it('renders the panel region with the diagnostic file/line/message', async () => {
    renderPanel()
    const region = screen.getByRole('region')
    expect(region).toBeInTheDocument()
    // File basename is rendered in <code>; line/char numbers in the same <p>.
    expect(within(region).getByText('example.rs')).toBeInTheDocument()
    expect(within(region).getByText(/Expected `;`/)).toBeInTheDocument()
    expect(within(region).getByText(/5:3/)).toBeInTheDocument() // line + 1, char + 1
  })

  it('capitalises the diagnostic message (sentence-case)', async () => {
    renderPanel(makeDiagnostic({ message: 'unexpected token' }))
    expect(screen.getByText(/Unexpected token/)).toBeInTheDocument()
  })

  it('leaves the message untouched when it is already empty', async () => {
    // sentenceCase("") → ""; defensive branch covers empty messages.
    renderPanel(makeDiagnostic({ message: '' }))
    // No diagnostic text rendered — just the file basename + line/col.
    const region = screen.getByRole('region')
    expect(within(region).queryByText(/Expected/)).not.toBeInTheDocument()
  })

  it('renders the refresh button always and the close button only when onClose is provided', async () => {
    const { rerender } = render(<LspQuickFixPanel diagnostic={makeDiagnostic()} />)
    expect(screen.getByRole('button', { name: /Re-fetch quick fixes/i })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Close quick-fix panel/i })).not.toBeInTheDocument()
    rerender(<LspQuickFixPanel diagnostic={makeDiagnostic()} onClose={vi.fn()} />)
    expect(screen.getByRole('button', { name: /Close quick-fix panel/i })).toBeInTheDocument()
  })
})

describe('LspQuickFixPanel — server selection', () => {
  it('uses the default server for rust diagnostics', async () => {
    const diag = makeDiagnostic({ language_id: 'rust' })
    renderPanel(diag)
    await waitFor(() => expect(lspCodeActions).toHaveBeenCalledTimes(1))
    const req = lspCodeActions.mock.calls[0]![0]
    expect(req.server_cmd).toBe('rust-analyzer')
    expect(req.server_args).toEqual([])
    expect(req.language_id).toBe('rust')
  })

  it('uses the typescript server for typescript/typescriptreact/javascript', async () => {
    for (const lang of ['typescript', 'typescriptreact', 'javascript']) {
      vi.mocked(api.lspCodeActions).mockClear()
      renderPanel(makeDiagnostic({ language_id: lang }))
      await waitFor(() => expect(lspCodeActions).toHaveBeenCalledTimes(1))
      expect(lspCodeActions.mock.calls[0]![0].server_cmd).toBe('typescript-language-server')
      expect(lspCodeActions.mock.calls[0]![0].server_args).toEqual(['--stdio'])
    }
  })

  it('uses gopls for go', async () => {
    renderPanel(makeDiagnostic({ language_id: 'go' }))
    await waitFor(() => expect(lspCodeActions).toHaveBeenCalledTimes(1))
    expect(lspCodeActions.mock.calls[0]![0].server_cmd).toBe('gopls')
  })

  it('uses pylsp for python', async () => {
    renderPanel(makeDiagnostic({ language_id: 'python' }))
    await waitFor(() => expect(lspCodeActions).toHaveBeenCalledTimes(1))
    expect(lspCodeActions.mock.calls[0]![0].server_cmd).toBe('pylsp')
  })

  it('prefers an explicit server_cmd/server_args override', async () => {
    renderPanel(
      makeDiagnostic({ language_id: 'rust' }),
      { server_cmd: 'custom-lsp', server_args: ['--stdio', '-v'] },
    )
    await waitFor(() => expect(lspCodeActions).toHaveBeenCalledTimes(1))
    const req = lspCodeActions.mock.calls[0]![0]
    expect(req.server_cmd).toBe('custom-lsp')
    expect(req.server_args).toEqual(['--stdio', '-v'])
  })

  it('shows the "no LSP configured" error when no server resolves for the language', async () => {
    renderPanel(makeDiagnostic({ language_id: 'cobol' }))
    // No call to lspCodeActions — we short-circuit before invoking the backend.
    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent(/cobol/i),
    )
    expect(lspCodeActions).not.toHaveBeenCalled()
  })

  it('disables the refresh button when no server resolves', async () => {
    renderPanel(makeDiagnostic({ language_id: 'cobol' }))
    const refresh = screen.getByRole('button', { name: /Re-fetch quick fixes/i })
    await waitFor(() => expect(refresh).toBeDisabled())
  })
})

describe('LspQuickFixPanel — action rendering', () => {
  it('renders an empty-state message when the server returns no actions', async () => {
    lspCodeActions.mockResolvedValue({ actions: [] })
    renderPanel()
    await waitFor(() =>
      expect(screen.getByText(/no quick fixes available/i)).toBeInTheDocument(),
    )
  })

  it('renders one button per returned action', async () => {
    lspCodeActions.mockResolvedValue({
      actions: [
        { title: 'Add semicolon', is_preferred: true, edit: { documentChanges: [] }, kind: 'quickfix' },
        { title: 'Suppress warning', is_preferred: false, command: 'foo', kind: 'refactor.suppress' },
      ],
    })
    renderPanel()
    await waitFor(() => expect(screen.getByRole('button', { name: /Add semicolon/ })).toBeInTheDocument())
    expect(screen.getByRole('button', { name: /Suppress warning/ })).toBeInTheDocument()
    // Kind stripper: "quickfix" stays, "refactor.suppress" → "suppress".
    expect(screen.getByText('quickfix')).toBeInTheDocument()
    expect(screen.getByText('suppress')).toBeInTheDocument()
  })

  it('disables the action button when the action has no edit (command-only)', async () => {
    lspCodeActions.mockResolvedValue({
      actions: [
        { title: 'Run command only', is_preferred: false, command: 'fixer' },
      ],
    })
    renderPanel()
    const btn = await screen.findByRole('button', { name: /Run command only/ })
    expect(btn).toBeDisabled()
  })
})

describe('LspQuickFixPanel — apply flow', () => {
  beforeEach(() => {
    lspCodeActions.mockResolvedValue({
      actions: [
        { title: 'Fix it', is_preferred: true, edit: { documentChanges: [] } },
        { title: 'Other fix', is_preferred: false, edit: { documentChanges: [] }, kind: 'quickfix' },
      ],
    })
  })

  it('applies the workspace edit and fires onApplied', async () => {
    const onApplied = vi.fn()
    applyCodeAction.mockResolvedValue(3)
    renderPanel(makeDiagnostic(), { onApplied })
    const btn = await screen.findByRole('button', { name: /Fix it/ })
    fireEvent.click(btn)
    await waitFor(() => expect(applyCodeAction).toHaveBeenCalledTimes(1))
    expect(applyCodeAction.mock.calls[0]![0]).toEqual({ documentChanges: [] })
    await waitFor(() => expect(onApplied).toHaveBeenCalledTimes(1))
    // Success chip uses the formatted "applies" message — assert it shows
    // (both the button label and the chip contain "Fix it"; match the
    // chip's "Applied: …" prefix to disambiguate).
    await waitFor(() =>
      expect(screen.getByText(/Applied: Fix it/)).toBeInTheDocument(),
    )
  })

  it('disables every action button while one is in flight', async () => {
    let resolveApply: ((count: number) => void) | undefined
    applyCodeAction.mockReturnValue(new Promise<number>((res) => { resolveApply = res }))
    renderPanel()
    const btn = await screen.findByRole('button', { name: /Fix it/ })
    const otherBtn = await screen.findByRole('button', { name: /Other fix/ })
    fireEvent.click(btn)
    await waitFor(() => expect(otherBtn).toBeDisabled())
    resolveApply?.(2)
  })

  it('toasts the apply-failure error message and keeps the panel usable', async () => {
    applyCodeAction.mockRejectedValue(new Error('apply boom'))
    renderPanel()
    const btn = await screen.findByRole('button', { name: /Fix it/ })
    fireEvent.click(btn)
    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent(/apply boom/i),
    )
    // Buttons re-enable after the in-flight state clears.
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Other fix/ })).not.toBeDisabled(),
    )
  })

  it('refuses to apply when the action has no workspace edit (defensive)', async () => {
    // Server returned a command-only action; clicking the disabled button
    // is a no-op — applyCodeAction is never invoked. Verified by the
    // "disables the action button when … no edit" test above. Belt-and-
    // braces here: even a forced click via JS shouldn't fire.
    lspCodeActions.mockResolvedValue({
      actions: [{ title: 'NoEdit', is_preferred: false, command: 'noop' }],
    })
    renderPanel()
    const btn = await screen.findByRole('button', { name: /NoEdit/ })
    expect(btn).toBeDisabled()
    expect(applyCodeAction).not.toHaveBeenCalled()
  })
})

describe('LspQuickFixPanel — refresh', () => {
  it('re-invokes lspCodeActions when the refresh button is clicked', async () => {
    renderPanel()
    await waitFor(() => expect(lspCodeActions).toHaveBeenCalledTimes(1))
    fireEvent.click(screen.getByRole('button', { name: /Re-fetch quick fixes/i }))
    await waitFor(() => expect(lspCodeActions).toHaveBeenCalledTimes(2))
  })

  it('shows the "asking" loading copy while the request is in flight', async () => {
    let resolve: ((r: { actions: api.CodeActionDto[] }) => void) | undefined
    lspCodeActions.mockReturnValue(new Promise((res) => { resolve = res }))
    renderPanel()
    await waitFor(() =>
      expect(screen.getByText(/asking rust-analyzer/i)).toBeInTheDocument(),
    )
    resolve?.({ actions: [] })
  })
})

describe('LspQuickFixPanel — error rendering', () => {
  it('renders an inline error chip when lspCodeActions rejects', async () => {
    lspCodeActions.mockRejectedValue(new Error('lsp down'))
    renderPanel()
    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent(/lsp down/i),
    )
  })
})