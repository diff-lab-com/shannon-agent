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
// so the behavior is pinned HERE instead, at the deterministic jsdom layer:
// the three frozen actions with their exact en.json copy, the Continue-once
// callback wiring, and the warning/exceeded mutual exclusion. The L2 spec
// (e2e/chat-script.budget.spec.ts) leans on this pin and skips its
// click-flow with a logged reason if the runner anomaly ever reappears.

import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, within } from '@testing-library/react'

import BudgetBanner from '@/components/chat/BudgetBanner'
import en from '@/i18n/locales/en.json'
import type { BudgetStatusPayload } from '@/types'

const PAYLOAD: BudgetStatusPayload = { sessionId: 'sess-budget', spentUsd: 6.4, budgetUsd: 5 }

// The frozen exceeded trio, keyed by their i18n ids — the assertion IS the
// key→copy mapping from en.json, so a copy drift fails here by name.
const ACTION_KEYS = [
  'budget.exceeded.continue',
  'budget.exceeded.raise',
  'budget.exceeded.stop',
] as const

function renderBanner(overrides: Partial<Parameters<typeof BudgetBanner>[0]> = {}) {
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

describe('BudgetBanner — exceeded bar (CHAT-TEST-1 deterministic pin)', () => {
  it('renders the three frozen actions with the en.json copy', () => {
    renderBanner({ exceeded: PAYLOAD })
    const banner = screen.getByRole('alert') // error tone → role="alert"
    expect(banner).toHaveTextContent(en['budget.exceeded.title'])
    // Intl-formatted USD in the shared body shape + the exceeded-only suffix
    // (the variant anchor the L2 spec also anchors on).
    expect(banner).toHaveTextContent(/\$6\.40 of \$5\.00 used/)
    expect(banner).toHaveTextContent(en['budget.exceeded.body'].replace('{spent}', '$6.40').replace('{budget}', '$5.00'))
    for (const key of ACTION_KEYS) {
      expect(within(banner).getByRole('button', { name: en[key] })).toBeInTheDocument()
    }
  })

  it('Continue (ignore once) clears the exceeded bar and fires onContinueOnce with no args', () => {
    const { onContinueOnce, clearExceeded, clearWarning } = renderBanner({ exceeded: PAYLOAD })
    fireEvent.click(screen.getByRole('button', { name: en['budget.exceeded.continue'] }))
    // The component's contract: the bar clears BEFORE the resend fires, and
    // onContinueOnce carries no arguments (the resent turn is resolved by
    // the caller, Chat.tsx continuePastBudget).
    expect(clearExceeded).toHaveBeenCalledTimes(1)
    expect(onContinueOnce).toHaveBeenCalledTimes(1)
    expect(onContinueOnce).toHaveBeenCalledWith()
    expect(clearWarning).not.toHaveBeenCalled()
  })

  it('Stop dismisses the exceeded bar without touching the resend callback', () => {
    const { onContinueOnce, clearExceeded } = renderBanner({ exceeded: PAYLOAD })
    fireEvent.click(screen.getByRole('button', { name: en['budget.exceeded.stop'] }))
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
    for (const key of ACTION_KEYS) {
      expect(screen.queryByRole('button', { name: en[key] })).toBeNull()
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
    renderBanner({ warning: PAYLOAD, exceeded: PAYLOAD })
    expect(screen.getByRole('alert')).toBeInTheDocument()
    expect(screen.queryByRole('status')).toBeNull()
    // And the trio is present exactly once — no duplicate bar underneath.
    expect(screen.getAllByRole('button', { name: en['budget.exceeded.continue'] })).toHaveLength(1)
  })
})
