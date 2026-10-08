// Office Wave 3 C3 — CompanionPage (companion Quick Capture window UI).
//
// The page is deliberately tiny; the tests pin the load-bearing contract:
// Send emits the cross-window event exactly once and only with trimmed
// non-empty text, success acks with `companion.sent` and clears the box,
// emit failures surface `companion.errorSend` without losing the text, and
// the stay-on-top checkbox drives the Rust command (reverting on failure).
// Design 13 adds the window contract (Esc → hide, in the dedicated
// companion webview only), the in-memory capture history (≤5) and the
// draft-only footer hint.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { emitTo } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

const setCompanionAlwaysOnTop = vi.hoisted(() => vi.fn())
const hideCompanionWindow = vi.hoisted(() => vi.fn())
vi.mock('@/lib/tauri-api', async importOriginal => {
  const actual = await importOriginal<typeof tauriApi>()
  return {
    ...actual,
    setCompanionAlwaysOnTop: (...args: unknown[]) =>
      setCompanionAlwaysOnTop(...(args as [boolean])),
    hideCompanionWindow: (...args: unknown[]) => hideCompanionWindow(...(args as [])),
  }
})

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
  emitTo: vi.fn().mockResolvedValue(undefined),
}))

// isCompanionWebview() reads the real window label; jsdom has no Tauri
// internals, so tests pin the label explicitly per block.
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: vi.fn().mockReturnValue({ label: 'main' }),
}))

import CompanionPage from '@/pages/CompanionPage'
import type * as tauriApi from '@/lib/tauri-api'

function typeText(value: string) {
  const input = screen.getByTestId('companion-input') as HTMLTextAreaElement
  fireEvent.change(input, { target: { value } })
  return input
}

function renderAsCompanionWindow() {
  vi.mocked(getCurrentWindow).mockReturnValue({ label: 'companion' } as never)
  render(<CompanionPage />)
}

beforeEach(() => {
  vi.mocked(emitTo).mockClear().mockResolvedValue(undefined)
  vi.mocked(getCurrentWindow).mockClear().mockReturnValue({ label: 'main' } as never)
  setCompanionAlwaysOnTop.mockClear().mockResolvedValue(undefined)
  hideCompanionWindow.mockClear().mockResolvedValue(undefined)
})

describe('CompanionPage', () => {
  it('renders title, labelled input and a disabled-when-empty Send', () => {
    render(<CompanionPage />)
    expect(screen.getByText('Quick capture')).toBeInTheDocument()
    expect(screen.getByLabelText('Capture text')).toBeInTheDocument()
    expect(screen.getByLabelText('Stay on top')).toBeChecked()
    expect(screen.getByTestId('companion-send')).toBeDisabled()
  })

  it('Send emits the frozen companion event once, acks, and clears the input', async () => {
    render(<CompanionPage />)
    typeText('  a thought with padding  ')
    const send = screen.getByTestId('companion-send')
    expect(send).toBeEnabled()
    fireEvent.click(send)

    await waitFor(() =>
      expect(emitTo).toHaveBeenCalledTimes(1),
    )
    expect(emitTo).toHaveBeenCalledWith('main', 'shannon:companion-prompt', {
      text: 'a thought with padding',
    })
    expect(await screen.findByTestId('companion-sent')).toBeInTheDocument()
    expect((screen.getByTestId('companion-input') as HTMLTextAreaElement).value).toBe('')
    expect(screen.getByTestId('companion-send')).toBeDisabled()
  })

  it('whitespace-only input cannot be sent', () => {
    render(<CompanionPage />)
    typeText('   ')
    expect(screen.getByTestId('companion-send')).toBeDisabled()
    expect(emitTo).not.toHaveBeenCalled()
  })

  it('Ctrl+Enter sends from the textarea', async () => {
    render(<CompanionPage />)
    typeText('keyboard capture')
    fireEvent.keyDown(screen.getByTestId('companion-input'), {
      key: 'Enter',
      ctrlKey: true,
    })
    await waitFor(() => expect(emitTo).toHaveBeenCalledTimes(1))
    expect(await screen.findByTestId('companion-sent')).toBeInTheDocument()
  })

  it('shows companion.errorSend and keeps the text when the emit fails', async () => {
    vi.mocked(emitTo).mockRejectedValueOnce(new Error('main window closed'))
    render(<CompanionPage />)
    typeText('precious draft')
    fireEvent.click(screen.getByTestId('companion-send'))

    expect(await screen.findByTestId('companion-error')).toBeInTheDocument()
    expect(screen.getByText("Couldn't reach the main window — it may be closed.")).toBeInTheDocument()
    expect((screen.getByTestId('companion-input') as HTMLTextAreaElement).value).toBe(
      'precious draft',
    )
    // No success line next to the error; typing again clears the error.
    expect(screen.queryByTestId('companion-sent')).not.toBeInTheDocument()
    fireEvent.change(screen.getByTestId('companion-input'), { target: { value: 'x' } })
    expect(screen.queryByTestId('companion-error')).not.toBeInTheDocument()
  })

  it('stay-on-top checkbox drives set_companion_always_on_top', async () => {
    render(<CompanionPage />)
    const checkbox = screen.getByLabelText('Stay on top')
    expect(checkbox).toBeChecked()

    fireEvent.click(checkbox)
    await waitFor(() => expect(setCompanionAlwaysOnTop).toHaveBeenCalledWith(false))
    await waitFor(() => expect(checkbox).not.toBeChecked())

    fireEvent.click(checkbox)
    await waitFor(() => expect(setCompanionAlwaysOnTop).toHaveBeenCalledWith(true))
    await waitFor(() => expect(checkbox).toBeChecked())
  })

  it('reverts the checkbox and surfaces an error when the pin command fails', async () => {
    setCompanionAlwaysOnTop.mockRejectedValueOnce(new Error('window closed'))
    render(<CompanionPage />)
    const checkbox = screen.getByLabelText('Stay on top')
    fireEvent.click(checkbox)

    await waitFor(() => expect(checkbox).toBeChecked(), { timeout: 1000 })
    expect(screen.getByTestId('companion-error')).toBeInTheDocument()
    expect(screen.getByText("Couldn't change the stay-on-top setting.")).toBeInTheDocument()
  }, 5000)
})

// Design 13 window + history contract (parity audit §13 / ruling B5).
describe('CompanionPage — design 13 (Esc, history, footer)', () => {
  beforeEach(() => {
    vi.mocked(emitTo).mockClear().mockResolvedValue(undefined)
    vi.mocked(getCurrentWindow).mockClear().mockReturnValue({ label: 'companion' } as never)
    setCompanionAlwaysOnTop.mockClear().mockResolvedValue(undefined)
    hideCompanionWindow.mockClear().mockResolvedValue(undefined)
  })

  it('Esc hides the window through the Rust command in the companion webview', () => {
    renderAsCompanionWindow()
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(hideCompanionWindow).toHaveBeenCalledTimes(1)
    // Other keys are ignored.
    fireEvent.keyDown(window, { key: 'Enter' })
    expect(hideCompanionWindow).toHaveBeenCalledTimes(1)
  })

  it('Esc is inert outside the dedicated companion webview (main-window fallback)', () => {
    vi.mocked(getCurrentWindow).mockReturnValue({ label: 'main' } as never)
    render(<CompanionPage />)
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(hideCompanionWindow).not.toHaveBeenCalled()
  })

  it('records sent captures in an in-memory history (text + relative time)', async () => {
    renderAsCompanionWindow()
    typeText('tomorrow 10am sync notes')
    fireEvent.click(screen.getByTestId('companion-send'))
    await waitFor(() => expect(screen.getByTestId('companion-history')).toBeInTheDocument())

    const item = screen.getByTestId('companion-history-item')
    expect(item).toHaveTextContent('tomorrow 10am sync notes')
    // Relative timestamp rendered (FormattedRelativeTime output is locale
    // text — just assert something non-empty is there).
    expect(item.textContent!.length).toBeGreaterThan('tomorrow 10am sync notes'.length)
  })

  it('keeps at most 5 captures, newest first, and never persists them', async () => {
    renderAsCompanionWindow()
    for (let i = 1; i <= 7; i++) {
      typeText(`capture ${i}`)
      fireEvent.click(screen.getByTestId('companion-send'))
      // The send promise resolves on the next microtask; wait for the
      // textarea to clear before queueing the next capture.
      await waitFor(() =>
        expect((screen.getByTestId('companion-input') as HTMLTextAreaElement).value).toBe(''),
      )
    }
    const items = screen.getAllByTestId('companion-history-item')
    expect(items).toHaveLength(5)
    // Newest first: the last-sent capture heads the list, the first two
    // were evicted by the cap.
    expect(items[0]).toHaveTextContent('capture 7')
    expect(items[4]).toHaveTextContent('capture 3')
  })

  it('failed sends do not enter the history', async () => {
    vi.mocked(emitTo).mockRejectedValueOnce(new Error('main window closed'))
    renderAsCompanionWindow()
    typeText('precious draft')
    fireEvent.click(screen.getByTestId('companion-send'))
    await waitFor(() => expect(screen.getByTestId('companion-error')).toBeInTheDocument())
    expect(screen.queryByTestId('companion-history')).not.toBeInTheDocument()
    // Retrying after a failure records exactly one entry.
    vi.mocked(emitTo).mockResolvedValueOnce(undefined)
    fireEvent.click(screen.getByTestId('companion-send'))
    await waitFor(() => expect(screen.getAllByTestId('companion-history-item')).toHaveLength(1))
  })

  it('renders the draft-only contract footer', () => {
    renderAsCompanionWindow()
    expect(screen.getByTestId('companion-footer')).toHaveTextContent(
      'Writes to the main window draft only — never auto-sends.',
    )
  })
})
