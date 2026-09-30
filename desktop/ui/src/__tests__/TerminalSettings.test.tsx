// Tests for the P3-1 Terminal settings card (Settings → Advanced):
// effective-value rendering after load/save, and the load-failure safety
// guard — a failed `terminal_get_settings` must never leave the card in a
// state where Save can fire from empty inputs. Sending Number('') = 0 for
// untouched numerics would let the backend clamp them onto the minimums
// and silently clobber the stored [terminal] config with a success toast.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { ReactElement } from 'react'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { TerminalSettings } from '@/components/settings/TerminalSettings'
import { I18nProvider } from '@/i18n'
import * as api from '@/lib/tauri-api'
import type { TerminalSettings } from '@/types'

const DEFAULTS: TerminalSettings = {
  shell: null, fontSize: 12, scrollback: 5000, drawerHeight: 320, screenReaderMode: false,
}

function wrap(ui: ReactElement) {
  return <I18nProvider>{ui}</I18nProvider>
}

const saveButton = () => screen.getByRole('button', { name: 'Save terminal settings' }) as HTMLButtonElement

describe('TerminalSettings card (P3-1 load-failure safety + effective values)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.terminalGetSettings).mockResolvedValue({ ...DEFAULTS })
    vi.mocked(api.terminalSetSettings).mockImplementation((settings) => Promise.resolve(settings))
  })

  it('shows the effective values and enables Save once a load succeeds', async () => {
    render(wrap(<TerminalSettings />))
    expect(await screen.findByDisplayValue('5000')).toBeTruthy()
    expect(screen.getByDisplayValue('12')).toBeTruthy()
    expect(screen.getByDisplayValue('320')).toBeTruthy()
    expect(saveButton().disabled).toBe(false)
  })

  it('sends the raw edit and renders the clamped effective values from the response', async () => {
    vi.mocked(api.terminalSetSettings).mockResolvedValue({
      shell: null, fontSize: 12, scrollback: 32000, drawerHeight: 320, screenReaderMode: false,
    })
    render(wrap(<TerminalSettings />))
    const scrollback = await screen.findByDisplayValue('5000')
    fireEvent.change(scrollback, { target: { value: '999999' } })
    fireEvent.click(saveButton())
    // The RAW value goes out (the backend clamps)…
    await waitFor(() => expect(api.terminalSetSettings).toHaveBeenCalledWith(
      expect.objectContaining({ scrollback: 999999 }),
    ))
    // …and the EFFECTIVE value from the set-response is what renders.
    expect(await screen.findByDisplayValue('32000')).toBeTruthy()
  })

  it('never lets a failed load springboard a save from empty inputs', async () => {
    vi.mocked(api.terminalGetSettings).mockRejectedValueOnce(new Error('config unreadable'))
    render(wrap(<TerminalSettings />))

    // Explicit error state (alert + retry) instead of a live empty form.
    expect(await screen.findByRole('alert')).toBeTruthy()

    // Every field is dead — there is no editable-springboard state.
    for (const label of ['Default shell', 'Font size (8–32)', 'Scrollback lines (0–100000)', 'Drawer height in px (120–1200)']) {
      expect((screen.getByLabelText(label) as HTMLInputElement).disabled).toBe(true)
    }
    // Base UI renders the switch as a non-button: disabled shows up as
    // aria-disabled (its click handlers self-guard on the flag).
    expect(screen.getByRole('switch', { name: 'Screen reader mode' }).getAttribute('aria-disabled')).toBe('true')
    expect(saveButton().disabled).toBe(true)

    // Hammering every control the card still shows cannot reach the setter.
    fireEvent.click(saveButton())
    fireEvent.click(screen.getByRole('switch', { name: 'Screen reader mode' }))
    fireEvent.change(screen.getByLabelText('Scrollback lines (0–100000)'), { target: { value: '42' } })
    await new Promise((r) => setTimeout(r, 25))
    expect(api.terminalSetSettings).not.toHaveBeenCalled()
  })

  it('recovers through Retry: a successful retry loads values and re-enables Save', async () => {
    vi.mocked(api.terminalGetSettings)
      .mockRejectedValueOnce(new Error('config unreadable'))
      .mockResolvedValueOnce({
        shell: '/bin/zsh', fontSize: 15, scrollback: 1234, drawerHeight: 480, screenReaderMode: true,
      })
    render(wrap(<TerminalSettings />))
    await screen.findByRole('alert')
    expect(saveButton().disabled).toBe(true)

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))

    expect(await screen.findByDisplayValue('1234')).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
    expect(saveButton().disabled).toBe(false)
  })
})
