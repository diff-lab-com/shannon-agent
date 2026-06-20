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
const getInboundListenerStatus = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  getWebhookConfig: (...args: unknown[]) => getWebhookConfig(...args),
  saveWebhookConfig: (...args: unknown[]) => saveWebhookConfig(...args),
  clearWebhookConfig: (...args: unknown[]) => clearWebhookConfig(...args),
  getInboundConfig: (...args: unknown[]) => getInboundConfig(...args),
  saveInboundConfig: (...args: unknown[]) => saveInboundConfig(...args),
  clearInboundConfig: (...args: unknown[]) => clearInboundConfig(...args),
  getInboundListenerStatus: (...args: unknown[]) => getInboundListenerStatus(...args),
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
  getInboundListenerStatus.mockReset()
  getWebhookConfig.mockResolvedValue(null)
  getInboundConfig.mockResolvedValue({})
  getInboundListenerStatus.mockResolvedValue({ slack_running: false, telegram_running: false })
})

describe('NotificationsSettings - Wizard Layout', () => {
  it('renders channel cards when no channel selected', async () => {
    render(wrap(<NotificationsSettings />))
    await waitFor(() => expect(screen.getByText('Slack')).toBeInTheDocument())
    expect(screen.getByText('Telegram')).toBeInTheDocument()
    expect(screen.getByText('Email')).toBeInTheDocument()
  })

  it('shows configured status for Slack channel when config exists', async () => {
    getInboundConfig.mockResolvedValue({
      slack: { bot_token: 'xoxb-test', trigger_word: 'shannon', allowed_channels: ['C123'] },
      telegram: null,
    })
    render(wrap(<NotificationsSettings />))
    await waitFor(() => expect(screen.getByText('Slack')).toBeInTheDocument())
    expect(screen.getByText('Configured')).toBeInTheDocument()
  })

  it('shows connected status when listener is active', async () => {
    getInboundConfig.mockResolvedValue({
      slack: { bot_token: 'xoxb-test', trigger_word: 'shannon', allowed_channels: ['C123'] },
      telegram: null,
    })
    getInboundListenerStatus.mockResolvedValue({ slack_running: true, telegram_running: false })
    render(wrap(<NotificationsSettings />))
    await waitFor(() => expect(screen.getByText('Connected')).toBeInTheDocument())
  })

  it('shows inactive badge when configured but listener not running', async () => {
    getInboundConfig.mockResolvedValue({
      slack: { bot_token: 'xoxb-test', trigger_word: 'shannon', allowed_channels: ['C123'] },
      telegram: null,
    })
    getInboundListenerStatus.mockResolvedValue({ slack_running: false, telegram_running: false })
    render(wrap(<NotificationsSettings />))
    await waitFor(() => expect(screen.getByText('Inactive')).toBeInTheDocument())
    expect(screen.queryByText('Connected')).not.toBeInTheDocument()
  })

  it('opens Slack wizard when Slack card is clicked', async () => {
    render(wrap(<NotificationsSettings />))
    await waitFor(() => expect(screen.getByText('Slack')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Slack'))
    await waitFor(() => expect(screen.getByRole('heading', { name: 'Create Slack App' })).toBeInTheDocument())
  })

  it('opens Telegram wizard when Telegram card is clicked', async () => {
    render(wrap(<NotificationsSettings />))
    await waitFor(() => expect(screen.getByText('Telegram')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Telegram'))
    await waitFor(() => expect(screen.getByRole('heading', { name: 'Create Telegram Bot' })).toBeInTheDocument())
  })

  it('opens Email wizard when Email card is clicked', async () => {
    render(wrap(<NotificationsSettings />))
    await waitFor(() => expect(screen.getByText('Email')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Email'))
    await waitFor(() => expect(screen.getByRole('heading', { name: 'Email (IMAP)' })).toBeInTheDocument())
  })
})

describe('NotificationsSettings - Slack Wizard', () => {
  beforeEach(() => {
    getWebhookConfig.mockResolvedValue(null)
    getInboundConfig.mockResolvedValue({})
  })

  it('renders Slack wizard with 3 steps', async () => {
    render(wrap(<NotificationsSettings />))
    await waitFor(() => expect(screen.getByText('Slack')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Slack'))
    await waitFor(() => expect(screen.getByRole('heading', { name: 'Create Slack App' })).toBeInTheDocument())

    // Check that wizard content rendered
    expect(screen.getByText('Next →')).toBeInTheDocument()
    expect(screen.getByText('Cancel')).toBeInTheDocument()
  })

  it('navigates through Slack wizard steps', async () => {
    render(wrap(<NotificationsSettings />))
    await waitFor(() => expect(screen.getByText('Slack')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Slack'))

    await waitFor(() => expect(screen.getByRole('heading', { name: 'Create Slack App' })).toBeInTheDocument())

    // Step 1 → Step 2
    const nextButtons = screen.getAllByText('Next →')
    fireEvent.click(nextButtons[0])
    await waitFor(() => expect(screen.getByRole('heading', { name: 'Configure Socket Mode' })).toBeInTheDocument())

    // Step 2 → Step 3
    fireEvent.click(nextButtons[nextButtons.length - 1])
    await waitFor(() => expect(screen.getByRole('heading', { name: 'Configure Inbound' })).toBeInTheDocument())
  })

  it('cancels Slack wizard and returns to channel cards', async () => {
    render(wrap(<NotificationsSettings />))
    await waitFor(() => expect(screen.getByText('Slack')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Slack'))

    await waitFor(() => expect(screen.getByRole('heading', { name: 'Create Slack App' })).toBeInTheDocument())

    fireEvent.click(screen.getByText('Cancel'))
    await waitFor(() => expect(screen.getByText('Slack')).toBeInTheDocument())
  })

  it('saves Slack config on final step', async () => {
    saveInboundConfig.mockResolvedValue(undefined)
    render(wrap(<NotificationsSettings />))
    await waitFor(() => expect(screen.getByText('Slack')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Slack'))

    await waitFor(() => expect(screen.getByRole('heading', { name: 'Create Slack App' })).toBeInTheDocument())

    // Navigate to step 2
    fireEvent.click(screen.getByText('Next →'))
    await waitFor(() => expect(screen.getByRole('heading', { name: 'Configure Socket Mode' })).toBeInTheDocument())

    // Enter bot token
    const botTokenInput = screen.getByPlaceholderText('xoxb-...')
    fireEvent.change(botTokenInput, { target: { value: 'xoxb-test-token' } })

    // Navigate to step 3
    fireEvent.click(screen.getByText('Next →'))
    await waitFor(() => expect(screen.getByRole('heading', { name: 'Configure Inbound' })).toBeInTheDocument())

    // Save - use the wizard's save button (not webhook section)
    const saveButtons = screen.getAllByText('Save')
    const wizardSaveButton = saveButtons[saveButtons.length - 1] // Last one should be wizard
    fireEvent.click(wizardSaveButton)
    await waitFor(() => expect(saveInboundConfig).toHaveBeenCalled())

    const savedConfig = saveInboundConfig.mock.calls[0][0]
    expect(savedConfig.slack.bot_token).toBe('xoxb-test-token')
  })
})

describe('NotificationsSettings - Telegram Wizard', () => {
  beforeEach(() => {
    getWebhookConfig.mockResolvedValue(null)
    getInboundConfig.mockResolvedValue({})
  })

  it('renders Telegram wizard with BotFather link', async () => {
    render(wrap(<NotificationsSettings />))
    await waitFor(() => expect(screen.getByText('Telegram')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Telegram'))

    await waitFor(() => expect(screen.getByRole('heading', { name: 'Create Telegram Bot' })).toBeInTheDocument())
    expect(screen.getByText('Open @BotFather')).toBeInTheDocument()
    // The link is inside a button that opens via window.open, not an anchor tag
    // Just verify the button text is present
  })

  it('navigates through Telegram wizard steps', async () => {
    render(wrap(<NotificationsSettings />))
    await waitFor(() => expect(screen.getByText('Telegram')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Telegram'))

    await waitFor(() => expect(screen.getByRole('heading', { name: 'Create Telegram Bot' })).toBeInTheDocument())

    // Step 1 → Step 2
    fireEvent.click(screen.getByText('Next →'))
    await waitFor(() => expect(screen.getByRole('heading', { name: 'Configure Bot Token' })).toBeInTheDocument())

    // Step 2 → Step 3
    fireEvent.click(screen.getByText('Next →'))
    await waitFor(() => expect(screen.getByRole('heading', { name: 'Configure Inbound' })).toBeInTheDocument())
  })

  it('saves Telegram config on final step', async () => {
    saveInboundConfig.mockResolvedValue(undefined)
    render(wrap(<NotificationsSettings />))
    await waitFor(() => expect(screen.getByText('Telegram')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Telegram'))

    await waitFor(() => expect(screen.getByRole('heading', { name: 'Create Telegram Bot' })).toBeInTheDocument())

    // Navigate to step 2
    fireEvent.click(screen.getByText('Next →'))
    await waitFor(() => expect(screen.getByRole('heading', { name: 'Configure Bot Token' })).toBeInTheDocument())

    // Enter bot token
    const botTokenInput = screen.getByPlaceholderText('123456789:ABC...')
    fireEvent.change(botTokenInput, { target: { value: '123456789:ABCtest' } })

    // Navigate to step 3
    fireEvent.click(screen.getByText('Next →'))
    await waitFor(() => expect(screen.getByRole('heading', { name: 'Configure Inbound' })).toBeInTheDocument())

    // Save - use the wizard's save button (not webhook section)
    const saveButtons = screen.getAllByText('Save')
    const wizardSaveButton = saveButtons[saveButtons.length - 1] // Last one should be wizard
    fireEvent.click(wizardSaveButton)
    await waitFor(() => expect(saveInboundConfig).toHaveBeenCalled())

    const savedConfig = saveInboundConfig.mock.calls[0][0]
    expect(savedConfig.telegram.bot_token).toBe('123456789:ABCtest')
  })
})

describe('NotificationsSettings - Email Wizard', () => {
  beforeEach(() => {
    getWebhookConfig.mockResolvedValue(null)
    getInboundConfig.mockResolvedValue({})
  })

  it('renders Email wizard with IMAP fields', async () => {
    render(wrap(<NotificationsSettings />))
    await waitFor(() => expect(screen.getByText('Email')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Email'))

    await waitFor(() => expect(screen.getByRole('heading', { name: 'Email (IMAP)' })).toBeInTheDocument())
    expect(screen.getByPlaceholderText('imap.gmail.com')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('993')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('user@example.com')).toBeInTheDocument()
  })
})
