// R3-3 — chat header Plan/Act tier pair: config→label wiring, the keyboard
// contract, and the portal stacking contract.
// w4 refactor/header-menus-baseui — the popover is a Base UI Menu now: Base
// UI owns the body-level portal, the trigger anchoring and the keyboard
// contract (arrows/typeahead, Escape, focus-out close, focus return to the
// trigger). These jsdom tests pin the user-visible results; the REAL-click
// proof for the glass-header stacking escape lives in
// e2e/chat-script.model-mode.spec.ts (radiogroup/radio roles preserved).

import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { PhaseTierSwitcher } from '@/components/chat/PhaseTierSwitcher'
import * as api from '@/lib/tauri-api'

const mockCtx = vi.hoisted(() => ({
  config: null as Record<string, unknown> | null,
  refreshConfig: vi.fn(),
}))

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => mockCtx,
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn(), message: vi.fn() },
}))

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    configure: vi.fn().mockResolvedValue(undefined),
  }
})

function renderSwitcher(config: Record<string, unknown> | null) {
  mockCtx.config = config
  return render(<PhaseTierSwitcher />)
}

const menu = () => screen.getByTestId('phase-tier-menu')
const radios = () => screen.getAllByRole('radio')

async function openPopover() {
  const trigger = screen.getByTestId('phase-tier-switcher')
  trigger.focus()
  await userEvent.click(trigger)
  await screen.findByTestId('phase-tier-menu')
  return trigger
}

async function expectClosed() {
  await waitFor(() => {
    expect(screen.queryByTestId('phase-tier-menu')).toBeNull()
  }, { timeout: 5000 })
}

const highlightedRadio = () => radios().findIndex((r) => r.getAttribute('data-highlighted') != null)

beforeEach(() => {
  vi.mocked(api.configure).mockClear()
  mockCtx.refreshConfig.mockClear()
})

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

describe('PhaseTierSwitcher keyboard contract (Base UI Menu)', () => {
  it('opens both radiogroups with the config value checked', async () => {
    renderSwitcher({ plan_tier: 'fast', act_tier: 'standard' })
    const trigger = await openPopover()
    const groups = screen.getAllByRole('radiogroup')
    expect(groups.map((g) => g.getAttribute('aria-label'))).toEqual(['Planning tier', 'Execution tier'])
    // aria-checked mirrors the config (controlled RadioGroup), scoped per
    // group — each group carries an inherit/fast/standard/pro radio.
    const checkedIn = (group: HTMLElement) =>
      Array.from(group.querySelectorAll('[role="radio"]')).find(
        (r) => r.getAttribute('aria-checked') === 'true',
      )?.textContent
    expect(checkedIn(groups[0]!)).toBe('Fast')
    expect(checkedIn(groups[1]!)).toBe('Standard')
    expect(trigger).toHaveAttribute('aria-expanded', 'true')
  })

  it('arrows walk the radios; Space commits the focused tier write and keeps the popover open for the other phase', async () => {
    renderSwitcher({ plan_tier: 'fast', act_tier: 'standard' })
    await openPopover()
    // Pointer-open leaves the highlight unclaimed; the first ArrowDown
    // claims it on the first radio (plan group, inherit) and the walk is
    // sequential across both groups.
    await userEvent.keyboard('{ArrowDown}')
    expect(highlightedRadio()).toBe(0)
    // Walk to the PLAN group's Standard: inherit → fast → standard.
    await userEvent.keyboard('{ArrowDown}')
    await userEvent.keyboard('{ArrowDown}')
    expect(radios()[highlightedRadio()]).toHaveTextContent('Standard')
    await userEvent.keyboard(' ')
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'plan_tier', value: 'standard' })
    })
    // The multi-pick contract: a pick does NOT close the popover — the user
    // can set the act tier without reopening (Cline-style plan/act workflow).
    expect(screen.queryByTestId('phase-tier-menu')).not.toBeNull()
  })

  it('Escape closes without writing and returns focus to the trigger', async () => {
    renderSwitcher(null)
    const trigger = await openPopover()
    await userEvent.keyboard('{Escape}')
    await expectClosed()
    expect(api.configure).not.toHaveBeenCalled()
    expect(trigger).toBe(document.activeElement)
  })

  it('closes when keyboard focus leaves the popover (Tab-out), focus returning to the trigger', async () => {
    renderSwitcher(null)
    const trigger = await openPopover()
    // Wait for the open-focus sequence to settle (focus inside the popover)
    // so the tab-out starts from the popover, as a keyboard user's would.
    await waitFor(() => {
      expect(menu()).toBe(document.activeElement)
    }, { timeout: 5000 })
    // Base UI's first-class menu Tab-out: close with focus returning to the
    // trigger. (Simulating the DOM focusout with .focus() on an outsider
    // races Base UI's open-focus guard under load — fix round 1 — so the
    // keyboard path is driven directly; it is the same user contract.)
    await userEvent.keyboard('{Shift>}{Tab}{/Shift}')
    await expectClosed()
    expect(trigger).toBe(document.activeElement)
  })
})

describe('PhaseTierSwitcher portal stacking contract (#250, Base UI edition)', () => {
  it('mounts the open popover as a body-level portal carrying the z-modal token class', async () => {
    const { container } = renderSwitcher(null)
    await openPopover()
    const popover = menu()
    // Portal subtree rooted at body (popup → positioner → portal host → body).
    let root: HTMLElement = popover
    while (root.parentElement && root.parentElement !== document.body) {
      root = root.parentElement
    }
    expect(root.parentElement).toBe(document.body)
    expect(container.contains(popover)).toBe(false)
    // The positioner carries the token (floating layer above the header,
    // below the permission scrim) — same convention as ui/select.tsx.
    expect(popover.parentElement).toHaveClass('z-modal')
    // Outside-press still closes it with the popover portalled away from the
    // trigger wrapper.
    await userEvent.click(document.body)
    await expectClosed()
  })
})
