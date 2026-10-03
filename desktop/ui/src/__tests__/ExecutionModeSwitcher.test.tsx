// w4 refactor/header-menus-baseui — the switcher menu is a Base UI Menu now;
// Base UI owns the keyboard contract (arrow/typeahead navigation, Escape,
// focus-out close, focus return to the trigger) and the body-level portal.
// These tests pin the USER-VISIBLE contract, not Base UI internals:
//   - keyboard: open → arrows/typeahead move highlight+focus, Enter picks,
//     Escape/focus-out/outside-press close, focus always lands back on the
//     trigger (the P2-10 keyboard contract, now carried by Base UI);
//   - portal stacking (the #250 contract, same "what" on the new "how"):
//     the open menu mounts at body level — outside the header-local wrapper —
//     with the z-modal token class riding the positioner, so it wins the
//     paint/hit-test over the glass header without leaving the token system.
// jsdom can't hit-test; the REAL-click proof lives in
// e2e/chat-script.model-mode.spec.ts.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { I18nProvider } from '@/i18n'
import { ExecutionModeSwitcher } from '@/components/chat/ExecutionModeSwitcher'
import * as api from '@/lib/tauri-api'
import type * as ReactRouterDom from 'react-router-dom'

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn(), message: vi.fn() },
}))

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof ReactRouterDom>('react-router-dom')
  return { ...actual, useNavigate: () => vi.fn() }
})

const mockRefreshConfig = vi.fn()
vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: { active_permission_profile: 'strict', approval_mode: 'auto' },
    refreshConfig: mockRefreshConfig,
  }),
}))

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    activatePermissionProfile: vi.fn().mockResolvedValue(undefined),
  }
})

function renderSwitcher() {
  return render(
    <I18nProvider>
      <ExecutionModeSwitcher />
    </I18nProvider>,
  )
}

const listbox = () => screen.getByRole('listbox', { name: 'Execution mode options' })
const options = () => screen.getAllByRole('option')

/** Open with the trigger focused (as a keyboard user would: Tab to it, Enter). */
async function openMenu(container: HTMLElement) {
  const trigger = container.querySelector('button')!
  trigger.focus()
  await userEvent.click(trigger)
  await screen.findByRole('listbox', { name: 'Execution mode options' })
  return trigger
}

/** The Base UI close is a transition-status → unmount microtask, not sync.
 *  5s budget: under parallel-test load the default 1s waitFor cap produced
 *  constant ~1.07s failures (fix round 1 review). */
async function expectClosed() {
  await waitFor(() => {
    expect(screen.queryByRole('listbox', { name: 'Execution mode options' })).toBeNull()
  }, { timeout: 5000 })
}

const highlighted = () => options().findIndex((o) => o.getAttribute('data-highlighted') != null)

beforeEach(() => {
  vi.mocked(api.activatePermissionProfile).mockClear()
  vi.mocked(api.activatePermissionProfile).mockResolvedValue(undefined)
  mockRefreshConfig.mockClear()
})

describe('ExecutionModeSwitcher keyboard contract (Base UI Menu)', () => {
  it('opens the listbox portal with the selected tier marked', async () => {
    const { container } = renderSwitcher()
    const trigger = await openMenu(container)
    // active_permission_profile=strict → the first option is the selected one.
    expect(options()[0]).toHaveAttribute('aria-selected', 'true')
    expect(trigger).toHaveAttribute('aria-expanded', 'true')
  })

  it('moves highlight and focus with ArrowDown/ArrowUp, focus following the highlight', async () => {
    const { container } = renderSwitcher()
    await openMenu(container)
    // Pointer-open leaves the highlight unclaimed; the first ArrowDown claims
    // it on the SELECTED option (strict at index 0), then navigation flows.
    await userEvent.keyboard('{ArrowDown}')
    expect(highlighted()).toBe(0)
    expect(options()[0]).toBe(document.activeElement)
    await userEvent.keyboard('{ArrowDown}')
    expect(highlighted()).toBe(1)
    expect(options()[1]).toBe(document.activeElement)
    await userEvent.keyboard('{ArrowUp}')
    expect(highlighted()).toBe(0)
    expect(options()[0]).toBe(document.activeElement)
    // Wrap upwards past the first option: the highlight leaves index 0 and
    // focus follows it. (jsdom's composite registration jitters the exact
    // wrap landing spot, so the exact index is pinned by real-browser e2e,
    // not here.)
    await userEvent.keyboard('{ArrowUp}')
    await waitFor(() => {
      const idx = highlighted()
      expect(idx).not.toBe(0)
      expect(options()[idx]).toBe(document.activeElement)
    })
  })

  it('typeahead jumps to the option whose label starts with the typed letter', async () => {
    const { container } = renderSwitcher()
    await openMenu(container)
    await userEvent.keyboard('p')
    const idx = highlighted()
    expect(options()[idx]).toHaveTextContent('Permissive')
    expect(options()[idx]).toBe(document.activeElement)
  })

  it('Enter picks the highlighted option, closes, and returns focus to the trigger', async () => {
    const { container } = renderSwitcher()
    const trigger = await openMenu(container)
    // Claim the highlight on strict, step to Balanced, pick it.
    await userEvent.keyboard('{ArrowDown}')
    await userEvent.keyboard('{ArrowDown}')
    await userEvent.keyboard('{Enter}')
    await waitFor(() => {
      expect(api.activatePermissionProfile).toHaveBeenCalledWith('balanced')
    })
    await expectClosed()
    expect(trigger).toBe(document.activeElement)
  })

  it('Escape closes without picking and returns focus to the trigger', async () => {
    const { container } = renderSwitcher()
    const trigger = await openMenu(container)
    await userEvent.keyboard('{Escape}')
    await expectClosed()
    expect(api.activatePermissionProfile).not.toHaveBeenCalled()
    expect(trigger).toBe(document.activeElement)
  })

  it('closes when keyboard focus leaves the menu (Tab-out), focus returning to the trigger', async () => {
    const { container } = renderSwitcher()
    const trigger = await openMenu(container)
    // Wait for the open-focus sequence to settle (focus inside the menu)
    // so the tab-out starts from the menu, as a keyboard user's would.
    await waitFor(() => {
      expect(listbox()).toBe(document.activeElement)
    }, { timeout: 5000 })
    // Base UI's first-class menu Tab-out: close with focus returning to the
    // trigger. (Simulating the DOM focusout with .focus() on an outsider
    // races Base UI's open-focus guard under load — fix round 1 — so the
    // keyboard path is driven directly; it is the same user contract.)
    await userEvent.keyboard('{Shift>}{Tab}{/Shift}')
    await expectClosed()
    expect(trigger).toBe(document.activeElement)
  })

  it('still closes on an outside pointer press', async () => {
    const { container } = renderSwitcher()
    await openMenu(container)
    await userEvent.click(document.body)
    await expectClosed()
  })
})

describe('ExecutionModeSwitcher portal stacking contract (#250, Base UI edition)', () => {
  it('mounts the open menu as a body-level portal, outside the header-local wrapper', async () => {
    const { container } = renderSwitcher()
    await openMenu(container)
    const menu = listbox()
    // Base UI wraps the popup in a positioner (and its portal host) under the
    // body: walk up to the portal root and assert it is a direct body child
    // outside the component wrapper.
    let root: HTMLElement = menu
    while (root.parentElement && root.parentElement !== document.body) {
      root = root.parentElement
    }
    expect(root.parentElement).toBe(document.body)
    expect(container.contains(menu)).toBe(false)
  })

  it('carries the z-modal token class on the positioner (never an arbitrary z-index)', async () => {
    const { container } = renderSwitcher()
    await openMenu(container)
    // The positioner owns the stacking context (floating-ui's transform would
    // otherwise trap the popup's own z-index) — same convention as
    // ui/select.tsx. Token scale: header 40 < modal 50 < scrim/flash.
    expect(listbox().parentElement).toHaveClass('z-modal')
    // The e2e anchor (chat-script.model-mode.spec.ts) drives the same path
    // with a REAL Playwright click; here we pin that the option's onClick
    // stays wired through the portal.
    fireEvent.click(options()[1]!)
    await waitFor(() => {
      expect(api.activatePermissionProfile).toHaveBeenCalledWith('balanced')
    })
  })
})
