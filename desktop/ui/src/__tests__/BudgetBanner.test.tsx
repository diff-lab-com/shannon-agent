// BudgetBanner — component-level pin (chat-testing 裁定修复波, ledger issue
// CHAT-TEST-1).
//
// Five CI rounds on GitHub 2-core runners showed one stable anomaly in the
// L2 budget spec: the exceeded banner's body text stood in the DOM while
// every action-button query (role AND DOM-anchored) came back empty for the
// whole retry window — the forensic dump confirmed the matched [role=alert]
// WAS the BudgetBanner exceeded bar, with no dialog mounted, no
// aria-modal/inert pruner, and dozens of buttons elsewhere on the page. The
// same commit is green locally and in a docker container, so the button
// rendering could never be exercised on that runner. Rendering is
// UNCONDITIONAL in the exceeded branch (BudgetBanner.tsx exceeded block),
// so the behavior is pinned HERE instead, at the deterministic jsdom layer.
// The L2 spec (e2e/chat-script.budget.spec.ts) leans on this pin and skips
// its click-flow with a logged reason if the runner anomaly ever reappears.
//
// R2 W2-4 contract (rebase 适配 2026-10-02): the exceeded bar's Continue is
// labeled by what it will actually DELIVER — the `continueTarget` prop
// drives a three-state contract:
//   - 'blocked'       → en['budget.exceeded.continue']  ("send the blocked
//                       message" — the payload the pre-turn guard refused);
//   - 'last-message'  → en['budget.exceeded.continueLast'] ("resend the
//                       last message" — the recorded-turn fallback);
//   - null            → NO Continue button (hides instead of a clickable
//                       no-op); Raise budget… and Stop remain.
// Every assertion references en.json by KEY — a copy drift fails here by
// name, never by a hardcoded string.

import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'

import BudgetBanner from '@/components/chat/BudgetBanner'
import en from '@/i18n/locales/en.json'
import type { BudgetStatusPayload } from '@/types'

const PAYLOAD: BudgetStatusPayload = { sessionId: 'sess-budget', spentUsd: 6.4, budgetUsd: 5 }

// The exceeded bar's copy, keyed by their i18n ids — the assertion IS the
// key→copy mapping from en.json, so a copy drift fails here by name.
const CONTINUE_BLOCKED = en['budget.exceeded.continue']
const CONTINUE_LAST = en['budget.exceeded.continueLast']
const RAISE = en['budget.exceeded.raise']
const STOP = en['budget.exceeded.stop']

type BannerProps = Parameters<typeof BudgetBanner>[0]

function renderBanner(overrides: Partial<BannerProps> = {}) {
  const spies = {
    onContinueOnce: vi.fn(),
    clearWarning: vi.fn(),
    clearExceeded: vi.fn(),
  }
  const utils = render(
    <BudgetBanner
      warning={null}
      exceeded={null}
      clearWarning={spies.clearWarning}
      clearExceeded={spies.clearExceeded}
      onContinueOnce={spies.onContinueOnce}
      sessionId="sess-budget"
      {...overrides}
    />,
  )
  return { ...spies, ...utils }
}

describe('BudgetBanner — exceeded bar Continue three-state (R2 W2-4)', () => {
  it("continueTarget='blocked' labels Continue with the blocked-message copy", () => {
    const { getByRole } = renderBanner({ exceeded: PAYLOAD, continueTarget: 'blocked' })
    const banner = screen.getByRole('alert') // error tone → role="alert"
    expect(banner).toHaveTextContent(en['budget.exceeded.title'])
    // The blocked-label button exists exactly once; the last-message label
    // is NOT on the page (the labels are mutually exclusive).
    expect(getByRole('button', { name: CONTINUE_BLOCKED })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: CONTINUE_LAST })).toBeNull()
    // The other two frozen actions stay alongside.
    expect(getByRole('button', { name: RAISE })).toBeInTheDocument()
    expect(getByRole('button', { name: STOP })).toBeInTheDocument()
  })

  it("continueTarget='last-message' labels Continue with the resend-last copy", () => {
    const { getByRole } = renderBanner({ exceeded: PAYLOAD, continueTarget: 'last-message' })
    expect(screen.getByRole('button', { name: CONTINUE_LAST })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: CONTINUE_BLOCKED })).toBeNull()
    expect(getByRole('button', { name: RAISE })).toBeInTheDocument()
    expect(getByRole('button', { name: STOP })).toBeInTheDocument()
  })

  it('continueTarget=null hides Continue but keeps Raise budget… and Stop', () => {
    renderBanner({ exceeded: PAYLOAD, continueTarget: null })
    // Neither Continue variant renders — the action hides instead of
    // staying a clickable no-op.
    expect(screen.queryByRole('button', { name: CONTINUE_BLOCKED })).toBeNull()
    expect(screen.queryByRole('button', { name: CONTINUE_LAST })).toBeNull()
    expect(screen.getByRole('button', { name: RAISE })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: STOP })).toBeInTheDocument()
  })

  it('an undefined continueTarget still renders a Continue button (continueLast branch) — current behavior, flagged', () => {
    // PRODUCTS-CODE SHARP EDGE, recorded not fixed (test-only adaptation):
    // the component gates on `continueTarget !== null`, so a MISSING prop
    // (undefined) passes the gate and falls into the non-'blocked' branch —
    // the banner renders an actionable "resend the last message" button
    // where a strict reading of the contract (nothing to deliver → hide)
    // suggests hiding. Every real call site passes the prop explicitly
    // (Chat.tsx), so this only bites future consumers; revisit if the
    // component ever renders without the page's derivation.
    renderBanner({ exceeded: PAYLOAD, continueTarget: undefined })
    expect(screen.getByRole('button', { name: CONTINUE_LAST })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: CONTINUE_BLOCKED })).toBeNull()
  })

  it.each([
    ['blocked', CONTINUE_BLOCKED],
    ['last-message', CONTINUE_LAST],
  ] as const)('Continue (%s) clears the exceeded bar and fires onContinueOnce with no args', (target, label) => {
    const { onContinueOnce, clearExceeded, clearWarning } = renderBanner({
      exceeded: PAYLOAD,
      continueTarget: target,
    })
    fireEvent.click(screen.getByRole('button', { name: label }))
    // The component's contract: the bar clears BEFORE the resend fires, and
    // onContinueOnce carries no arguments (the resent turn is resolved by
    // the caller, Chat.tsx continuePastBudget).
    expect(clearExceeded).toHaveBeenCalledTimes(1)
    expect(onContinueOnce).toHaveBeenCalledTimes(1)
    expect(onContinueOnce).toHaveBeenCalledWith()
    expect(clearWarning).not.toHaveBeenCalled()
  })
})

describe('BudgetBanner — shared copy pins (en.json key→copy)', () => {
  it('renders the exceeded body with Intl-formatted USD in the shared shape', () => {
    renderBanner({ exceeded: PAYLOAD, continueTarget: 'last-message' })
    const banner = screen.getByRole('alert')
    // Intl-formatted USD in the shared body shape + the exceeded-only suffix
    // (the variant anchor the L2 spec also anchors on).
    expect(banner).toHaveTextContent(/\$6\.40 of \$5\.00 used/)
    expect(banner).toHaveTextContent(en['budget.exceeded.body'].replace('{spent}', '$6.40').replace('{budget}', '$5.00'))
  })

  it('Stop dismisses the exceeded bar without touching the resend callback', () => {
    const { onContinueOnce, clearExceeded } = renderBanner({ exceeded: PAYLOAD, continueTarget: 'last-message' })
    fireEvent.click(screen.getByRole('button', { name: STOP }))
    expect(clearExceeded).toHaveBeenCalledTimes(1)
    expect(onContinueOnce).not.toHaveBeenCalled()
  })
})

describe('BudgetBanner — warning bar', () => {
  it('shows no action buttons and dismisses via clearWarning', () => {
    const { clearWarning, clearExceeded, onContinueOnce } = renderBanner({ warning: PAYLOAD })
    const banner = screen.getByRole('status') // warning tone → role="status"
    expect(banner).toHaveTextContent(en['budget.warning.title'])
    expect(banner).toHaveTextContent(/\$6\.40 of \$5\.00 used/)
    for (const name of [CONTINUE_BLOCKED, CONTINUE_LAST, RAISE, STOP]) {
      expect(screen.queryByRole('button', { name })).toBeNull()
    }
    const dismiss = screen.getByRole('button', { name: en['budget.banner.dismiss'] })
    fireEvent.click(dismiss)
    expect(clearWarning).toHaveBeenCalledTimes(1)
    expect(clearExceeded).not.toHaveBeenCalled()
    expect(onContinueOnce).not.toHaveBeenCalled()
  })
})

describe('BudgetBanner — variant exclusivity (the CI variant-flip family)', () => {
  it('exceeded supersedes warning — only the error bar is mounted', () => {
    renderBanner({ warning: PAYLOAD, exceeded: PAYLOAD, continueTarget: 'last-message' })
    expect(screen.getByRole('alert')).toBeInTheDocument()
    expect(screen.queryByRole('status')).toBeNull()
    // And the actions are present exactly once — no duplicate bar underneath.
    expect(screen.getAllByRole('button', { name: CONTINUE_LAST })).toHaveLength(1)
    expect(screen.getAllByRole('button', { name: STOP })).toHaveLength(1)
  })
})
