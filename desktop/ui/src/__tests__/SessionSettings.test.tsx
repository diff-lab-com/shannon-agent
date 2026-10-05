import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'
import SessionSettings from '@/components/settings/SessionSettings'

function wrap(ui: React.ReactElement) {
  return (
    <AppProvider>
      {ui}
    </AppProvider>
  )
}

const baseConfig = {
  provider: 'anthropic',
  model: 'claude-sonnet-4-6',
  api_key: 'sk-test',
  working_dir: '/tmp',
  approval_mode: 'normal',
}

describe('SessionSettings (Settings R3 T6 — 会话分区)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.getConfig).mockResolvedValue({ ...baseConfig })
  })

  // ① Auto-compaction card — the T6 replacement of the T1 placeholder.

  it('renders the auto-compact card with the new-session badge and next-message scope note', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-autocompact-card')
    expect(within(card).getByText('Auto-compact context')).toBeInTheDocument()
    // T1 EffectBadge semantics + the ruling wording: the engine is rebuilt
    // per message, so the flip lands on the NEXT message, not a new session.
    expect(within(card).getByText('Applies to new sessions')).toBeInTheDocument()
    expect(within(card).getByText('Takes effect on your next message')).toBeInTheDocument()
    // The informed-consent help copy (off = model I/O preserved verbatim).
    expect(within(card).getByText(/preserved in full/i)).toBeInTheDocument()
  })

  it('hydrates the auto-compact switch from the persisted config initial value', async () => {
    vi.mocked(api.getConfig).mockResolvedValue({ ...baseConfig, context_auto_compact: false })
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-autocompact-card')
    const sw = within(card).getByRole('switch', { name: 'Auto-compact context' })
    await waitFor(() => expect(sw).toHaveAttribute('aria-checked', 'false'))
  })

  it('defaults the auto-compact switch to ON when the config omits the field', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-autocompact-card')
    const sw = within(card).getByRole('switch', { name: 'Auto-compact context' })
    expect(sw).toHaveAttribute('aria-checked', 'true')
  })

  it('persists context.auto_compact=false through configure when toggled off', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-autocompact-card')
    const sw = within(card).getByRole('switch', { name: 'Auto-compact context' })
    await waitFor(() => expect(sw).toHaveAttribute('aria-checked', 'true'))
    fireEvent.click(sw)
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'context.auto_compact', value: 'false' })
    })
  })

  // ② Session GC card — migrated verbatim from AdvancedSettings (卡A),
  // now with a 7-day retention gear.

  it('renders the migrated session storage card with the disclosure copy and the 7-day gear', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-gc-card')
    expect(within(card).getByText('Session storage management')).toBeInTheDocument()
    const desc = within(card).getByText(/Automatically free storage by cleaning up archived sessions/)
    const copy = desc.textContent ?? ''
    expect(copy).toMatch(/Only archived sessions are ever cleaned/)
    expect(copy).toMatch(/last activity/)
    expect(copy).toMatch(/nothing is auto-deleted by default/)

    const select = within(card).getByRole('combobox', { name: 'Retention window' }) as HTMLSelectElement
    const options = Array.from(select.options).map(o => o.value)
    // T6 adds the 7-day gear: 永不 / 7 / 30 / 90.
    expect(options).toEqual(['0', '7', '30', '90'])
    expect(select.selectedOptions[0].textContent).toBe('Never')
  })

  it('persists the 7-day gear through configure as session_retention_days=7', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-gc-card')
    const select = within(card).getByRole('combobox', { name: 'Retention window' })
    fireEvent.change(select, { target: { value: '7' } })
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'session_retention_days', value: '7' })
    })
  })

  it('persists session_gc_enabled through configure when the migrated switch is toggled', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-gc-card')
    const sw = within(card).getByRole('switch', { name: 'Auto-clean archived sessions' })
    fireEvent.click(sw)
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'session_gc_enabled', value: 'true' })
    })
  })

  it('does not render the T1 placeholder card anymore', async () => {
    render(wrap(<SessionSettings />))
    await screen.findByTestId('session-autocompact-card')
    expect(screen.queryByTestId('session-placeholder-card')).not.toBeInTheDocument()
  })

  // ③ Auto-archive card — Settings R3 T7.

  it('renders the auto-archive card with the instant badge and the informed-consent help copy', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-auto-archive-card')
    expect(within(card).getByText('Auto-archive sessions')).toBeInTheDocument()
    // EffectBadge instant: the toggle persists immediately (no restart);
    // the help copy carries the scan semantics + where to restore from.
    expect(within(card).getByText('Instant effect')).toBeInTheDocument()
    const help = within(card).getByText(/Periodically scans sessions/i).textContent ?? ''
    expect(help).toMatch(/not running, nothing unread/)
    expect(help).toMatch(/unpinned/)
    expect(help).toMatch(/Archived section/)
  })

  it('defaults the auto-archive switch to OFF and the retention gear to 7 days', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-auto-archive-card')
    const sw = within(card).getByRole('switch', { name: 'Auto-archive sessions' })
    expect(sw).toHaveAttribute('aria-checked', 'false')
    const select = within(card).getByRole('combobox', { name: 'Retention period' }) as HTMLSelectElement
    expect(select.value).toBe('7')
    // The gear offers exactly the brief's 1/7/30/90 options.
    expect(Array.from(select.options).map(o => o.value)).toEqual(['1', '7', '30', '90'])
  })

  it('hydrates the auto-archive switch from the persisted config', async () => {
    vi.mocked(api.getConfig).mockResolvedValue({
      ...baseConfig,
      session_auto_archive_enabled: true,
      session_auto_archive_days: 30,
    })
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-auto-archive-card')
    const sw = within(card).getByRole('switch', { name: 'Auto-archive sessions' })
    await waitFor(() => expect(sw).toHaveAttribute('aria-checked', 'true'))
    const select = within(card).getByRole('combobox', { name: 'Retention period' }) as HTMLSelectElement
    expect(select.value).toBe('30')
  })

  it('persists the switch through configure as session.auto_archive_enabled', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-auto-archive-card')
    const sw = within(card).getByRole('switch', { name: 'Auto-archive sessions' })
    await waitFor(() => expect(sw).toBeInTheDocument())
    fireEvent.click(sw)
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'session.auto_archive_enabled', value: 'true' })
    })
  })

  it('persists the retention gear through configure as session.auto_archive_days', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-auto-archive-card')
    const select = within(card).getByRole('combobox', { name: 'Retention period' })
    fireEvent.change(select, { target: { value: '90' } })
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'session.auto_archive_days', value: '90' })
    })
  })

  // ④ Ask auto-continue card — Settings R3 T8 (chat.ask_user_auto_continue).

  it('renders the ask auto-continue card with the instant badge and the 5-minute help copy', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-ask-auto-continue-card')
    expect(within(card).getByText('Auto-continue unanswered questions')).toBeInTheDocument()
    // Ruling help copy: 5 minutes → "best judgment"; off = wait forever.
    const help = within(card).getByText(/auto-answered with/i).textContent ?? ''
    expect(help).toMatch(/5 minutes/)
    expect(help).toMatch(/best judgment/)
    expect(help).toMatch(/waits for your answer indefinitely/)
    // Reads live per question — instant, not new-session.
    expect(within(card).getByText('Instant effect')).toBeInTheDocument()
    expect(within(card).queryByText('Applies to new sessions')).not.toBeInTheDocument()
  })

  it('defaults the ask auto-continue switch to OFF when the config omits the field', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-ask-auto-continue-card')
    const sw = within(card).getByRole('switch', { name: 'Auto-continue unanswered questions' })
    await waitFor(() => expect(sw).toHaveAttribute('aria-checked', 'false'))
  })

  it('hydrates the ask auto-continue switch from the persisted config', async () => {
    vi.mocked(api.getConfig).mockResolvedValue({ ...baseConfig, chat_ask_user_auto_continue: true })
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-ask-auto-continue-card')
    const sw = within(card).getByRole('switch', { name: 'Auto-continue unanswered questions' })
    await waitFor(() => expect(sw).toHaveAttribute('aria-checked', 'true'))
  })

  it('persists chat.ask_user_auto_continue through configure when toggled on', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-ask-auto-continue-card')
    const sw = within(card).getByRole('switch', { name: 'Auto-continue unanswered questions' })
    await waitFor(() => expect(sw).toHaveAttribute('aria-checked', 'false'))
    fireEvent.click(sw)
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'chat.ask_user_auto_continue', value: 'true' })
    })
  })
})

// ⑤ Send-while-running card — Settings R3 T10. Purely front-end
// localStorage ('shannon.chat.sendBehavior'); no configure() round-trip.
describe('SessionSettings ⑤ running-send behavior (Settings R3 T10)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.getConfig).mockResolvedValue({ ...baseConfig })
    window.localStorage.clear()
  })

  it('renders the card with the two-tier segmented control, the ruling help copy, and the instant badge', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-send-behavior-card')
    expect(within(card).getByText('Send while running')).toBeInTheDocument()
    // The help copy explains both tiers and the always-on bolt escape hatch.
    const help = within(card).getByText(/while a run is still streaming/i).textContent ?? ''
    expect(help).toMatch(/interrupted right away/)
    expect(help).toMatch(/jumps the queue/)
    expect(help).toMatch(/waits in line/)
    expect(help).toMatch(/Ctrl\+Enter always interrupts immediately/)
    // Read live per send — instant, not new-session.
    expect(within(card).getByText('Instant effect')).toBeInTheDocument()
    // The two tiers, as a radiogroup.
    expect(within(card).getByRole('radiogroup', { name: 'Send while running' })).toBeInTheDocument()
    expect(within(card).getByRole('radio', { name: 'Steer (interrupt)' })).toBeInTheDocument()
    expect(within(card).getByRole('radio', { name: 'Add to queue' })).toBeInTheDocument()
  })

  it('defaults to queue — the status quo — when nothing is stored', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-send-behavior-card')
    const queue = within(card).getByRole('radio', { name: 'Add to queue' })
    const steer = within(card).getByRole('radio', { name: 'Steer (interrupt)' })
    expect(queue).toHaveAttribute('aria-checked', 'true')
    expect(steer).toHaveAttribute('aria-checked', 'false')
    expect(window.localStorage.getItem('shannon.chat.sendBehavior')).toBeNull()
  })

  it('writes steer to localStorage when Steer is picked (no configure round-trip)', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-send-behavior-card')
    const steer = within(card).getByRole('radio', { name: 'Steer (interrupt)' })
    fireEvent.click(steer)
    expect(steer).toHaveAttribute('aria-checked', 'true')
    expect(within(card).getByRole('radio', { name: 'Add to queue' })).toHaveAttribute('aria-checked', 'false')
    expect(window.localStorage.getItem('shannon.chat.sendBehavior')).toBe('steer')
    expect(api.configure).not.toHaveBeenCalled()
  })

  it('writes queue back to localStorage when Queue is re-picked', async () => {
    window.localStorage.setItem('shannon.chat.sendBehavior', 'steer')
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-send-behavior-card')
    // hydrates from the persisted value first
    await waitFor(() => expect(within(card).getByRole('radio', { name: 'Steer (interrupt)' })).toHaveAttribute('aria-checked', 'true'))
    const queue = within(card).getByRole('radio', { name: 'Add to queue' })
    fireEvent.click(queue)
    expect(queue).toHaveAttribute('aria-checked', 'true')
    expect(window.localStorage.getItem('shannon.chat.sendBehavior')).toBe('queue')
  })
})
