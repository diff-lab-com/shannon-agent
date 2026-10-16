// H2 P1-4 — long-text paste converts to a .txt attachment.
//
// 1. Text at/over MAX_INLINE_PASTE_CHARS (8192) is persisted via
//    `save_pasted_text`, the returned path rides the normal attachment
//    pipeline, the textarea receives nothing, and a sonner toast carries an
//    explicit Undo action (put the text back + drop the chip).
// 2. Below the threshold the paste is never intercepted — the default
//    insertion stays alive (no preventDefault, no backend call).
// 3. A backend failure surfaces as a toast error; nothing attaches.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, createEvent, waitFor } from '@testing-library/react'
import { toast } from 'sonner'
import ChatInput from '@/components/chat/ChatInput'
import { MAX_INLINE_PASTE_CHARS } from '@/components/chat/ChatInput'
import type * as ReactRouterDom from 'react-router-dom'

const { navigate } = vi.hoisted(() => ({ navigate: vi.fn() }))

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

const { savePastedText } = vi.hoisted(() => ({ savePastedText: vi.fn() }))

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    savePastedText,
    checkAttachmentPaths: vi.fn().mockResolvedValue([]),
    onWebviewFileDrop: vi.fn(() => Promise.resolve(() => {})),
    registerFileIndexEntry: vi.fn().mockResolvedValue(undefined),
  }
})

vi.mock('sonner', async () => {
  const actual = await vi.importActual<object>('sonner')
  return {
    ...actual,
    toast: {
      ...((actual as { toast: object }).toast as object),
      error: vi.fn(),
      success: vi.fn(),
      warning: vi.fn(),
      info: vi.fn(),
    },
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
    attachedFiles: [] as string[],
    onAttach: vi.fn(),
    onDetachAll: vi.fn(),
    isQuerying: false,
    onCancelQuery: vi.fn(),
    onOpenQuickFix: vi.fn(),
    onOpenEditor: vi.fn(),
  }
  return render(<ChatInput {...defaultProps} {...props} />)
}

/** Fires a paste with `text` as the plain-text payload; preventDefault observable. */
function pasteText(text: string) {
  const textarea = screen.getByRole('textbox', { name: 'Message' })
  const event = createEvent.paste(textarea, {
    clipboardData: { items: [], getData: (type: string) => (type === 'text/plain' ? text : '') },
  })
  const preventDefault = vi.spyOn(event, 'preventDefault')
  fireEvent(textarea, event)
  return preventDefault
}

const LONG = 'x'.repeat(9000)

describe('ChatInput — long-text paste to attachment (H2 P1-4)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    savePastedText.mockResolvedValue('/home/u/.shannon/cache/pasted/1730000000-cafebabe.txt')
  })

  it('converts an over-threshold paste: backend save, no insertion, toast with Undo', async () => {
    const onChange = vi.fn()
    const onAttach = vi.fn()
    renderChatInput({ onChange, onAttach })
    const preventDefault = pasteText(LONG)

    // Default insertion is cancelled — the textarea never receives the dump.
    expect(preventDefault).toHaveBeenCalled()
    expect(onChange).not.toHaveBeenCalled()
    await waitFor(() => expect(savePastedText).toHaveBeenCalledWith(LONG))
    await waitFor(() => expect(onAttach).toHaveBeenCalledWith(['/home/u/.shannon/cache/pasted/1730000000-cafebabe.txt']))
    expect(toast.success).toHaveBeenCalledTimes(1)
    const [message, options] = vi.mocked(toast.success).mock.calls[0]
    expect(message).toBe('Long text converted to attachment')
    expect((options as { action: { label: string; onClick: () => void } }).action.label).toBe('Undo')
  })

  it('the toast Undo puts the text back and removes the attachment', async () => {
    const onChange = vi.fn()
    const onAttach = vi.fn()
    const attached = ['/home/u/.shannon/cache/pasted/1730000000-cafebabe.txt']
    renderChatInput({ onChange, onAttach, attachedFiles: attached, value: 'kept draft' })
    pasteText(LONG)
    await waitFor(() => expect(toast.success).toHaveBeenCalledTimes(1))

    const { action } = vi.mocked(toast.success).mock.calls[0][1] as {
      action: { label: string; onClick: () => void }
    }
    action.onClick()

    // The chip set comes back WITHOUT the converted paste (replace semantics)…
    expect(onAttach).toHaveBeenLastCalledWith([])
    // …and the composer text is restored: the pre-existing draft plus the
    // pasted text, blank-line separated.
    expect(onChange).toHaveBeenLastCalledWith(expect.stringContaining('kept draft'))
    expect(onChange).toHaveBeenLastCalledWith(expect.stringContaining(LONG))
  })

  it('below the threshold the paste falls through untouched', () => {
    const onChange = vi.fn()
    renderChatInput({ onChange })
    const preventDefault = pasteText('a reasonable snippet')
    expect(preventDefault).not.toHaveBeenCalled()
    expect(savePastedText).not.toHaveBeenCalled()
    expect(toast.success).not.toHaveBeenCalled()
  })

  it('the threshold itself converts (8192 in, 8191 out)', () => {
    renderChatInput()
    pasteText('a'.repeat(MAX_INLINE_PASTE_CHARS))
    expect(savePastedText).toHaveBeenCalledTimes(1)
    pasteText('a'.repeat(MAX_INLINE_PASTE_CHARS - 1))
    expect(savePastedText).toHaveBeenCalledTimes(1)
  })

  it('a backend failure toasts the error and attaches nothing', async () => {
    savePastedText.mockRejectedValue(new Error('cache dir unwritable'))
    const onAttach = vi.fn()
    renderChatInput({ onAttach })
    pasteText(LONG)
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
    expect(onAttach).not.toHaveBeenCalled()
  })
})
