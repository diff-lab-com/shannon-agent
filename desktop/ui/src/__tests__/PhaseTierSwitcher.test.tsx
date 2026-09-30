// R3-3 — chat header Plan/Act tier pair: config→label wiring only (per the
// batch brief the popover interaction itself is e2e territory).

import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { PhaseTierSwitcher } from '@/components/chat/PhaseTierSwitcher'

const mockCtx = vi.hoisted(() => ({
  config: null as Record<string, unknown> | null,
  refreshConfig: vi.fn(),
}))

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => mockCtx,
}))

function renderSwitcher(config: Record<string, unknown> | null) {
  mockCtx.config = config
  return render(<PhaseTierSwitcher />)
}

describe('PhaseTierSwitcher (R3-3 header pair)', () => {
  it('renders the plan/act values from the config in the aria label', () => {
    renderSwitcher({ plan_tier: 'fast', act_tier: 'standard' })
    const btn = screen.getByTestId('phase-tier-switcher')
    expect(btn.getAttribute('aria-label')).toContain('Fast')
    expect(btn.getAttribute('aria-label')).toContain('Standard')
  })

  it('unset / junk config values render as inherit', () => {
    const { unmount } = renderSwitcher(null)
    expect(screen.getByTestId('phase-tier-switcher').getAttribute('aria-label')).toContain('Inherit')
    unmount()
    renderSwitcher({ plan_tier: 'ultra', act_tier: 'haiku' })
    expect(screen.getByTestId('phase-tier-switcher').getAttribute('aria-label')).toContain('Inherit')
  })
})
