// G3 P0-3 — the composer must surface attachment refusals BEFORE the send.
//
// Two surfaces:
//   1. Chip preflight: at attach time the backend's `check_attachment_paths`
//      verdict flags the chip (warning icon + tooltip) — an out-of-scope or
//      unreadable file is visible while the user is still composing, not a
//      chip that silently vanishes on send.
//   2. No-working-dir banner: with no configured working directory the
//      attachment domain is UNDEFINED (no process-CWD fallback), so every
//      chip is flagged and a banner deep-links to /settings.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ChatInput from '@/components/chat/ChatInput'
import type * as ReactRouterDom from 'react-router-dom'
import type { WebviewFileDropEvent } from '@/lib/tauri-api'
import * as api from '@/lib/tauri-api'

const { dragDrop, navigate } = vi.hoisted(() => ({
  dragDrop: { handler: null as null | ((e: unknown) => void) },
  navigate: vi.fn(),
}))

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: { approval_mode: 'suggest', working_dir: '/tmp' },
    status: { model: 'Claude Sonnet 4.6', provider: 'anthropic', querying: false, message_count: 0, working_dir: '/tmp' },
    models: [
      { id: 'anthropic-claude-sonnet-4-6', name: 'Claude Sonnet 4.6', provider: 'anthropic', context_window: 200000 },
    ],
    refreshConfig: vi.fn(),
    refreshStatus: vi.fn(),
  }),
}))

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    checkAttachmentPaths: vi.fn().mockResolvedValue([]),
    onWebviewFileDrop: vi.fn((handler: (e: unknown) => void) => {
      dragDrop.handler = handler
      return Promise.resolve(() => { dragDrop.handler = null })
    }),
  }
})

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof ReactRouterDom>('react-router-dom')
  return {
    ...actual,
    useOutletContext: () => ({ search: '' }),
    useNavigate: () => navigate,
  }
})

function renderChatInput(props: Partial<React.ComponentProps<typeof ChatInput>> = {}) {
  const defaultProps = {
    value: '',
    onChange: vi.fn(),
    onSend: vi.fn(),
    onExecuteSlash: vi.fn(),
    attachedFiles: [],
    onAttach: vi.fn(),
    onDetachAll: vi.fn(),
    isQuerying: false,
    onCancelQuery: vi.fn(),
    onOpenQuickFix: vi.fn(),
    onOpenEditor: vi.fn(),
  }
  return render(<ChatInput {...defaultProps} {...props} />, { wrapper: I18nProvider })
}

/** Attach via the drag-drop path (the same flow mergePaths serves). */
async function dropPaths(paths: string[], onAttach: ReturnType<typeof vi.fn>) {
  await waitFor(() => expect(dragDrop.handler).toBeTruthy())
  act(() => {
    dragDrop.handler?.({ type: 'drop', paths } as WebviewFileDropEvent)
  })
  expect(onAttach).toHaveBeenCalledWith(paths)
}

describe('ChatInput — attachment preflight flagging (G3 P0-3)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.checkAttachmentPaths).mockResolvedValue([])
    dragDrop.handler = null
  })

  it('flags a chip whose path the preflight would refuse', async () => {
    vi.mocked(api.checkAttachmentPaths).mockResolvedValue([
      { path: '/etc/hosts', ok: false, reason: 'out_of_working_dir' },
      { path: '/tmp/project/notes.txt', ok: true },
    ])
    const onAttach = vi.fn()
    // Start from the accepted file so the chip list renders both.
    renderChatInput({
      attachedFiles: ['/etc/hosts', '/tmp/project/notes.txt'],
      onAttach,
    })
    // The preflight runs on attach — drive the drop that adds these paths.
    await dropPaths(['/etc/hosts', '/tmp/project/notes.txt'], onAttach)

    await waitFor(() => expect(api.checkAttachmentPaths).toHaveBeenCalledWith(['/etc/hosts', '/tmp/project/notes.txt']))
    const warnings = await screen.findAllByTestId('attachment-chip-issue')
    expect(warnings).toHaveLength(1)
    // The tooltip names the reason, not just a bare icon.
    expect(warnings[0]).toHaveAttribute('title', expect.stringContaining('outside the working directory'))
    // The clean chip gets no warning.
    expect(screen.getByText('notes.txt')).toBeInTheDocument()
  })

  it('shows the no-working-dir banner with a /settings deep link when the domain is undefined', async () => {
    vi.mocked(api.checkAttachmentPaths).mockResolvedValue([
      { path: '/home/u/Downloads/report.pdf', ok: false, reason: 'no_working_dir' },
    ])
    const onAttach = vi.fn()
    renderChatInput({ attachedFiles: ['/home/u/Downloads/report.pdf'], onAttach })
    await dropPaths(['/home/u/Downloads/report.pdf'], onAttach)

    expect(await screen.findByTestId('no-working-dir-banner')).toBeInTheDocument()
    fireEvent.click(screen.getByTestId('no-working-dir-open-settings'))
    expect(navigate).toHaveBeenCalledWith('/settings')
  })

  it('leaves clean chips unmarked and drops flags when the chip is removed', async () => {
    const onAttach = vi.fn()
    const { rerender } = renderChatInput({ attachedFiles: ['/tmp/ok.txt'], onAttach })
    vi.mocked(api.checkAttachmentPaths).mockResolvedValue([
      { path: '/tmp/ok.txt', ok: true },
    ])
    await dropPaths(['/tmp/ok.txt'], onAttach)
    await waitFor(() => expect(api.checkAttachmentPaths).toHaveBeenCalled())
    // A clean preflight never renders a warning icon.
    expect(screen.getByText('ok.txt')).toBeInTheDocument()
    await waitFor(() => {
      expect(screen.queryByTestId('attachment-chip-issue')).not.toBeInTheDocument()
    })

    // Parent removes the chip → its flag (if any) must not linger.
    vi.mocked(api.checkAttachmentPaths).mockResolvedValue([
      { path: '/tmp/gone.txt', ok: false, reason: 'unresolvable' },
    ])
    rerender(
      <I18nProvider>
        <ChatInput
          value=""
          onChange={vi.fn()}
          onSend={vi.fn()}
          onExecuteSlash={vi.fn()}
          attachedFiles={['/tmp/gone.txt']}
          onAttach={onAttach}
          onDetachAll={vi.fn()}
          isQuerying={false}
          onCancelQuery={vi.fn()}
          onOpenQuickFix={vi.fn()}
          onOpenEditor={vi.fn()}
        />
      </I18nProvider>,
    )
    await dropPaths(['/tmp/gone.txt'], onAttach)
    await waitFor(() => expect(screen.getAllByTestId('attachment-chip-issue')).toHaveLength(1))
    rerender(
      <I18nProvider>
        <ChatInput
          value=""
          onChange={vi.fn()}
          onSend={vi.fn()}
          onExecuteSlash={vi.fn()}
          attachedFiles={[]}
          onAttach={onAttach}
          onDetachAll={vi.fn()}
          isQuerying={false}
          onCancelQuery={vi.fn()}
          onOpenQuickFix={vi.fn()}
          onOpenEditor={vi.fn()}
        />
      </I18nProvider>,
    )
    await waitFor(() => expect(screen.queryByText('gone.txt')).not.toBeInTheDocument())
    expect(screen.queryByTestId('attachment-chip-issue')).not.toBeInTheDocument()
    expect(screen.queryByTestId('no-working-dir-banner')).not.toBeInTheDocument()
  })

  it('preflight failure is advisory: no marking, no crash', async () => {
    vi.mocked(api.checkAttachmentPaths).mockRejectedValue(new Error('no tauri'))
    const onAttach = vi.fn()
    renderChatInput({ attachedFiles: ['/tmp/ok.txt'], onAttach })
    await dropPaths(['/tmp/ok.txt'], onAttach)
    await waitFor(() => expect(api.checkAttachmentPaths).toHaveBeenCalled())
    expect(screen.queryByTestId('attachment-chip-issue')).not.toBeInTheDocument()
    expect(screen.getByText('ok.txt')).toBeInTheDocument()
  })
})
