import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import NotificationsSettings from '@/components/settings/NotificationsSettings'

const getWebhookConfig = vi.hoisted(() => vi.fn())
const saveWebhookConfig = vi.hoisted(() => vi.fn())
const clearWebhookConfig = vi.hoisted(() => vi.fn())
const getInboundConfig = vi.hoisted(() => vi.fn())
const saveInboundConfig = vi.hoisted(() => vi.fn())
const clearInboundConfig = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  getWebhookConfig: (...args: unknown[]) => getWebhookConfig(...args),
  saveWebhookConfig: (...args: unknown[]) => saveWebhookConfig(...args),
  clearWebhookConfig: (...args: unknown[]) => clearWebhookConfig(...args),
  getInboundConfig: (...args: unknown[]) => getInboundConfig(...args),
  saveInboundConfig: (...args: unknown[]) => saveInboundConfig(...args),
  clearInboundConfig: (...args: unknown[]) => clearInboundConfig(...args),
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
  getInboundConfig.mockReset()
  saveInboundConfig.mockReset()
  clearInboundConfig.mockReset()
  getInboundConfig.mockResolvedValue({})
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

  describe('InboundSection', () => {
    beforeEach(() => {
      getWebhookConfig.mockResolvedValue(null)
    })

    it('renders Slack and Telegram inbound sections with inputs', async () => {
      render(wrap(<NotificationsSettings />))
      await waitFor(() => expect(screen.getByPlaceholderText('xoxb-...')).toBeInTheDocument())
      expect(screen.getByPlaceholderText('123456789:ABC...')).toBeInTheDocument()
      expect(screen.getByPlaceholderText('C012345, C678901')).toBeInTheDocument()
      expect(screen.getByPlaceholderText('-1001234567890, 123456789')).toBeInTheDocument()
      expect(screen.getByText('Inbound Messages (Phase 1)')).toBeInTheDocument()
    })

    it('prefills inbound form from saved config', async () => {
      getInboundConfig.mockResolvedValue({
        slack: {
          bot_token: 'xoxb-prefilled',
          trigger_word: 'ava',
          allowed_channels: ['C111', 'C222'],
        },
        telegram: {
          bot_token: 'tg-prefilled',
          trigger_word: 'mara',
          allowed_chats: ['-100111', '222333'],
        },
      })
      render(wrap(<NotificationsSettings />))
      await waitFor(() => expect((screen.getByPlaceholderText('xoxb-...') as HTMLInputElement).value).toBe('xoxb-prefilled'))
      expect((screen.getByPlaceholderText('C012345, C678901') as HTMLInputElement).value).toBe('C111, C222')
      expect((screen.getByPlaceholderText('123456789:ABC...') as HTMLInputElement).value).toBe('tg-prefilled')
      expect((screen.getByPlaceholderText('-1001234567890, 123456789') as HTMLInputElement).value).toBe('-100111, 222333')
      const triggers = screen.getAllByPlaceholderText('shannon')
      expect((triggers[0] as HTMLInputElement).value).toBe('ava')
      expect((triggers[1] as HTMLInputElement).value).toBe('mara')
    })

    it('saves inbound dto when Slack + Telegram fields are filled', async () => {
      saveInboundConfig.mockResolvedValue(undefined)
      render(wrap(<NotificationsSettings />))
      await waitFor(() => expect(screen.getByPlaceholderText('xoxb-...')).toBeInTheDocument())
      fireEvent.change(screen.getByPlaceholderText('xoxb-...'), { target: { value: 'xoxb-test' } })
      fireEvent.change(screen.getByPlaceholderText('C012345, C678901'), { target: { value: 'C42, C99' } })
      fireEvent.change(screen.getByPlaceholderText('123456789:ABC...'), { target: { value: 'tg-test' } })
      fireEvent.click(screen.getByText('Save Inbound Config'))
      await waitFor(() => expect(saveInboundConfig).toHaveBeenCalledTimes(1))
      const dto = saveInboundConfig.mock.calls[0][0]
      expect(dto.slack.bot_token).toBe('xoxb-test')
      expect(dto.slack.trigger_word).toBe('shannon')
      expect(dto.slack.allowed_channels).toEqual(['C42', 'C99'])
      expect(dto.telegram.bot_token).toBe('tg-test')
      expect(dto.telegram.allowed_chats).toEqual([])
    })

    it('omits slack/telegram from dto when their bot token is empty', async () => {
      saveInboundConfig.mockResolvedValue(undefined)
      render(wrap(<NotificationsSettings />))
      await waitFor(() => expect(screen.getByPlaceholderText('xoxb-...')).toBeInTheDocument())
      fireEvent.click(screen.getByText('Save Inbound Config'))
      await waitFor(() => expect(saveInboundConfig).toHaveBeenCalledTimes(1))
      const dto = saveInboundConfig.mock.calls[0][0]
      expect(dto.slack).toBeNull()
      expect(dto.telegram).toBeNull()
    })

    it('clears inbound config when Clear Inbound is clicked', async () => {
      clearInboundConfig.mockResolvedValue(undefined)
      getInboundConfig.mockResolvedValue({
        slack: { bot_token: 'xoxb-old', trigger_word: 'shannon', allowed_channels: [] },
        telegram: null,
      })
      render(wrap(<NotificationsSettings />))
      await waitFor(() => expect((screen.getByPlaceholderText('xoxb-...') as HTMLInputElement).value).toBe('xoxb-old'))
      fireEvent.click(screen.getByText('Clear Inbound'))
      await waitFor(() => expect(clearInboundConfig).toHaveBeenCalledTimes(1))
      expect((screen.getByPlaceholderText('xoxb-...') as HTMLInputElement).value).toBe('')
    })
  })
})
