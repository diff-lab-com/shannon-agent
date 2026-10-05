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
})
