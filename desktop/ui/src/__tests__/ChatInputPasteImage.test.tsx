// G3b P1-6 — the composer's clipboard-image paste.
//
// 1. An `image/*` clipboard item is persisted via `save_pasted_image` and the
//    returned absolute path enters the normal attachment list (onAttach),
//    so the preflight + send pipeline apply unchanged.
// 2. An oversized image is refused client-side (shared 10 MiB cap) with the
//    i18n message — the backend is never invoked.
// 3. A text-only paste is NOT intercepted: no backend call, default behavior.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { toast } from 'sonner'
import ChatInput from '@/components/chat/ChatInput'
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

const { savePastedImage } = vi.hoisted(() => ({ savePastedImage: vi.fn() }))

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    savePastedImage,
    checkAttachmentPaths: vi.fn().mockResolvedValue([]),
    onWebviewFileDrop: vi.fn(() => Promise.resolve(() => {})),
    registerFileIndexEntry: vi.fn().mockResolvedValue(undefined),
  }
})

vi.mock('sonner', async () => {
  const actual = await vi.importActual<object>('sonner')
  return { ...actual, toast: { ...((actual as { toast: object }).toast as object), error: vi.fn(), success: vi.fn(), warning: vi.fn() } }
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

function pasteWith(items: unknown[]) {
  const textarea = screen.getByRole('textbox', { name: 'Message' })
  fireEvent.paste(textarea, { clipboardData: { items } })
}

const PNG_BYTES = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]

describe('ChatInput — clipboard image paste (G3b P1-6)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    savePastedImage.mockResolvedValue('/home/u/.shannon/cache/pasted/1730000000-deadbeef.png')
  })

  it('persists a pasted image and attaches the returned absolute path', async () => {
    const onAttach = vi.fn()
    renderChatInput({ onAttach })
    pasteWith([
      { kind: 'file', type: 'image/png', getAsFile: () => new File([new Uint8Array(PNG_BYTES)], 'clipboard.png', { type: 'image/png' }) },
    ])
    await waitFor(() => expect(savePastedImage).toHaveBeenCalledTimes(1))
    const [dataBase64, ext] = savePastedImage.mock.calls[0]
    expect(ext).toBe('png')
    // Bare base64 (no data-URL prefix) decoding back to the pasted bytes.
    expect(atob(dataBase64)).toBe(String.fromCharCode(...PNG_BYTES))
    await waitFor(() => expect(onAttach).toHaveBeenCalledWith(['/home/u/.shannon/cache/pasted/1730000000-deadbeef.png']))
  })

  it('an oversized image is refused with the i18n message and never sent to the backend', async () => {
    const onError = vi.mocked(toast.error)
    renderChatInput({ onAttach: vi.fn() })
    const big = new File([new ArrayBuffer(4)], 'big.png', { type: 'image/png' })
    Object.defineProperty(big, 'size', { value: 10 * 1024 * 1024 + 1 })
    pasteWith([{ kind: 'file', type: 'image/png', getAsFile: () => big }])
    await waitFor(() => expect(onError).toHaveBeenCalled())
    expect(savePastedImage).not.toHaveBeenCalled()
  })

  it('a text-only paste is not intercepted (no backend call, no toast)', () => {
    renderChatInput({ onAttach: vi.fn() })
    pasteWith([
      { kind: 'string', type: 'text/plain', getAsFile: () => null },
    ])
    expect(savePastedImage).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('a backend failure toasts the i18n failure title with the cause', async () => {
    savePastedImage.mockRejectedValue(new Error('pasted image data does not match its declared png format'))
    const onError = vi.mocked(toast.error)
    renderChatInput({ onAttach: vi.fn() })
    pasteWith([
      { kind: 'file', type: 'image/png', getAsFile: () => new File([new Uint8Array(PNG_BYTES)], 'clipboard.png', { type: 'image/png' }) },
    ])
    await waitFor(() => expect(onError).toHaveBeenCalled())
    expect(onError.mock.calls[0][0]).toBe('Failed to save pasted image')
    expect(onError.mock.calls[0][1]).toMatchObject({ description: expect.stringContaining('does not match') })
  })
})
