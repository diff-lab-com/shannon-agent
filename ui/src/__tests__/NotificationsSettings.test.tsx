import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import NotificationsSettings from '@/components/settings/NotificationsSettings'

const getWebhookConfig = vi.hoisted(() => vi.fn())
const saveWebhookConfig = vi.hoisted(() => vi.fn())
const clearWebhookConfig = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  getWebhookConfig: (...args: unknown[]) => getWebhookConfig(...args),
  saveWebhookConfig: (...args: unknown[]) => saveWebhookConfig(...args),
  clearWebhookConfig: (...args: unknown[]) => clearWebhookConfig(...args),
}))

function wrap(ui: React.ReactElement) {
  return (
    <I18nProvider>
      <MemoryRouter>{ui}</MemoryRouter>
    </I18nProvider>
  )
}

beforeEach(() => {
  getWebhookConfig.mockReset()
  saveWebhookConfig.mockReset()
  clearWebhookConfig.mockReset()
})

describe('NotificationsSettings', () => {
  it('renders loading spinner before config resolves', () => {
    getWebhookConfig.mockReturnValue(new Promise(() => {}))
    render(wrap(<NotificationsSettings />))
    expect(document.querySelector('.animate-spin')).toBeInTheDocument()
  })

  it('renders form with empty defaults when no webhook configured', async () => {
    getWebhookConfig.mockResolvedValue(null)
    render(wrap(<NotificationsSettings />))
    await waitFor(() =>
      expect(screen.getByPlaceholderText('https://hooks.slack.com/services/...')).toBeInTheDocument(),
    )
    expect(screen.getByText('Webhook URL')).toBeInTheDocument()
    expect(screen.getByText('Payload template')).toBeInTheDocument()
    expect(screen.getByText('Save')).toBeInTheDocument()
    expect(screen.getByText('Clear')).toBeInTheDocument()
  })

  it('prefills form from saved config', async () => {
    getWebhookConfig.mockResolvedValue({
      url: 'https://hooks.slack.com/services/abc',
      template: 'slack',
      secret: 'sec',
      timeout_ms: 7000,
      include_body: true,
    })
    render(wrap(<NotificationsSettings />))
    await waitFor(() =>
      expect((screen.getByPlaceholderText('https://hooks.slack.com/services/...') as HTMLInputElement).value).toBe(
        'https://hooks.slack.com/services/abc',
      ),
    )
    expect(
      (screen.getByPlaceholderText('(optional) shared secret for signature verification') as HTMLInputElement).value,
    ).toBe('sec')
  })

  it('blocks save when URL is empty', async () => {
    getWebhookConfig.mockResolvedValue(null)
    saveWebhookConfig.mockResolvedValue(undefined)
    render(wrap(<NotificationsSettings />))
    await waitFor(() =>
      expect(screen.getByPlaceholderText('https://hooks.slack.com/services/...')).toBeInTheDocument(),
    )
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => expect(saveWebhookConfig).not.toHaveBeenCalled())
  })

  it('saves the dto when URL is filled', async () => {
    getWebhookConfig.mockResolvedValue(null)
    saveWebhookConfig.mockResolvedValue(undefined)
    render(wrap(<NotificationsSettings />))
    await waitFor(() =>
      expect(screen.getByPlaceholderText('https://hooks.slack.com/services/...')).toBeInTheDocument(),
    )
    fireEvent.change(screen.getByPlaceholderText('https://hooks.slack.com/services/...'), {
      target: { value: 'https://example.com/hook' },
    })
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => expect(saveWebhookConfig).toHaveBeenCalledTimes(1))
    const dto = saveWebhookConfig.mock.calls[0][0]
    expect(dto.url).toBe('https://example.com/hook')
    expect(dto.template).toBe('raw')
    expect(dto.timeout_ms).toBe(5000)
  })

  it('clears config when Clear is clicked', async () => {
    getWebhookConfig.mockResolvedValue({
      url: 'https://hooks.slack.com/services/abc',
      template: 'slack',
      secret: null,
      timeout_ms: 5000,
      include_body: false,
    })
    clearWebhookConfig.mockResolvedValue(undefined)
    render(wrap(<NotificationsSettings />))
    await waitFor(() =>
      expect((screen.getByPlaceholderText('https://hooks.slack.com/services/...') as HTMLInputElement).value).toBe(
        'https://hooks.slack.com/services/abc',
      ),
    )
    fireEvent.click(screen.getByText('Clear'))
    await waitFor(() => expect(clearWebhookConfig).toHaveBeenCalledTimes(1))
  })
})
