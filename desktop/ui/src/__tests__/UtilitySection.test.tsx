// S3-3 — Settings → Models utility-slot dropdowns: backend wiring tests.
// The dropdowns are NATIVE <select> elements with <optgroup> per roster
// provider (the PhaseTierSection pattern), so the change events below are
// plain fireEvent — no Base-UI popup interaction.

import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { UtilitySection } from '@/components/settings/models-settings/UtilitySection'
import * as api from '@/lib/tauri-api'
type UtilitySlotsView = api.UtilitySlotsView
type UtilitySlotStatus = api.UtilitySlotStatus

const ROSTER = [
  { provider_id: 'anthropic', display_name: 'Anthropic', models: ['claude-haiku-4-5', 'claude-sonnet-4-6'] },
  { provider_id: 'zhipu', display_name: 'GLM (Zhipu)', models: ['glm-5.3-air'] },
  // A roster slot with NO candidates must not render a group.
  { provider_id: 'ollama', display_name: 'Ollama', models: [] },
]

function viewOf(slots: UtilitySlotStatus[]): UtilitySlotsView {
  return { slots, roster: ROSTER, profile: 'default' }
}

vi.mock('@/lib/tauri-api', async (importOriginal) => {
  const actual: Record<string, unknown> = await importOriginal()
  return {
    ...actual,
    getUtilitySlots: vi.fn(),
    setUtilitySlot: vi.fn(),
  }
})

describe('UtilitySection (S3-3 settings wiring)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.getUtilitySlots).mockResolvedValue(viewOf([
      { role: 'compression', provider: 'zhipu', model: 'glm-5.3-air', resolves: true },
      { role: 'title_generation', provider: null, model: null, resolves: false },
    ]))
    vi.mocked(api.setUtilitySlot).mockResolvedValue({
      role: 'compression',
      provider: 'anthropic',
      model: 'claude-haiku-4-5',
    })
  })

  it('renders both selects from the persisted slot state', async () => {
    render(<UtilitySection />)
    const compaction = (await screen.findByTestId('utility-slot-select-compaction')) as HTMLSelectElement
    const summary = (await screen.findByTestId('utility-slot-select-summary')) as HTMLSelectElement
    expect(compaction.value).toBe('zhipu::glm-5.3-air')
    // Unset slot renders the blank follow-default option.
    expect(summary.value).toBe('')
  })

  it('offers the roster as optgroups and skips candidate-less providers', async () => {
    render(<UtilitySection />)
    const select = await screen.findByTestId('utility-slot-select-compaction')
    const groups = Array.from(select.querySelectorAll('optgroup'))
    expect(groups.map((g) => g.label)).toEqual(['Anthropic', 'GLM (Zhipu)'])
    const options = Array.from(select.querySelectorAll('option')).map((o) => o.value)
    // Blank follow-default first, then every candidate model.
    expect(options[0]).toBe('')
    expect(options).toContain('anthropic::claude-haiku-4-5')
    expect(options).toContain('zhipu::glm-5.3-air')
  })

  it('shows the live resolution line for configured and unset slots', async () => {
    render(<UtilitySection />)
    await screen.findByTestId('utility-slot-select-compaction')
    expect(screen.getByTestId('utility-slot-resolved-compaction').textContent).toContain('glm-5.3-air')
    expect(screen.getByTestId('utility-slot-resolved-summary').textContent).toContain(
      'global default',
    )
  })

  it('flags a configured slot whose provider no longer resolves', async () => {
    vi.mocked(api.getUtilitySlots).mockResolvedValue(viewOf([
      { role: 'compression', provider: 'ghost', model: 'gone', resolves: false },
      { role: 'title_generation', provider: null, model: null, resolves: false },
    ]))
    render(<UtilitySection />)
    await screen.findByTestId('utility-slot-select-compaction')
    expect(screen.getByTestId('utility-slot-resolved-compaction').textContent).not.toContain('glm')
  })

  it('writes provider and model together and refreshes from the backend', async () => {
    render(<UtilitySection />)
    const select = await screen.findByTestId('utility-slot-select-compaction')
    // Wait for the mount fetch so `view` is loaded (the select enables only
    // then — a change before it would be an early-return no-op).
    await waitFor(() => expect(select).not.toBeDisabled())
    fireEvent.change(select, { target: { value: 'anthropic::claude-haiku-4-5' } })
    await waitFor(() =>
      expect(api.setUtilitySlot).toHaveBeenCalledWith(
        'compression',
        'anthropic',
        'claude-haiku-4-5',
      ),
    )
    await waitFor(() => expect(api.getUtilitySlots).toHaveBeenCalledTimes(2))
  })

  it('clearing a slot writes null/null (follow the global default again)', async () => {
    render(<UtilitySection />)
    const select = await screen.findByTestId('utility-slot-select-compaction')
    await waitFor(() => expect(select).not.toBeDisabled())
    fireEvent.change(select, { target: { value: '' } })
    await waitFor(() =>
      expect(api.setUtilitySlot).toHaveBeenCalledWith('compression', null, null),
    )
  })

  it('re-reads the persisted state even when the write fails (snap-back)', async () => {
    vi.mocked(api.setUtilitySlot).mockRejectedValueOnce(new Error('disk full'))
    render(<UtilitySection />)
    const select = await screen.findByTestId('utility-slot-select-compaction')
    await waitFor(() => expect(select).not.toBeDisabled())
    fireEvent.change(select, { target: { value: 'anthropic::claude-haiku-4-5' } })
    await waitFor(() => expect(api.setUtilitySlot).toHaveBeenCalled())
    await waitFor(() => expect(api.getUtilitySlots).toHaveBeenCalledTimes(2))
    // The select still shows the persisted value (state unchanged).
    expect((screen.getByTestId('utility-slot-select-compaction') as HTMLSelectElement).value).toBe(
      'zhipu::glm-5.3-air',
    )
  })
})
