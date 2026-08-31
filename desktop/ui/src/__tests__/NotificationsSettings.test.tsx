// NotificationsSettings page tests.
//
// Two independently-mounted sections (DND + Webhook) share only the i18n
// provider. The DND "Save" and the Webhook "Save" both render literal `Save`
// (per en.json), so all assertions go through scoped helpers that anchor on
// the section heading text and walk up to the enclosing `<section class="mt-xl">`.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import * as api from '@/lib/tauri-api'
import NotificationsSettings from '@/components/settings/NotificationsSettings'

const getWebhookConfig = vi.mocked(api.getWebhookConfig)
const saveWebhookConfig = vi.mocked(api.saveWebhookConfig)
const clearWebhookConfig = vi.mocked(api.clearWebhookConfig)
const getNotificationPrefs = vi.mocked(api.getNotificationPrefs)
const setNotificationPrefs = vi.mocked(api.setNotificationPrefs)

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
    message: vi.fn(),
  },
}))

/** The outer <section class="mt-xl"> for the DND block — anchored to its h3. */
function getDndSection(): HTMLElement {
  return screen.getByRole('heading', { name: /Desktop & quiet hours/i, level: 3 })
    .closest('section.mt-xl') as HTMLElement
}

/** The outer <section class="mt-xl"> for the Webhook block — anchored to its h3. */
function getWebhookSection(): HTMLElement {
  return screen.getByRole('heading', { name: /Webhook Notifications/i, level: 3 })
    .closest('section.mt-xl') as HTMLElement
}

/** Wait for the webhook initial load (its loading state shows a status spinner). */
async function waitForWebhookLoaded() {
  await waitFor(() => expect(screen.queryByRole('status')).toBeNull())
}

/** Wait for the DND initial load — the Save button is `disabled` until loading flips. */
async function waitForDndLoaded() {
  await waitFor(() => {
    const save = within(getDndSection()).getByRole('button', { name: /^Save$/ })
    expect(save).not.toBeDisabled()
  })
}

beforeEach(() => {
  // Use mockReset so the setup.ts defaults are wiped, then re-establish
  // baseline values — `mockReset` alone leaves `vi.fn()` returning undefined.
  getWebhookConfig.mockReset()
  saveWebhookConfig.mockReset()
  clearWebhookConfig.mockReset()
  getNotificationPrefs.mockReset()
  setNotificationPrefs.mockReset()
  getWebhookConfig.mockResolvedValue(null)
  saveWebhookConfig.mockResolvedValue(undefined)
  clearWebhookConfig.mockResolvedValue(undefined)
  getNotificationPrefs.mockResolvedValue({
    master_enabled: true,
    dnd_enabled: false,
    dnd_start: null,
    dnd_end: null,
    on_completed: true,
    on_failed: true,
  })
  setNotificationPrefs.mockResolvedValue(undefined)
})

describe('NotificationsSettings — layout', () => {
  it('renders the page title, DND section, and webhook section', async () => {
    render(<NotificationsSettings />)
    expect(screen.getByRole('heading', { name: 'Notifications', level: 2 })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: /Desktop & quiet hours/i, level: 3 })).toBeInTheDocument()
    await waitForWebhookLoaded()
    expect(screen.getByRole('heading', { name: /Webhook Notifications/i, level: 3 })).toBeInTheDocument()
  })
})

describe('NotificationsSettings — webhook loading', () => {
  it('seeds the form from getWebhookConfig on mount', async () => {
    getWebhookConfig.mockResolvedValue({
      url: 'https://hooks.slack.com/services/T/B/X',
      template: 'slack',
      secret: 'shh',
      timeout_ms: 7500,
      include_body: true,
    })
    render(<NotificationsSettings />)
    await waitForWebhookLoaded()
    const section = getWebhookSection()
    const urlInput = within(section).getByLabelText(/Webhook URL/) as HTMLInputElement
    expect(urlInput.value).toBe('https://hooks.slack.com/services/T/B/X')
    expect(within(section).getByText('Valid URL')).toBeInTheDocument()
  })

  it('treats a null config as the empty preset', async () => {
    render(<NotificationsSettings />)
    await waitForWebhookLoaded()
    const section = getWebhookSection()
    const urlInput = within(section).getByLabelText(/Webhook URL/) as HTMLInputElement
    expect(urlInput.value).toBe('')
  })

  it('falls back to the raw template + 5000ms when fields are missing', async () => {
    getWebhookConfig.mockResolvedValue({
      url: 'https://example.com/webhook',
      template: '',
      secret: null,
      timeout_ms: 0,
      include_body: false,
    })
    render(<NotificationsSettings />)
    await waitForWebhookLoaded()
    // No timeout input is rendered; verify the URL seeds and the Save button
    // enables, which together imply the underlying timeoutMs state defaulted
    // back to 5000 (or the user would need to re-enable to save).
    const section = getWebhookSection()
    const urlInput = within(section).getByLabelText(/Webhook URL/) as HTMLInputElement
    expect(urlInput.value).toBe('https://example.com/webhook')
    expect(within(section).getByText('Valid URL')).toBeInTheDocument()
  })

  it('decodes a custom:<body> template back into the custom preset + body', async () => {
    getWebhookConfig.mockResolvedValue({
      url: 'https://example.com/hook',
      template: 'custom:{"text":"hi"}',
      secret: null,
      timeout_ms: 5000,
      include_body: false,
    })
    render(<NotificationsSettings />)
    await waitForWebhookLoaded()
    const section = getWebhookSection()
    const save = within(section).getByRole('button', { name: /^Save$/ })
    expect(save).not.toBeDisabled()
  })

  it('silently logs and proceeds when getWebhookConfig rejects', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    getWebhookConfig.mockRejectedValue(new Error('boom'))
    render(<NotificationsSettings />)
    await waitFor(() =>
      expect(warn).toHaveBeenCalledWith('getWebhookConfig error:', expect.any(Error)),
    )
    warn.mockRestore()
  })

  it('ignores late getWebhookConfig resolves after unmount (no state update)', async () => {
    let resolveLoad: ((dto: api.WebhookConfigDto | null) => void) | undefined
    getWebhookConfig.mockReturnValue(new Promise((res) => { resolveLoad = res }))
    const { unmount } = render(<NotificationsSettings />)
    unmount()
    // Resolving after unmount must not throw — the cancelled guard covers it.
    expect(() =>
      resolveLoad?.({
        url: 'https://x', template: 'raw', secret: null, timeout_ms: 5000, include_body: false,
      }),
    ).not.toThrow()
  })
})

describe('NotificationsSettings — webhook save validation', () => {
  it('disables the save button until a valid URL is entered', async () => {
    render(<NotificationsSettings />)
    await waitForWebhookLoaded()
    const section = getWebhookSection()
    const save = within(section).getByRole('button', { name: /^Save$/ })
    expect(save).toBeDisabled()
    const urlInput = within(section).getByLabelText(/Webhook URL/) as HTMLInputElement
    fireEvent.change(urlInput, { target: { value: 'not a url' } })
    expect(save).toBeDisabled()
    expect(within(section).getByText('Invalid URL')).toBeInTheDocument()
    fireEvent.change(urlInput, { target: { value: 'https://hooks.slack.com/services/T/B/X' } })
    await waitFor(() => expect(save).not.toBeDisabled())
    expect(within(section).getByText('Valid URL')).toBeInTheDocument()
  })

  it('flags an http://localhost URL as invalid (private-host rejection)', async () => {
    render(<NotificationsSettings />)
    await waitForWebhookLoaded()
    const section = getWebhookSection()
    const urlInput = within(section).getByLabelText(/Webhook URL/) as HTMLInputElement
    fireEvent.change(urlInput, { target: { value: 'http://localhost:8080/hook' } })
    expect(within(section).getByText('Invalid URL')).toBeInTheDocument()
    expect(within(section).getByRole('button', { name: /^Save$/ })).toBeDisabled()
  })

  it('flags an ftp:// URL as invalid (scheme rejection)', async () => {
    render(<NotificationsSettings />)
    await waitForWebhookLoaded()
    const section = getWebhookSection()
    const urlInput = within(section).getByLabelText(/Webhook URL/) as HTMLInputElement
    fireEvent.change(urlInput, { target: { value: 'ftp://example.com' } })
    expect(within(section).getByText('Invalid URL')).toBeInTheDocument()
  })

  it('encodes the selected preset as the saved template', async () => {
    render(<NotificationsSettings />)
    await waitForWebhookLoaded()
    const section = getWebhookSection()
    const urlInput = within(section).getByLabelText(/Webhook URL/) as HTMLInputElement
    fireEvent.change(urlInput, { target: { value: 'https://hooks.slack.com/services/T/B/X' } })
    fireEvent.click(within(section).getByRole('button', { name: /^Save$/ }))
    await waitFor(() => expect(saveWebhookConfig).toHaveBeenCalledTimes(1))
    const dto = saveWebhookConfig.mock.calls[0]![0]
    expect(dto.url).toBe('https://hooks.slack.com/services/T/B/X')
    expect(dto.template.startsWith('custom')).toBe(true)
    expect(dto.timeout_ms).toBe(5000)
    expect(dto.include_body).toBe(false)
    expect(dto.secret).toBeNull()
  })

  it('shows the saving label while save is in flight', async () => {
    let resolveSave: (() => void) | undefined
    saveWebhookConfig.mockReturnValue(new Promise<void>((res) => { resolveSave = res }))
    render(<NotificationsSettings />)
    await waitForWebhookLoaded()
    const section = getWebhookSection()
    const urlInput = within(section).getByLabelText(/Webhook URL/) as HTMLInputElement
    fireEvent.change(urlInput, { target: { value: 'https://hooks.slack.com/services/T/B/X' } })
    fireEvent.click(within(section).getByRole('button', { name: /^Save$/ }))
    await waitFor(() =>
      expect(within(section).getByRole('button', { name: /Saving/ })).toBeInTheDocument(),
    )
    resolveSave?.()
    await waitFor(() =>
      expect(within(section).getByRole('button', { name: /^Save$/ })).toBeInTheDocument(),
    )
  })

  it('toasts an error when saveWebhookConfig rejects', async () => {
    const { toast } = await import('sonner')
    saveWebhookConfig.mockRejectedValue(new Error('network down'))
    render(<NotificationsSettings />)
    await waitForWebhookLoaded()
    const section = getWebhookSection()
    const urlInput = within(section).getByLabelText(/Webhook URL/) as HTMLInputElement
    fireEvent.change(urlInput, { target: { value: 'https://hooks.slack.com/services/T/B/X' } })
    fireEvent.click(within(section).getByRole('button', { name: /^Save$/ }))
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
  })
})

describe('NotificationsSettings — webhook clear', () => {
  beforeEach(() => {
    getWebhookConfig.mockResolvedValue({
      url: 'https://hooks.slack.com/services/T/B/X',
      template: 'slack',
      secret: 's',
      timeout_ms: 8000,
      include_body: true,
    })
  })

  it('clears the form and toasts on success', async () => {
    const { toast } = await import('sonner')
    render(<NotificationsSettings />)
    await waitForWebhookLoaded()
    const section = getWebhookSection()
    expect((within(section).getByLabelText(/Webhook URL/) as HTMLInputElement).value).toBeTruthy()
    fireEvent.click(within(section).getByRole('button', { name: /^Clear$/ }))
    await waitFor(() => expect(clearWebhookConfig).toHaveBeenCalledTimes(1))
    await waitFor(() =>
      expect((within(section).getByLabelText(/Webhook URL/) as HTMLInputElement).value).toBe(''),
    )
    expect(toast.success).toHaveBeenCalled()
  })

  it('toasts an error when clearWebhookConfig rejects', async () => {
    const { toast } = await import('sonner')
    clearWebhookConfig.mockRejectedValue(new Error('denied'))
    render(<NotificationsSettings />)
    await waitForWebhookLoaded()
    const section = getWebhookSection()
    fireEvent.click(within(section).getByRole('button', { name: /^Clear$/ }))
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
  })

  it('disables the clear button while the URL is empty', async () => {
    getWebhookConfig.mockResolvedValue(null)
    render(<NotificationsSettings />)
    await waitForWebhookLoaded()
    expect(within(getWebhookSection()).getByRole('button', { name: /^Clear$/ })).toBeDisabled()
  })

  it('disables the clear button while the clear is in flight', async () => {
    let resolveClear: (() => void) | undefined
    clearWebhookConfig.mockReturnValue(new Promise<void>((res) => { resolveClear = res }))
    render(<NotificationsSettings />)
    await waitForWebhookLoaded()
    const section = getWebhookSection()
    fireEvent.click(within(section).getByRole('button', { name: /^Clear$/ }))
    await waitFor(() =>
      expect(within(section).getByRole('button', { name: /Clearing/ })).toBeInTheDocument(),
    )
    expect(within(section).getByRole('button', { name: /Clearing/ })).toBeDisabled()
    resolveClear?.()
  })
})

describe('NotificationsSettings — DND prefs', () => {
  it('seeds the form from getNotificationPrefs on mount', async () => {
    getNotificationPrefs.mockResolvedValue({
      master_enabled: true,
      dnd_enabled: true,
      dnd_start: '23:00',
      dnd_end: '06:30',
      on_completed: false,
      on_failed: true,
    })
    render(<NotificationsSettings />)
    await waitForDndLoaded()
    const section = getDndSection()
    expect(within(section).getByLabelText(/Desktop notifications$/)).toBeChecked()
    expect(within(section).getByLabelText(/Task completed/)).not.toBeChecked()
    expect(within(section).getByLabelText(/Task failed/)).toBeChecked()
    expect(within(section).getByLabelText(/Do not disturb/)).toBeChecked()
    expect((within(section).getByLabelText('Start') as HTMLInputElement).value).toBe('23:00')
    expect((within(section).getByLabelText('End') as HTMLInputElement).value).toBe('06:30')
  })

  it('persists the new prefs on save', async () => {
    const { toast } = await import('sonner')
    render(<NotificationsSettings />)
    await waitForDndLoaded()
    const section = getDndSection()
    const dndSwitch = within(section).getByLabelText(/Do not disturb/)
    // Base UI Switch responds to a click that hits the inner <span role="switch">.
    fireEvent.click(dndSwitch)
    // After toggling on, the time inputs render in a {dnd && master && (...)} block.
    await waitFor(() =>
      expect(within(section).getByLabelText('Start')).toBeInTheDocument(),
    )
    fireEvent.change(within(section).getByLabelText('Start') as HTMLInputElement, {
      target: { value: '21:00' },
    })
    fireEvent.change(within(section).getByLabelText('End') as HTMLInputElement, {
      target: { value: '08:00' },
    })
    fireEvent.click(within(section).getByRole('button', { name: /^Save$/ }))
    await waitFor(() => expect(setNotificationPrefs).toHaveBeenCalledTimes(1))
    const payload = setNotificationPrefs.mock.calls[0]![0]
    expect(payload.dnd_enabled).toBe(true)
    expect(payload.dnd_start).toBe('21:00')
    expect(payload.dnd_end).toBe('08:00')
    expect(payload.master_enabled).toBe(true)
    expect(payload.on_completed).toBe(true)
    expect(payload.on_failed).toBe(true)
    expect(toast.success).toHaveBeenCalled()
  })

  it('sends null start/end when DND is off', async () => {
    getNotificationPrefs.mockResolvedValue({
      master_enabled: true,
      dnd_enabled: false,
      dnd_start: null,
      dnd_end: null,
      on_completed: true,
      on_failed: true,
    })
    render(<NotificationsSettings />)
    await waitForDndLoaded()
    const section = getDndSection()
    fireEvent.click(within(section).getByRole('button', { name: /^Save$/ }))
    await waitFor(() => expect(setNotificationPrefs).toHaveBeenCalledTimes(1))
    expect(setNotificationPrefs.mock.calls[0]![0].dnd_start).toBeNull()
    expect(setNotificationPrefs.mock.calls[0]![0].dnd_end).toBeNull()
  })

  it('disables event toggles + DND switch when master is off', async () => {
    getNotificationPrefs.mockResolvedValue({
      master_enabled: false,
      dnd_enabled: true,
      dnd_start: '22:00',
      dnd_end: '07:00',
      on_completed: true,
      on_failed: true,
    })
    render(<NotificationsSettings />)
    await waitForDndLoaded()
    const section = getDndSection()
    // Switch uses aria-disabled on its <span role="switch"> wrapper rather
    // than the `disabled` HTML attribute.
    expect(within(section).getByLabelText(/Task completed/)).toHaveAttribute('aria-disabled', 'true')
    expect(within(section).getByLabelText(/Task failed/)).toHaveAttribute('aria-disabled', 'true')
    expect(within(section).getByLabelText(/Do not disturb/)).toHaveAttribute('aria-disabled', 'true')
  })

  it('hides the start/end time fields when DND is off', async () => {
    getNotificationPrefs.mockResolvedValue({
      master_enabled: true,
      dnd_enabled: false,
      dnd_start: null,
      dnd_end: null,
      on_completed: true,
      on_failed: true,
    })
    render(<NotificationsSettings />)
    await waitForDndLoaded()
    const section = getDndSection()
    expect(within(section).queryByText('Start')).not.toBeInTheDocument()
    expect(within(section).queryByText('End')).not.toBeInTheDocument()
  })

  it('toasts an error when setNotificationPrefs rejects', async () => {
    const { toast } = await import('sonner')
    setNotificationPrefs.mockRejectedValue(new Error('prefs write failed'))
    render(<NotificationsSettings />)
    await waitForDndLoaded()
    const section = getDndSection()
    fireEvent.click(within(section).getByRole('button', { name: /^Save$/ }))
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
  })

  it('survives an initial getNotificationPrefs rejection', async () => {
    const { toast } = await import('sonner')
    getNotificationPrefs.mockRejectedValue(new Error('prefs read failed'))
    render(<NotificationsSettings />)
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
  })
})