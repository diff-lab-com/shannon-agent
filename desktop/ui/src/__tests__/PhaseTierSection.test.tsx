// R3-3 — Settings → Models phase-tier dropdowns: config wiring tests.
// The dropdowns are NATIVE <select> elements (the settings-modals pattern),
// so the change events below are plain fireEvent — no Base-UI popup
// interaction (the known-flaky jsdom path; the header popover's open/close
// behavior is e2e territory).

import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { PhaseTierSection } from '@/components/settings/models-settings/PhaseTierSection'
import * as api from '@/lib/tauri-api'

const mockRefreshConfig = vi.fn()

const TIERED_MODELS = [
  { id: 'claude-haiku-4-5-20251001', name: 'Claude Haiku 4.5', provider: 'anthropic', context_window: 200000, tier: 'fast', price_in: 0.8, price_out: 4 },
  { id: 'gpt-5-mini', name: 'GPT-5 Mini', provider: 'openai', context_window: 128000, tier: 'fast', price_in: 0.25, price_out: 2 },
  { id: 'claude-sonnet-4-6', name: 'Claude Sonnet 4.6', provider: 'anthropic', context_window: 200000, tier: 'standard', price_in: 3, price_out: 15 },
]

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: configStore.current,
    models: TIERED_MODELS,
    refreshConfig: mockRefreshConfig,
  }),
}))

const configStore: { current: Record<string, unknown> | null } = { current: null }

function renderWithConfig(config: Record<string, unknown> | null) {
  configStore.current = config
  return render(<PhaseTierSection />)
}

describe('PhaseTierSection (R3-3 settings wiring)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.configure).mockResolvedValue(undefined)
    mockRefreshConfig.mockResolvedValue(undefined)
  })

  it('renders both selects from the persisted config values', () => {
    renderWithConfig({ plan_tier: 'fast', act_tier: 'standard' })
    const plan = screen.getByTestId('phase-tier-select-plan') as HTMLSelectElement
    const act = screen.getByTestId('phase-tier-select-act') as HTMLSelectElement
    expect(plan.value).toBe('fast')
    expect(act.value).toBe('standard')
    // Four options each: inherit + the three tiers.
    expect(plan.options.length).toBe(4)
  })

  it('shows the resolved catalog model under each dropdown (cheapest-wins)', () => {
    renderWithConfig({ plan_tier: 'fast', act_tier: 'inherit' })
    // fast resolves to gpt-5-mini (0.25 + 2 < 0.8 + 4).
    expect(screen.getByTestId('phase-tier-resolved-plan').textContent).toContain('gpt-5-mini')
    expect(screen.getByTestId('phase-tier-resolved-act').textContent).toContain('global default')
  })

  it('junk stored values render as inherit, never as a wrong tier', () => {
    renderWithConfig({ plan_tier: 'ultra', act_tier: null })
    expect((screen.getByTestId('phase-tier-select-plan') as HTMLSelectElement).value).toBe('inherit')
    expect((screen.getByTestId('phase-tier-select-act') as HTMLSelectElement).value).toBe('inherit')
  })

  it('writes the right config key + value and refreshes on change', async () => {
    renderWithConfig({ plan_tier: 'inherit', act_tier: 'standard' })
    fireEvent.change(screen.getByTestId('phase-tier-select-plan'), { target: { value: 'pro' } })
    await waitFor(() =>
      expect(api.configure).toHaveBeenCalledWith({ key: 'plan_tier', value: 'pro' }),
    )
    await waitFor(() => expect(mockRefreshConfig).toHaveBeenCalled())
  })

  it('re-reads the persisted config even when the write fails (snap-back)', async () => {
    vi.mocked(api.configure).mockRejectedValueOnce(new Error('disk full'))
    renderWithConfig({ plan_tier: 'inherit', act_tier: 'inherit' })
    fireEvent.change(screen.getByTestId('phase-tier-select-act'), { target: { value: 'fast' } })
    await waitFor(() => expect(mockRefreshConfig).toHaveBeenCalled())
    // The select still shows the persisted value (config store unchanged).
    expect((screen.getByTestId('phase-tier-select-act') as HTMLSelectElement).value).toBe('inherit')
  })
})
