import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import OutboundSection from '@/components/settings/notifications/OutboundSection'
import * as api from '@/lib/tauri-api'

describe('OutboundSection', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  afterEach(() => {
    cleanup()
  })

  it('renders loading state before config is fetched', () => {
    vi.spyOn(api, 'getOutboundConfig').mockImplementation(() => new Promise(() => {}))
    render(<OutboundSection />)
    expect(screen.getByRole('status')).toBeInTheDocument()
  })

  it('loads and populates slack + telegram fields', async () => {
    vi.spyOn(api, 'getOutboundConfig').mockResolvedValue({
      slack: { bot_token: 'xoxb-secret', channel: '#general' },
      telegram: { bot_token: '123:abc', chat_id: '@chan' },
    })
    render(<OutboundSection />)
    await waitFor(() => {
      expect((screen.getByPlaceholderText('xoxb-••••••') as HTMLInputElement).value).toBe(
        'xoxb-secret',
      )
    })
    expect(
      (screen.getByPlaceholderText('#general or C012345') as HTMLInputElement).value,
    ).toBe('#general')
    expect(
      (screen.getByPlaceholderText('1234567890:ABC...') as HTMLInputElement).value,
    ).toBe('123:abc')
  })

  it('errors when both providers are empty on save', async () => {
    vi.spyOn(api, 'getOutboundConfig').mockResolvedValue({})
    const saveSpy = vi
      .spyOn(api, 'saveOutboundConfig')
      .mockResolvedValue(undefined)
    render(<OutboundSection />)
    await waitFor(() => expect(screen.queryByRole('status')).not.toBeInTheDocument())
    fireEvent.click(screen.getByText('Save'))
    // Toast renders in a portal — assert via the save spy instead.
    await waitFor(() => {
      expect(saveSpy).not.toHaveBeenCalled()
    })
  })

  it('saves when slack fields are filled', async () => {
    vi.spyOn(api, 'getOutboundConfig').mockResolvedValue({})
    const saveSpy = vi
      .spyOn(api, 'saveOutboundConfig')
      .mockResolvedValue(undefined)
    render(<OutboundSection />)
    await waitFor(() => expect(screen.queryByRole('status')).not.toBeInTheDocument())
    fireEvent.change(screen.getByPlaceholderText('xoxb-••••••'), {
      target: { value: 'xoxb-1' },
    })
    fireEvent.change(screen.getByPlaceholderText('#general or C012345'), {
      target: { value: '#dev' },
    })
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => {
      expect(saveSpy).toHaveBeenCalledWith({
        slack: { bot_token: 'xoxb-1', channel: '#dev' },
        telegram: null,
      })
    })
  })

  it('renders per-channel results after a test send', async () => {
    vi.spyOn(api, 'getOutboundConfig').mockResolvedValue({
      slack: { bot_token: 'x', channel: '#x' },
    })
    vi.spyOn(api, 'sendOutboundTest').mockResolvedValue({
      results: [
        { provider: 'slack', ok: true },
        { provider: 'telegram', ok: false, error: 'not configured' },
      ],
    })
    render(<OutboundSection />)
    await waitFor(() => expect(screen.queryByRole('status')).not.toBeInTheDocument())
    fireEvent.click(screen.getByText('Send test'))
    await waitFor(() => {
      expect(screen.getByText('slack')).toBeInTheDocument()
      expect(screen.getByText('telegram')).toBeInTheDocument()
      expect(screen.getByText('not configured')).toBeInTheDocument()
    })
  })

  it('clears fields on clear', async () => {
    vi.spyOn(api, 'getOutboundConfig').mockResolvedValue({
      slack: { bot_token: 'xoxb-x', channel: '#x' },
    })
    const clearSpy = vi
      .spyOn(api, 'clearOutboundConfig')
      .mockResolvedValue(undefined)
    render(<OutboundSection />)
    await waitFor(() => {
      expect((screen.getByPlaceholderText('xoxb-••••••') as HTMLInputElement).value).toBe(
        'xoxb-x',
      )
    })
    fireEvent.click(screen.getByText('Clear'))
    await waitFor(() => {
      expect(clearSpy).toHaveBeenCalled()
      expect((screen.getByPlaceholderText('xoxb-••••••') as HTMLInputElement).value).toBe('')
    })
  })
})
