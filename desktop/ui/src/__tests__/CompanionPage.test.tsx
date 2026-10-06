// Office Wave 3 C3 — CompanionPage (companion Quick Capture window UI).
//
// The page is deliberately tiny; the tests pin the load-bearing contract:
// Send emits the cross-window event exactly once and only with trimmed
// non-empty text, success acks with `companion.sent` and clears the box,
// emit failures surface `companion.errorSend` without losing the text, and
// the stay-on-top checkbox drives the Rust command (reverting on failure).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { emitTo } from '@tauri-apps/api/event'

const setCompanionAlwaysOnTop = vi.hoisted(() => vi.fn())
vi.mock('@/lib/tauri-api', async importOriginal => {
  const actual = await importOriginal<typeof tauriApi>()
  return {
    ...actual,
    setCompanionAlwaysOnTop: (...args: unknown[]) =>
      setCompanionAlwaysOnTop(...(args as [boolean])),
  }
})

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
  emitTo: vi.fn().mockResolvedValue(undefined),
}))

import CompanionPage from '@/pages/CompanionPage'
import type * as tauriApi from '@/lib/tauri-api'

function typeText(value: string) {
  const input = screen.getByTestId('companion-input') as HTMLTextAreaElement
  fireEvent.change(input, { target: { value } })
  return input
}

beforeEach(() => {
  vi.mocked(emitTo).mockClear().mockResolvedValue(undefined)
  setCompanionAlwaysOnTop.mockClear().mockResolvedValue(undefined)
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
