// Wave-2 journey #22 — voice-input（方案 §9.4 J22 / G12）. NIGHTLY-ONLY
// (D8): playwright.config.ts testIgnore keeps this family out of the PR
// gate; playwright.chat-nightly.config.ts re-includes it.
//
// Status before this journey: zero voice mocks (five STT commands were
// UNMOCKED_ALLOWLIST). Task 6 adds deterministic handlers (handlers.ts):
// transcribe_audio (cloud — the DEFAULT provider ChatInput picks),
// transcribe_audio_local(_base64) and the get/save_voice_local_config pair
// (the config command path that flips ChatInput to the local provider), so
// the real UI path is exercisable: MicButton → MediaRecorder (fake device)
// → base64 → transcribe command → transcript MERGED into the composer
// draft (never auto-sent — the trust contract this journey pins).
//
// Browser seams: MediaRecorder/getUserMedia need the fake media device
// flags (per-describe test.use — the config files stay untouched); the
// unsupported branch is simulated by removing navigator.mediaDevices in an
// init script (the exact seam lib/voice's isSupported() checks).
// Note: VoiceOrb is deliberately role=presentation aria-hidden (no testid
// — ChatInput is another task's file), so the recording state is pinned via
// the button's aria-pressed/label flip plus the orb's icon glyph.
import { expect, test, type Page } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { attachConsoleWatchdog, expectNoConsoleErrors } from './helpers/watchdog'
import { loadChatScriptObject } from './helpers/scriptLoader'
import { mockSnapshot } from './helpers/knownIssues'

const CLOUD_TRANSCRIPT = 'Cloud transcript: stand up the staging cluster.'
const LOCAL_TRANSCRIPT = 'Local transcript: whisper ran on device.'

// Fake mic: MediaRecorder + getUserMedia are gated on real device seams.
// launchOptions cannot live in a describe group (forces a new worker), so
// this is file-wide — the unsupported-environment test below removes the
// navigator.mediaDevices seam in an init script, so it stays "unsupported"
// no matter what the launch flags provide. The config files stay untouched.
test.use({
  launchOptions: {
    args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'],
  },
})

/** Unarmed demo boot on /chat (no script needed for composer-level flows). */
async function gotoChat(page: Page): Promise<ChatPage> {
  attachConsoleWatchdog(page)
  await page.goto('/chat')
  const chat = new ChatPage(page)
  await expect(chat.composer()).toBeVisible({ timeout: 15_000 })
  return chat
}

/**
 * Probe when the provider's MediaRecorder actually STARTS. The recording
 * STATE is synchronous (useVoice flips it before the permission resolves),
 * but getUserMedia resolution is async — clicking stop before the recorder
 * exists is a provider-level no-op (stop-before-start has no UI contract),
 * so the stop click must wait for the real start. Counts instrument a
 * subclass installed before app code; the app never sees the difference.
 */
async function armRecorderStartProbe(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const win = window as unknown as { __recProbe?: { started: number }; MediaRecorder?: typeof MediaRecorder }
    const Orig = win.MediaRecorder
    if (!Orig) return
    class Probe extends Orig {
      start(...args: Parameters<MediaRecorder['start']>): void {
        win.__recProbe = { started: (win.__recProbe?.started ?? 0) + 1 }
        super.start(...args)
      }
    }
    win.MediaRecorder = Probe as typeof MediaRecorder
  })
}

async function waitForRecorderStarted(page: Page): Promise<void> {
  await page.waitForFunction(() => (window as unknown as { __recProbe?: { started: number } }).__recProbe?.started != null
    && (window as unknown as { __recProbe: { started: number } }).__recProbe.started > 0
    , null, { timeout: 10_000 })
}

test.describe('voice input (journey #22, nightly-only) — supported environment', () => {
  test('mic renders, records, and merges the cloud transcript into the draft WITHOUT sending', async ({ page }) => {
    await armRecorderStartProbe(page)
    const chat = await gotoChat(page)
    const mic = page.getByRole('button', { name: 'Start voice recording' })

    // Supported environment → the MicButton mounts (ChatInput.tsx gates on
    // voice.supported; without a provider there is NO button at all).
    await expect(mic).toBeVisible()

    // A pre-existing draft proves the MERGE semantics (append with a space,
    // not overwrite).
    await chat.composer().fill('Draft prefix:')

    await mic.click()
    // Recording state is synchronous (useVoice sets it before the mic
    // permission resolves): pressed + orb visible + label flips to stop.
    const stopMic = page.getByRole('button', { name: 'Stop recording' })
    await expect(stopMic).toHaveAttribute('aria-pressed', 'true')
    // The orb's mic glyph: while recording, the MicButton's own icon flips
    // to stop_circle, so this exact glyph can only be the VoiceOrb's.
    await expect(page.getByText('mic', { exact: true })).toBeVisible()

    // Wait for the REAL recorder start (see armRecorderStartProbe) before
    // stopping — a stop that lands before getUserMedia resolves is a
    // provider no-op and the journey would hang in "transcribing".
    await waitForRecorderStarted(page)
    await stopMic.click()
    // Stop → transcribing → the transcript lands in the composer draft.
    await expect(chat.composer()).toHaveValue(`Draft prefix: ${CLOUD_TRANSCRIPT}`, { timeout: 15_000 })
    // Trust contract: a transcript NEVER auto-sends — no turn consumed, no
    // NEW bubble (the unarmed demo boot carries the demo conversation, so
    // the anchor is the boot count, not zero).
    expect((await mockSnapshot(page)).sentTurns).toBe(0)
    const bubblesAtBoot = await chat.messageCount()
    await expect(page.locator('[data-message-index]')).toHaveCount(bubblesAtBoot)

    await expectNoConsoleErrors(page)
  })

  test('the mic is disabled while this session streams', async ({ page }) => {
    // Inline script object (no YAML needed for a two-step hold): chunks
    // then a waitFor:'ui' park keeps isQuerying true for the assertion.
    await loadChatScriptObject(page, {
      name: 'voice-input-stream-hold',
      description: 'hold a stream open so the mic-disabled state is observable',
      seed: { config: { hasKey: true }, sessions: [{ id: 'voice-hold-sess', title: 'Voice hold', messages: [] }] },
      turns: [
        {
          user: 'stream while I dictate',
          script: [
            { event: 'query:text', chunks: ['still streaming '], chunkDelayMs: 50 },
            { waitFor: 'ui' },
          ],
        },
      ],
    })
    const chat = new ChatPage(page)
    await expect(chat.composer()).toBeVisible({ timeout: 15_000 })

    const mic = page.getByRole('button', { name: 'Start voice recording' })
    await expect(mic).toBeEnabled()

    await chat.send('stream while I dictate')
    await chat.expectStreamingCursor()
    await expect(mic).toBeDisabled()

    // Settle: resume → completed → the mic re-arms.
    await page.evaluate(() => (window as unknown as { __shannonMock: { control: { resume(): void } } }).__shannonMock.control.resume())
    await expect(chat.bubbleAt(1)).toContainText('still streaming', { timeout: 15_000 })
    await expect(page.getByRole('button', { name: 'Start voice recording' })).toBeEnabled()

    await expectNoConsoleErrors(page)
  })

  test('flipping the voice_local config switch routes the next recording through the local provider', async ({ page }) => {
    await armRecorderStartProbe(page)
    // The Advanced section (the local-voice card's home) is dev-mode gated.
    await page.addInitScript(() => {
      window.localStorage.setItem('shannon-sidebar-mode', 'dev')
    })
    const chat = await gotoChat(page)

    // The real user path: Settings → Advanced → Speech-to-text card →
    // "Enable local voice". The save goes through save_voice_local_config,
    // mirrors into get_config and re-emits config-updated; coming back to
    // /chat remounts ChatInput, whose useVoice rebuilds the provider from
    // the refreshed config — no reload anywhere.
    await page.getByRole('link', { name: 'Settings' }).click()
    await page.getByRole('link', { name: 'Advanced' }).click()
    const enableSwitch = page.getByRole('switch', { name: 'Enable local voice' })
    await expect(enableSwitch).toBeVisible({ timeout: 15_000 })
    await enableSwitch.click()
    await expect(page.getByText('Local voice settings saved')).toBeVisible()
    // The save re-emits config-updated; AppContext's async refreshConfig
    // must commit BEFORE ChatInput remounts, or useVoice pins the provider
    // from the stale config on its first render. Anchor the wait on the
    // toast's own auto-dismiss (sonner's ~4s lifecycle) instead of a blind
    // timeout — the refresh lands well inside it.
    await expect(page.getByText('Local voice settings saved')).toHaveCount(0, { timeout: 10_000 })

    await page.getByRole('link', { name: 'Chat' }).click()
    await expect(chat.composer()).toBeVisible({ timeout: 15_000 })
    await page.getByRole('button', { name: 'Start voice recording' }).click()
    await waitForRecorderStarted(page)
    await page.getByRole('button', { name: 'Stop recording' }).click()
    // The mock returns a distinct text per transcribe command, so the LOCAL
    // string in the draft proves the config command path end-to-end.
    await expect(chat.composer()).toHaveValue(LOCAL_TRANSCRIPT, { timeout: 15_000 })
    expect((await mockSnapshot(page)).sentTurns).toBe(0)

    await expectNoConsoleErrors(page, [
      // Known demo gaps on the /settings/advanced page (unrelated to the
      // voice path under test): the cloud-STT card's save endpoint and the
      // CLI-install probe have no browser equivalent (both still live in
      // the tripwire's UNMOCKED_ALLOWLIST), and @tauri-apps/api/app's
      // getVersion() goes through the plugin:app|* namespace, which demo
      // mode does not implement.
      /get_cli_install_status/,
      /save_stt_config/,
      /plugin:app\|version/,
    ])
  })
})

test.describe('voice input (journey #22, nightly-only) — capture seam removed (FINDING pinned)', () => {
  test('even with getUserMedia removed the mic renders and yields the stub transcript (supported-gate is dead code)', async ({ page }) => {
    // Remove the exact seam useVoice's provider checks: no mediaDevices →
    // createRemoteProvider().isSupported() false. PINNED CURRENT STATE (not
    // the intended behavior): the FACTORY (lib/voice/factory.ts) falls back
    // to the stub provider for any unsupported kind, and the stub reports
    // isSupported() === true — so ChatInput's `voice.supported` gate can
    // never close and the MicButton renders in EVERY environment, with the
    // stub's canned transcript flowing into the draft. Expected behavior
    // ("no provider → no mic button", B4 P2-5) is a finding for the fix
    // PR: the factory must surface the unsupported provider (or the gate
    // must read the real kind), and this test flips to toHaveCount(0).
    await page.addInitScript(() => {
      Object.defineProperty(navigator, 'mediaDevices', { get: () => undefined, configurable: true })
    })
    await gotoChat(page)

    await expect(page.getByRole('button', { name: 'Start voice recording' })).toHaveCount(1)

    // The stub path is what an "unsupported" user actually gets today.
    await page.getByRole('button', { name: 'Start voice recording' }).click()
    await page.getByRole('button', { name: 'Stop recording' }).click()
    await expect(page.getByRole('textbox', { name: 'Message' })).toHaveValue(
      'This is a stub transcript. Real STT backend not configured.',
      { timeout: 15_000 },
    )
    expect((await mockSnapshot(page)).sentTurns).toBe(0)

    await expectNoConsoleErrors(page)
  })
})
