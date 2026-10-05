// Settings R3 T11 (C6) — SessionSettings ⑥ 「消息流分组」card: three
// localStorage-backed switches (explore / terminal / changes), all default
// ON, writing through lib/toolGrouping's GROUPING_PREF_KEYS on flip.
// Deliberately a separate file from SessionSettings.test.tsx so the parallel
// T10 card work never collides in the same suite.

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, within } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'
import SessionSettings from '@/components/settings/SessionSettings'
import { GROUPING_PREF_KEYS, readGroupingPrefs } from '@/lib/toolGrouping'

function wrap(ui: React.ReactElement) {
  return <AppProvider>{ui}</AppProvider>
}

const baseConfig = {
  provider: 'anthropic',
  model: 'claude-sonnet-4-6',
  api_key: 'sk-test',
  working_dir: '/tmp',
  approval_mode: 'normal',
}

describe('SessionSettings ⑥ — message stream grouping (Settings R3 T11)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    localStorage.clear()
    vi.mocked(api.getConfig).mockResolvedValue({ ...baseConfig })
  })

  it('renders the grouping card with the instant badge and all three switches', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-grouping-card')
    expect(within(card).getByText('Message stream grouping')).toBeInTheDocument()
    // Instant: the switches persist to localStorage and apply on the next
    // render — no restart, no new-session semantics.
    expect(within(card).getByText('Instant effect')).toBeInTheDocument()
    expect(within(card).getByRole('switch', { name: 'Explore tools' })).toBeInTheDocument()
    expect(within(card).getByRole('switch', { name: 'Terminal commands' })).toBeInTheDocument()
    expect(within(card).getByRole('switch', { name: 'File changes' })).toBeInTheDocument()
    // Each switch carries its short help line.
    expect(within(card).getByText(/read-only calls/i)).toBeInTheDocument()
    expect(within(card).getByText(/shell command runs/i)).toBeInTheDocument()
    expect(within(card).getByText(/file-mutating calls/i)).toBeInTheDocument()
  })

  it('defaults all three switches to ON with no localStorage entries', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-grouping-card')
    for (const name of ['Explore tools', 'Terminal commands', 'File changes']) {
      expect(within(card).getByRole('switch', { name })).toHaveAttribute('aria-checked', 'true')
    }
    expect(readGroupingPrefs()).toEqual({ explore: true, terminal: true, changes: true })
  })

  it('turning a switch off persists the exact localStorage keys', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-grouping-card')
    fireEvent.click(within(card).getByRole('switch', { name: 'Explore tools' }))
    fireEvent.click(within(card).getByRole('switch', { name: 'File changes' }))
    expect(localStorage.getItem(GROUPING_PREF_KEYS.explore)).toBe('false')
    expect(localStorage.getItem(GROUPING_PREF_KEYS.changes)).toBe('false')
    // Terminal untouched: default ON.
    expect(localStorage.getItem(GROUPING_PREF_KEYS.terminal)).toBeNull()
    expect(readGroupingPrefs()).toEqual({ explore: false, terminal: true, changes: false })
  })

  it('turning a switch back on writes "true"', async () => {
    localStorage.setItem(GROUPING_PREF_KEYS.terminal, 'false')
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-grouping-card')
    const sw = within(card).getByRole('switch', { name: 'Terminal commands' })
    await new Promise(r => setTimeout(r, 0))
    expect(sw).toHaveAttribute('aria-checked', 'false')
    fireEvent.click(sw)
    expect(localStorage.getItem(GROUPING_PREF_KEYS.terminal)).toBe('true')
  })

  it('never touches the engine configure channel (display-only prefs)', async () => {
    render(wrap(<SessionSettings />))
    const card = await screen.findByTestId('session-grouping-card')
    fireEvent.click(within(card).getByRole('switch', { name: 'Explore tools' }))
    const groupingCalls = vi.mocked(api.configure).mock.calls.filter(
      c => typeof c[0]?.key === 'string' && c[0].key.startsWith('chat.grouping'),
    )
    expect(groupingCalls).toHaveLength(0)
  })
})
