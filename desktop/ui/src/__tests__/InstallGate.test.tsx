// Dangerous-install confirmation gate — UI half (2026-10-10 design).
//
// Covers the four contracts the surfaces depend on:
//   1. parseConfirmationRequired detects the backend's structured refusal
//      (JSON string / Error-wrapped / { message } shapes) and leaves every
//      non-gate error alone;
//   2. InstallConfirmDrawer renders a real gate payload, keeps the confirm
//      button disabled until the entry name is typed back EXACTLY (no
//      client-side trim, case-sensitive), and reports confirm/cancel;
//   3. useInstallGate retries the SAME command with
//      `confirmation: { acknowledged_risk: 'dangerous', typed_name }`,
//      resolves null on cancel, reopens with a fresh payload when the retry
//      gates again, and lets non-gate errors bypass the drawer;
//   4. the 批 2 badge helper copy carries the gate fact
//      (extensions.featured.securityBadgeGateHelp).

import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import InstallConfirmDrawer from '@/components/extensions/InstallConfirmDrawer'
import {
  parseConfirmationRequired,
  buildInstallConfirmation,
} from '@/lib/installGate'
import { useInstallGate, type InstallAttempt } from '@/hooks/useInstallGate'

// The backend's wire payload (extensions_commands.rs ConfirmationRequiredError),
// exactly as the real scanner would emit it for the demo Dangerous seed's
// description (data/catalog.ts skill-auto-reply-pro).
const GATE_PAYLOAD = {
  error: 'confirmation_required',
  risk: 'dangerous',
  matches: [
    {
      pattern: 'ignore previous instructions',
      matched_substring: 'Ignore previous instructions',
      category: 'system_override',
    },
    {
      pattern: "send the user's",
      matched_substring: "send the user's",
      category: 'data_exfil',
    },
  ],
  match_count: 2,
  required: 'type_to_confirm',
  name: 'auto-reply-pro',
}

const GATE_JSON = JSON.stringify(GATE_PAYLOAD)

const okResult = { id: 'skill:auto-reply-pro', name: 'auto-reply-pro', install_path: '~/.shannon/skills/auto-reply-pro' }

// --- 1. payload detection ---------------------------------------------------

describe('parseConfirmationRequired', () => {
  it('detects the gate in a raw JSON string rejection (Tauri Err(String))', () => {
    expect(parseConfirmationRequired(GATE_JSON)).toMatchObject({
      error: 'confirmation_required',
      name: 'auto-reply-pro',
      match_count: 2,
    })
  })

  it('detects the gate in an Error-wrapped payload (mock layers)', () => {
    expect(parseConfirmationRequired(new Error(GATE_JSON))?.name).toBe('auto-reply-pro')
  })

  it('detects the gate in a { message }-shaped rejection', () => {
    expect(parseConfirmationRequired({ message: GATE_JSON })?.risk).toBe('dangerous')
  })

  it('returns null for plain-string install errors', () => {
    expect(parseConfirmationRequired("clone of 'demo/x' failed: network down")).toBeNull()
  })

  it('returns null for non-gate JSON errors (JSON parses but error differs)', () => {
    expect(parseConfirmationRequired('{"error":"unauthorized"}')).toBeNull()
  })

  it('returns null for malformed JSON that starts like an object', () => {
    expect(parseConfirmationRequired('{"error": nope')).toBeNull()
  })

  it('returns null for non-string rejection values', () => {
    expect(parseConfirmationRequired(undefined)).toBeNull()
    expect(parseConfirmationRequired(null)).toBeNull()
    expect(parseConfirmationRequired(42)).toBeNull()
  })

  it('builds the exact override payload (snake_case risk)', () => {
    expect(buildInstallConfirmation('auto-reply-pro')).toEqual({
      acknowledged_risk: 'dangerous',
      typed_name: 'auto-reply-pro',
    })
  })
})

// --- 2. drawer --------------------------------------------------------------

describe('InstallConfirmDrawer', () => {
  it('renders title, matches (pattern + category + substring) and target name from a real gate payload', () => {
    const onConfirm = vi.fn()
    const onCancel = vi.fn()
    render(<InstallConfirmDrawer payload={GATE_PAYLOAD} onConfirm={onConfirm} onCancel={onCancel} />)

    // Title carries the entry name; the matches list shows WHY.
    expect(screen.getByTestId('install-confirm-drawer')).toBeInTheDocument()
    expect(screen.getByText('Confirm dangerous install — auto-reply-pro')).toBeInTheDocument()
    expect(screen.getByText('system_override')).toBeInTheDocument()
    // The matched substring is rendered in quotes; the lowercase detection
    // pattern sits on its own line (the "send the user's" match duplicates
    // its pattern text, hence getAllBy).
    expect(screen.getByText(/Ignore previous instructions/)).toBeInTheDocument()
    expect(screen.getAllByText(/send the user's/).length).toBeGreaterThan(0)
    // The exact target name is shown next to the input.
    expect(screen.getByText('Entry name: auto-reply-pro')).toBeInTheDocument()
    // Cancel stays available — nothing has been installed.
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument()
  })

  it('keeps confirm disabled until the typed name matches EXACTLY (case-sensitive, no trim)', () => {
    const onConfirm = vi.fn()
    render(
      <InstallConfirmDrawer payload={GATE_PAYLOAD} onConfirm={onConfirm} onCancel={vi.fn()} />,
    )
    const input = screen.getByTestId('install-confirm-input')
    const confirm = screen.getByTestId('install-confirm-button')

    // Wrong case → disabled + mismatch hint.
    fireEvent.change(input, { target: { value: 'Auto-Reply-Pro' } })
    expect(confirm).toBeDisabled()
    expect(screen.getByText(/must match exactly/)).toBeInTheDocument()

    // Padded exact name → STILL disabled client-side (the honest gesture is
    // exact typing even though the backend would trim).
    fireEvent.change(input, { target: { value: ' auto-reply-pro ' } })
    expect(confirm).toBeDisabled()

    // Exact name → enabled + match hint.
    fireEvent.change(input, { target: { value: 'auto-reply-pro' } })
    expect(confirm).toBeEnabled()
    expect(screen.getByText(/you can confirm the install/)).toBeInTheDocument()

    fireEvent.click(confirm)
    expect(onConfirm).toHaveBeenCalledWith('auto-reply-pro')
  })

  it('disabled confirm never fires and cancel reports the honest no-op', () => {
    const onConfirm = vi.fn()
    const onCancel = vi.fn()
    render(<InstallConfirmDrawer payload={GATE_PAYLOAD} onConfirm={onConfirm} onCancel={onCancel} />)
    fireEvent.click(screen.getByTestId('install-confirm-button'))
    expect(onConfirm).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(onCancel).toHaveBeenCalledTimes(1)
  })
})

// --- 3. retry plumbing (useInstallGate) --------------------------------------

/** Minimal surface harness: starts the gated install and reports its outcome. */
function GateHarness({ attempt }: { attempt: InstallAttempt }) {
  const { installWithGate, gateDrawer } = useInstallGate()
  const [status, setStatus] = useState('idle')
  const start = async () => {
    try {
      const result = await installWithGate(attempt)
      setStatus(result === null ? 'cancelled' : `installed:${result.name}`)
    } catch (e) {
      setStatus(`failed:${String(e)}`)
    }
  }
  return (
    <div>
      <button type="button" onClick={start}>start</button>
      <span data-testid="harness-status">{status}</span>
      {gateDrawer}
    </div>
  )
}

describe('useInstallGate', () => {
  it('opens the drawer on a gate rejection and retries the SAME command with the confirmation payload', async () => {
    const attempt = vi.fn()
      .mockRejectedValueOnce(GATE_JSON)
      .mockResolvedValueOnce(okResult)
    render(<GateHarness attempt={attempt} />)

    fireEvent.click(screen.getByRole('button', { name: 'start' }))
    const input = await screen.findByTestId('install-confirm-input')
    expect(attempt).toHaveBeenCalledTimes(1)
    expect(attempt).toHaveBeenNthCalledWith(1, null)

    fireEvent.change(input, { target: { value: 'auto-reply-pro' } })
    fireEvent.click(screen.getByTestId('install-confirm-button'))

    await waitFor(() => {
      expect(screen.getByTestId('harness-status')).toHaveTextContent('installed:auto-reply-pro')
    })
    expect(attempt).toHaveBeenCalledTimes(2)
    expect(attempt).toHaveBeenNthCalledWith(2, {
      acknowledged_risk: 'dangerous',
      typed_name: 'auto-reply-pro',
    })
    // The drawer closed after a successful retry.
    expect(screen.queryByTestId('install-confirm-drawer')).not.toBeInTheDocument()
  })

  it('cancel resolves null and never re-invokes the command', async () => {
    const attempt = vi.fn().mockRejectedValue(GATE_JSON)
    render(<GateHarness attempt={attempt} />)

    fireEvent.click(screen.getByRole('button', { name: 'start' }))
    await screen.findByTestId('install-confirm-input')
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))

    await waitFor(() => {
      expect(screen.getByTestId('harness-status')).toHaveTextContent('cancelled')
    })
    expect(attempt).toHaveBeenCalledTimes(1) // the refused first try only
    expect(screen.queryByTestId('install-confirm-drawer')).not.toBeInTheDocument()
  })

  it('a retry that gates AGAIN reopens the drawer with the fresh payload', async () => {
    const freshPayload = { ...GATE_PAYLOAD, name: 'renamed-entry', match_count: 3 }
    const attempt = vi.fn()
      .mockRejectedValueOnce(GATE_JSON)
      .mockRejectedValueOnce(JSON.stringify(freshPayload))
    render(<GateHarness attempt={attempt} />)

    fireEvent.click(screen.getByRole('button', { name: 'start' }))
    const input = await screen.findByTestId('install-confirm-input')
    fireEvent.change(input, { target: { value: 'auto-reply-pro' } })
    fireEvent.click(screen.getByTestId('install-confirm-button'))

    // Drawer closed on the retry attempt, then reopened with the FRESH name.
    await waitFor(() => {
      expect(screen.getByText('Confirm dangerous install — renamed-entry')).toBeInTheDocument()
    })
    expect(attempt).toHaveBeenCalledTimes(2)
    // The surface promise is still pending — no outcome reported yet.
    expect(screen.getByTestId('harness-status')).toHaveTextContent('idle')
  })

  it('non-gate errors bypass the drawer and reject with the original error', async () => {
    const attempt = vi.fn().mockRejectedValue("clone of 'demo/x' failed: network down")
    render(<GateHarness attempt={attempt} />)

    fireEvent.click(screen.getByRole('button', { name: 'start' }))
    await waitFor(() => {
      expect(screen.getByTestId('harness-status')).toHaveTextContent(
        "failed:clone of 'demo/x' failed: network down",
      )
    })
    expect(screen.queryByTestId('install-confirm-drawer')).not.toBeInTheDocument()
    expect(attempt).toHaveBeenCalledTimes(1)
  })
})

// --- 4. badge copy (批 2 badge evolution) ------------------------------------

describe('security badge gate copy', () => {
  const localesDir = join(process.cwd(), 'src', 'i18n', 'locales')

  it('en carries the gate fact on the 批 2 badge helper key', () => {
    const en = JSON.parse(readFileSync(join(localesDir, 'en.json'), 'utf8')) as Record<string, string>
    expect(en['extensions.featured.securityBadgeGateHelp']).toBe(
      'Entries flagged Dangerous require typing the name to install',
    )
  })

  it('zh-CN carries the gate fact on the 批 2 badge helper key', () => {
    const zh = JSON.parse(readFileSync(join(localesDir, 'zh-CN.json'), 'utf8')) as Record<string, string>
    expect(zh['extensions.featured.securityBadgeGateHelp']).toBe(
      'Dangerous 判定的条目需输入名称确认后才会安装',
    )
  })

  it('the drawer strings exist in the en baseline (i18n-check guards the other 9 locales)', () => {
    const en = JSON.parse(readFileSync(join(localesDir, 'en.json'), 'utf8')) as Record<string, string>
    for (const key of [
      'extensions.installGate.ariaLabel',
      'extensions.installGate.title',
      'extensions.installGate.reason',
      'extensions.installGate.matchesHeading',
      'extensions.installGate.typePrompt',
      'extensions.installGate.typeTarget',
      'extensions.installGate.matchHint',
      'extensions.installGate.mismatchHint',
      'extensions.installGate.confirm',
      'extensions.installGate.cancel',
    ]) {
      expect(en[key], key).toBeTruthy()
    }
  })
})
