// R3-3 — chat header Plan/Act tier pair: config→label wiring only (per the
// batch brief the popover interaction itself is e2e territory).
// w3 fix/header-dropdown-hit-test — plus the portal stacking contract: the
// popover must mount at body level carrying the z-modal token class, so it
// escapes the header's contain:paint stacking context and real pointer
// clicks reach the radios (jsdom can't hit-test; e2e proves the rest).

import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
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

describe('PhaseTierSwitcher portal stacking contract (w3 hit-test fix)', () => {
  it('mounts the open popover as a body-level portal carrying the z-modal token class', () => {
    const { container } = renderSwitcher(null)
    fireEvent.click(screen.getByTestId('phase-tier-switcher'))
    const menu = screen.getByTestId('phase-tier-menu')
    expect(menu.parentElement).toBe(document.body)
    expect(container.contains(menu)).toBe(false)
    expect(menu).toHaveClass('z-modal')
    // Outside-click still closes it with the menu portalled away from the
    // trigger wrapper.
    fireEvent.mouseDown(document.body)
    expect(screen.queryByTestId('phase-tier-menu')).toBeNull()
  })
})
