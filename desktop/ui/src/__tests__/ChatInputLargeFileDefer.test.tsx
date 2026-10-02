// R7-③ threshold hybrid — large parseable documents come back from the
// `check_attachment_paths` preflight with `deferred_parse` and NO extraction:
// the chip shows the honest "large file · will be parsed when sent"
// placeholder badge instead of pretending the attach-time parse ran. Small
// documents keep the extraction badge; the deferred placeholder is pruned
// with its chip.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, act } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ChatInput from '@/components/chat/ChatInput'
import type * as ReactRouterDom from 'react-router-dom'
import type { WebviewFileDropEvent } from '@/lib/tauri-api'
import * as api from '@/lib/tauri-api'
import type { AttachmentExtractionReport } from '@/types'

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
    registerFileIndexEntry: vi.fn().mockResolvedValue(undefined),
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

async function dropPaths(paths: string[], onAttach: ReturnType<typeof vi.fn>) {
  await waitFor(() => expect(dragDrop.handler).toBeTruthy())
  act(() => {
    dragDrop.handler?.({ type: 'drop', paths } as WebviewFileDropEvent)
  })
  await waitFor(() => expect(onAttach).toHaveBeenCalledWith(paths))
}

const smallReport: AttachmentExtractionReport = {
  path: '/tmp/project/small.pdf',
  kind: 'pdf',
  extracted: true,
  sections_total: 0,
  sections_inlined: 0,
  truncated: false,
}

describe('ChatInput — large-file parse defer placeholder (R7-③)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.checkAttachmentPaths).mockResolvedValue([])
    dragDrop.handler = null
  })

  it('shows the "parsed on send" placeholder for a deferred large document, with no extraction badge', async () => {
    vi.mocked(api.checkAttachmentPaths).mockResolvedValue([
      // The backend defers: ok, marked, and deliberately extraction-less.
      { path: '/tmp/project/big.pdf', ok: true, deferred_parse: true },
    ])
    const onAttach = vi.fn()
    renderChatInput({ attachedFiles: ['/tmp/project/big.pdf'], onAttach })
    await dropPaths(['/tmp/project/big.pdf'], onAttach)

    const badge = await screen.findByTestId('attachment-chip-deferred')
    expect(badge).toHaveAttribute(
      'title',
      expect.stringContaining('parsed when sent'),
    )
    expect(badge).toHaveAttribute('aria-label', expect.stringContaining('parsed when sent'))
    // The extraction badge would be a lie here — no parse has run yet.
    expect(screen.queryByTestId('attachment-chip-extraction')).not.toBeInTheDocument()
    // The chip is not flagged as a refusal — the file WILL be sent.
    expect(screen.queryByTestId('attachment-chip-issue')).not.toBeInTheDocument()
  })

  it('a small document keeps the extraction badge and gets no defer placeholder', async () => {
    vi.mocked(api.checkAttachmentPaths).mockResolvedValue([
      { path: '/tmp/project/small.pdf', ok: true, extraction: smallReport },
    ])
    const onAttach = vi.fn()
    renderChatInput({ attachedFiles: ['/tmp/project/small.pdf'], onAttach })
    await dropPaths(['/tmp/project/small.pdf'], onAttach)

    await screen.findByTestId('attachment-chip-extraction')
    expect(screen.queryByTestId('attachment-chip-deferred')).not.toBeInTheDocument()
  })

  it('the placeholder is pruned when the chip is removed', async () => {
    vi.mocked(api.checkAttachmentPaths).mockResolvedValue([
      { path: '/tmp/project/big.pdf', ok: true, deferred_parse: true },
    ])
    const onAttach = vi.fn()
    const { rerender } = renderChatInput({ attachedFiles: ['/tmp/project/big.pdf'], onAttach })
    await dropPaths(['/tmp/project/big.pdf'], onAttach)
    await screen.findByTestId('attachment-chip-deferred')
    rerender(
      <I18nProvider>
        <ChatInput
          value=""
          onChange={vi.fn()}
          onSend={vi.fn()}
          onExecuteSlash={vi.fn()}
          attachedFiles={[]}
          onAttach={vi.fn()}
          onDetachAll={vi.fn()}
          isQuerying={false}
          onCancelQuery={vi.fn()}
          onOpenQuickFix={vi.fn()}
          onOpenEditor={vi.fn()}
        />
      </I18nProvider>,
    )
    await waitFor(() => expect(screen.queryByTestId('attachment-chip-deferred')).not.toBeInTheDocument())
  })
})
