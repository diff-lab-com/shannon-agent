// wave-2 composer/input L1 — slashComposer.journeys.test.tsx
//
// Independent file (parallel-PR discipline — the shared suite stays
// untouched). Covers the J15 brief items that are cheaper and sharper at
// the component/pure-function layer than in a browser:
//
//   - parse rules, table-driven: `/name args` never executes (goes to the
//     model as text), unknown `/tokens` (pasted paths) stay plain text,
//     case-insensitive bare `/NAME` resolves, aliases are equivalent;
//   - the needsSession error card actually RENDERS (the command-level
//     behavior is pinned in slashCommands.test.ts; this pins the card);
//   - /dream's remaining skipped toasts (throttled, in-progress — the
//     disabled case and the done path already live in slashCommands.test.ts);
//   - the composer-draft bridge contract pin (J18): a RAW
//     `shannon:composer-draft` event with no mounted subscriber is lost by
//     design — which is exactly why surface components must call
//     pushComposerDraft (the pending-queue flush contract itself is covered
//     in composerBridge.test.tsx).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, renderHook, act, screen } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import SlashResultCard from '@/components/chat/SlashResultCard'
import { parseSlashInput, SLASH_COMMANDS, type SlashCommand } from '@/lib/slash/commands'
import { resetPendingComposerDraftsForTests, useComposerDraftListener } from '@/lib/composerBridge'
import * as api from '@/lib/tauri-api'

// /dream reports through sonner toasts; mock the module like the existing
// slashCommands.test.ts so assertions don't depend on jsdom rendering.
const toastSuccess = vi.hoisted(() => vi.fn())
const toastInfo = vi.hoisted(() => vi.fn())
vi.mock('sonner', () => ({
  toast: {
    success: toastSuccess,
    info: toastInfo,
    error: vi.fn(),
    message: vi.fn(),
  },
}))

function mustResolve(command: SlashCommand | null): SlashCommand {
  if (!command) throw new Error('expected a resolved command')
  return command
}

describe('slash parse rules — table-driven (J15)', () => {
  it.each([
    // Bare commands resolve (case-insensitive).
    { input: '/goal', resolves: 'goal' },
    { input: '  /DIFF  ', resolves: 'diff' },
    { input: '/Detect-Skills', resolves: 'detect-skills' },
    // Aliases are equivalent to the canonical command.
    { input: '/save', resolves: 'export' },
    { input: '/clear', resolves: 'new' },
    { input: '/usage-session', resolves: 'cost' },
    { input: '/agents', resolves: 'extensions' },
    // `/name args` never executes — the whole input goes to the model.
    { input: '/goal ship it', resolves: null },
    { input: '/diff now', resolves: null },
    { input: '/dream 7', resolves: null },
    // Unknown /tokens (typically pasted absolute paths) stay plain text.
    { input: '/usr/local/bin', resolves: null },
    { input: '/totally-unknown', resolves: null },
    // Not a slash query at all.
    { input: 'plain question', resolves: null },
    { input: '', resolves: null },
    { input: 'hello /world', resolves: null },
  ])('$input → $resolves', ({ input, resolves }) => {
    const cmd = parseSlashInput(input)
    expect(cmd?.name ?? null).toBe(resolves)
  })

  it('every registered command (and alias) round-trips through the parser', () => {
    for (const cmd of SLASH_COMMANDS) {
      expect(parseSlashInput(`/${cmd.name}`)?.name).toBe(cmd.name)
      for (const alias of cmd.aliases ?? []) {
        expect(parseSlashInput(`/${alias}`)?.name).toBe(cmd.name)
      }
    }
  })
})

describe('needsSession — the error card renders (J15)', () => {
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <I18nProvider>{children}</I18nProvider>
  )

  it('shows the calm notice with its failure title, dismissible', () => {
    render(<SlashResultCard result={{ kind: 'error', messageKey: 'slash.needsSession' }} onDismiss={() => {}} />, { wrapper })
    // The message names the fix (open/start a session), not the raw error.
    expect(screen.getByText('Open or start a chat session first')).toBeInTheDocument()
    expect(screen.getByText('Command failed')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Dismiss' })).toBeInTheDocument()
  })
})

describe('/dream skipped toasts — remaining skip reasons (J15)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it.each([
    { reason: 'throttled' as const, messageKey: 'slash.toast.dream.throttled' },
    { reason: 'in-progress' as const, messageKey: 'slash.toast.dream.inProgress' },
  ])('$reason surfaces the honest skip toast instead of counts', async ({ reason, messageKey }) => {
    vi.mocked(api.runDreamPass).mockResolvedValue({
      skipped_reason: reason,
      scanned_sessions: 0,
      projects: [],
      merge_proposed: 0,
      remove_proposed: 0,
      add_proposed: 0,
      candidates_detected: 0,
      candidates_refined: 0,
      proposal_ids: [],
      report_path: null,
      duration_ms: 0,
    })
    await mustResolve(parseSlashInput('/dream')).run({
      navigate: vi.fn(),
      sessionId: 'sess-1',
      workingDir: '/repo',
      sessions: [],
      createSession: vi.fn().mockResolvedValue(undefined),
      compactSession: vi.fn(),
      showResult: vi.fn(),
      toastError: vi.fn(),
      t: (id: string) => id,
    })
    // The command awaits its API before toasting.
    await vi.waitFor(() => {
      expect(toastInfo).toHaveBeenCalledWith(messageKey)
    })
    expect(toastSuccess).not.toHaveBeenCalled()
  })
})

// ── J18 contract pin: raw events have no stash of their own ─────────────────

describe('composer-draft bridge — raw event contract pin (J18)', () => {
  beforeEach(() => {
    resetPendingComposerDraftsForTests()
  })

  it('a raw CustomEvent with no mounted subscriber is lost — surfaces must call pushComposerDraft', () => {
    // No listener mounted: dispatching the RAW event (the shape e2e tests
    // use against a LIVE composer) delivers nothing and stashes nothing.
    act(() => {
      window.dispatchEvent(new CustomEvent('shannon:composer-draft', { detail: { text: 'unheard draft' } }))
    })

    // The subscriber mounting later must NOT receive the raw-event text:
    // only pushComposerDraft parks into the pending queue.
    const delivered = vi.fn()
    renderHook(() => useComposerDraftListener(delivered))
    expect(delivered).not.toHaveBeenCalled()
  })
})
