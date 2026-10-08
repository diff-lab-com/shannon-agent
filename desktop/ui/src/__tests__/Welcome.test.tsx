import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'
import Welcome, { shouldShowWelcome, markWelcomeSeen, WELCOME_SEEN_KEY } from '@/pages/Welcome'
import { COMPOSER_DRAFT_EVENT, resetPendingComposerDraftsForTests } from '@/lib/composerBridge'
import { I18nProvider } from '@/i18n'

// Mock AppContext to avoid AppProvider's heavy API surface; Welcome only
// needs refreshConfig/refreshStatus/config from the context.
const ctx = vi.hoisted(() => ({
  refreshConfig: vi.fn().mockResolvedValue(undefined),
  refreshStatus: vi.fn().mockResolvedValue(undefined),
  config: { working_dir: '/tmp/test' },
}))
vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ctx,
}))

// Design 01「开始 →」: the hero-composer submit mirrors
// Sidebar.startWithPrompt — it needs useSessions to create the first
// session. Mocked so the page renders without the real AppProvider.
const sessions = vi.hoisted(() => ({
  currentSessionId: null as string | null,
  createSession: vi.fn().mockResolvedValue(undefined),
}))
vi.mock('@/context/SessionContext', () => ({
  useSessions: () => sessions,
}))

// Mock sonner so toast.success/error/warning calls can be asserted.
vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
  },
}))

// P1.2-C: provider save now goes through AddProviderModal which calls
// saveProvider + setActiveProvider internally. testProviderConnection is
// owned by the modal now (covered in AddProviderModal.test.tsx) and is no
// longer surfaced on the Welcome wizard.
vi.mock('@/lib/tauri-api', () => ({
  configure: vi.fn().mockResolvedValue(undefined),
  seedSampleData: vi.fn().mockResolvedValue({ tasks_seeded: 3 }),
  detectProviderFromEnv: vi.fn().mockResolvedValue(null),
  // Office Wave 1 A3' — DocumentsSkillsList probes the host on mount.
  probeHostRuntime: vi.fn().mockResolvedValue({ python3: true, pythonVersion: 'Python 3.12.3', pandoc: false, libreoffice: false }),
  listProviders: vi.fn().mockResolvedValue({ active_provider_id: null, providers: [] }),
  saveProvider: vi.fn().mockResolvedValue({
    active_provider_id: 'anthropic-main',
    providers: [
      {
        id: 'anthropic-main',
        display_name: 'Anthropic',
        kind: 'anthropic',
        has_api_key: false,
      },
    ],
  }),
  setActiveProvider: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue(null),
}))

// Shared across the two describe blocks below — the AddProviderModal defaults
// to `openai-compatible`, which requires a base URL, and refuses submit
// without a label. Pick the Anthropic kind (no base URL required) and fill
// the label so Save fires.
async function saveProviderViaModal() {
  fireEvent.click(screen.getByTestId('welcome-add-provider'))
  // Switch the kind to anthropic via the kind <select> (first option = anthropic).
  const kindSelect = screen.getByRole('combobox') as HTMLSelectElement
  fireEvent.change(kindSelect, { target: { value: 'anthropic' } })
  fireEvent.change(screen.getByPlaceholderText('My GLM key'), {
    target: { value: 'Anthropic' },
  })
  fireEvent.click(screen.getByText('Save'))
}

describe('shouldShowWelcome', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('returns false while config is still loading', () => {
    expect(shouldShowWelcome(true, false)).toBe(false)
  })

  it('returns true when not loading, no provider, no seen flag', () => {
    expect(shouldShowWelcome(false, false)).toBe(true)
  })

  it('returns false when provider is already configured', () => {
    expect(shouldShowWelcome(false, true)).toBe(false)
  })

  it('returns false when seen flag is set even without provider (skip path)', () => {
    window.localStorage.setItem(WELCOME_SEEN_KEY, '1')
    expect(shouldShowWelcome(false, false)).toBe(false)
  })
})

describe('markWelcomeSeen', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('writes the seen flag to localStorage', () => {
    markWelcomeSeen()
    expect(window.localStorage.getItem(WELCOME_SEEN_KEY)).toBe('1')
  })
})

describe('Welcome component — 2-step flow', () => {
  beforeEach(() => {
    window.localStorage.clear()
    vi.mocked(api.detectProviderFromEnv).mockResolvedValue(null)
    sessions.currentSessionId = null
    sessions.createSession.mockClear().mockResolvedValue(undefined)
    resetPendingComposerDraftsForTests()
  })

  function wrap() {
    return render(
      <I18nProvider>
        <MemoryRouter>
          <Welcome />
        </MemoryRouter>
      </I18nProvider>
    )
  }

  // Step 0 — Task
  it('renders task picker as step 1', () => {
    wrap()
    expect(screen.getByText('What will you use Shannon for?')).toBeInTheDocument()
    expect(screen.getByText('Fix the failing tests')).toBeInTheDocument()
    expect(screen.getByText('Review a pull request')).toBeInTheDocument()
    expect(screen.getByText('Summarize a data report')).toBeInTheDocument()
    expect(screen.getByText('Refactor a legacy module')).toBeInTheDocument()
  })

  it('starts with an empty hero composer and a disabled Start button (design 01)', () => {
    wrap()
    const input = screen.getByTestId('welcome-composer-input') as HTMLTextAreaElement
    expect(input.value).toBe('')
    expect(screen.getByTestId('welcome-composer-start')).toBeDisabled()
    // No template picked yet → the model section stays collapsed; the
    // composer is the hero of the screen.
    expect(screen.queryByText('Choose your AI provider')).not.toBeInTheDocument()
  })

  it('fills the hero composer and marks the template pressed when clicked (design 01:141-160)', () => {
    wrap()
    const codeCard = screen.getByRole('button', {
      name: /Locate the broken assertions and propose a patch\./,
    })
    fireEvent.click(codeCard)
    expect(codeCard).toHaveAttribute('aria-pressed', 'true')
    // Click =「填入输入框」: the card's full prompt lands in the composer,
    // ready to edit before「开始 →」.
    expect((screen.getByTestId('welcome-composer-input') as HTMLTextAreaElement).value).toBe(
      'Fix the failing tests in this workspace — locate the broken assertions and propose a patch.',
    )
    expect(screen.getByTestId('welcome-composer-start')).toBeEnabled()
  })

  it('hero-composer submit creates a session, queues the draft, and enters /chat (design 01:122-137)', async () => {
    wrap()
    fireEvent.change(screen.getByTestId('welcome-composer-input'), {
      target: { value: '  fix the flaky checkout test  ' },
    })
    // The draft is parked in the composerBridge pending queue (no ChatInput
    // mounted here) but still dispatches once — same event a subscribed
    // composer would receive.
    const seenDrafts: string[] = []
    const onDraft = (e: Event) => {
      const text = (e as CustomEvent<{ text?: unknown }>).detail?.text
      if (typeof text === 'string') seenDrafts.push(text)
    }
    window.addEventListener(COMPOSER_DRAFT_EVENT, onDraft)
    fireEvent.click(screen.getByTestId('welcome-composer-start'))
    await waitFor(() => {
      // First session created (Sidebar.startWithPrompt contract), then the
      // trimmed text handed to the composer as a DRAFT.
      expect(sessions.createSession).toHaveBeenCalledTimes(1)
      expect(seenDrafts).toEqual(['fix the flaky checkout test'])
      expect(api.seedSampleData).toHaveBeenCalled()
    })
    expect(window.localStorage.getItem(WELCOME_SEEN_KEY)).toBe('1')
    window.removeEventListener(COMPOSER_DRAFT_EVENT, onDraft)
  })

  it('hero-composer submit skips session creation when one is already active', async () => {
    sessions.currentSessionId = 'existing-session'
    wrap()
    fireEvent.change(screen.getByTestId('welcome-composer-input'), {
      target: { value: 'summarize the CSV' },
    })
    fireEvent.click(screen.getByTestId('welcome-composer-start'))
    await waitFor(() => expect(api.seedSampleData).toHaveBeenCalled())
    expect(sessions.createSession).not.toHaveBeenCalled()
  })

  it('does NOT show API key field on step 1 (task picker)', () => {
    wrap()
    expect(screen.queryByLabelText('API key')).not.toBeInTheDocument()
  })

  it('shows Skip button that marks welcome seen', () => {
    wrap()
    const skip = screen.getByRole('button', { name: /skip welcome/i })
    fireEvent.click(skip)
    expect(window.localStorage.getItem(WELCOME_SEEN_KEY)).toBe('1')
  })

  // Step 1 — Model: now a launcher button → AddProviderModal
  it('advances to Model step with Add provider button', () => {
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    expect(screen.getByText('Choose your AI provider')).toBeInTheDocument()
    expect(screen.getByTestId('welcome-add-provider')).toBeInTheDocument()
    // Legacy picker surface is gone.
    expect(screen.queryByText('OpenAI')).not.toBeInTheDocument()
    expect(screen.queryByText('DeepSeek')).not.toBeInTheDocument()
    expect(screen.queryByLabelText('API key')).not.toBeInTheDocument()
  })

  it('Back button on Model section collapses back to task-only screen', () => {
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    expect(screen.getByText('Choose your AI provider')).toBeInTheDocument()
    fireEvent.click(screen.getByText('← Back'))
    expect(screen.queryByText('Choose your AI provider')).not.toBeInTheDocument()
    expect(screen.getByText('What will you use Shannon for?')).toBeInTheDocument()
  })

  it('shows task-aware recommendation in Model subtitle', () => {
    wrap()
    // General picked → recommends Anthropic
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    expect(screen.getByText(/For "Refactor a legacy module", we recommend Anthropic\./)).toBeInTheDocument()
  })

  it('disables Continue on Model step until provider saved or env key detected', () => {
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    const continueButtons = screen.getAllByRole('button', { name: /Continue/ })
    const modelContinue = continueButtons[continueButtons.length - 1]
    expect(modelContinue).toBeDisabled()
    expect(screen.getByText('Click Add provider to continue.')).toBeInTheDocument()
  })

  it('opens AddProviderModal when the Add provider button is clicked', () => {
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    expect(screen.queryByTestId('add-provider-modal')).not.toBeInTheDocument()
    fireEvent.click(screen.getByTestId('welcome-add-provider'))
    expect(screen.getByTestId('add-provider-modal')).toBeInTheDocument()
  })

  it('closes AddProviderModal when the modal cancel button is clicked', () => {
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    fireEvent.click(screen.getByTestId('welcome-add-provider'))
    expect(screen.getByTestId('add-provider-modal')).toBeInTheDocument()
    // Cancel button renders inside the modal — pick the last button labelled
    // "Cancel" on the page (the modal's Cancel), not the model-step Back.
    const cancelBtns = screen.getAllByRole('button', { name: /Cancel/ })
    fireEvent.click(cancelBtns[cancelBtns.length - 1])
    expect(screen.queryByTestId('add-provider-modal')).not.toBeInTheDocument()
  })

  it('calls saveProvider + setActiveProvider when modal saves, then advances to Tools step', async () => {
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    await saveProviderViaModal()
    await waitFor(() => {
      expect(api.saveProvider).toHaveBeenCalled()
      expect(api.setActiveProvider).toHaveBeenCalledWith('anthropic-main')
    })
    // After saving, we land on the Done step (tools use task defaults now).
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
  })

  // Model step — env-key users go straight through to Done (tools are
  // prefilled from the task's recommendations in the 2-step flow).
  it('advances to Done step when env has key for recommended provider', async () => {
    vi.mocked(api.detectProviderFromEnv).mockResolvedValue({
      provider: 'anthropic',
      has_api_key: true,
    })
    wrap()
    // env detection fires on mount; let it resolve.
    await waitFor(() => expect(api.detectProviderFromEnv).toHaveBeenCalled())
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    await waitFor(() => {
      const continueBtns = screen.getAllByRole('button', { name: /Continue/ })
      const modelContinue = continueBtns[continueBtns.length - 1]
      expect(modelContinue).not.toBeDisabled()
    })
    fireEvent.click(screen.getAllByRole('button', { name: /Continue/ }).at(-1)!)
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
  })

  // Step 3 — Done
  it('reaches Done step with summary and shortcuts', async () => {
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
    expect(screen.getByText('Your setup')).toBeInTheDocument()
    expect(screen.getByText('Shortcuts')).toBeInTheDocument()
  })

  // B5-33 (decision 4-B): the summary states a recommendation — it must not
  // claim tools were "enabled", since Welcome never persists tool config.
  it('Done step states the tool recommendation honestly (B5-33)', async () => {
    vi.mocked(api.detectProviderFromEnv).mockResolvedValue({
      provider: 'anthropic',
      has_api_key: true,
    })
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    await waitFor(() => {
      const continueBtns = screen.getAllByRole('button', { name: /Continue/ })
      expect(continueBtns[continueBtns.length - 1]).not.toBeDisabled()
    })
    fireEvent.click(screen.getAllByRole('button', { name: /Continue/ }).at(-1)!)
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
    // General task recommends 2 tools, phrased as a recommendation.
    expect(screen.getByText('2 recommended tools — enable them in Settings')).toBeInTheDocument()
    expect(screen.queryByText(/tools? enabled/i)).not.toBeInTheDocument()
  })

  // 01b — the Done step itemizes the recommendation: one chip per tool with
  // its fixed reason, still phrased as "recommended" (never "enabled"). W5:
  // chips deep-link to Settings → Connections — navigation only, Welcome
  // itself still flips no config (decision 4-B holds).
  it('Done step lists each recommended tool as a chip that deep-links to Settings → Connections', async () => {
    vi.mocked(api.detectProviderFromEnv).mockResolvedValue({
      provider: 'anthropic',
      has_api_key: true,
    })
    // Local wrapper with a stub pane so the chip's navigation is observable
    // (the shared wrap() has no Routes to land on).
    render(
      <I18nProvider>
        <MemoryRouter initialEntries={['/welcome']}>
          <Routes>
            <Route path="*" element={<Welcome />} />
            <Route path="/settings/connections" element={<div data-testid="connections-pane" />} />
          </Routes>
        </MemoryRouter>
      </I18nProvider>
    )
    // Code task → filesystem, git, playwright.
    fireEvent.click(screen.getByRole('button', { name: /Locate the broken assertions and propose a patch\./ }))
    await waitFor(() => {
      const continueBtns = screen.getAllByRole('button', { name: /Continue/ })
      expect(continueBtns[continueBtns.length - 1]).not.toBeDisabled()
    })
    fireEvent.click(screen.getAllByRole('button', { name: /Continue/ }).at(-1)!)
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())

    const chips = screen.getAllByTestId('welcome-done-tool-chip')
    expect(chips).toHaveLength(3)
    expect(screen.getByText('Filesystem')).toBeInTheDocument()
    expect(screen.getByText('Git')).toBeInTheDocument()
    expect(screen.getByText('Playwright')).toBeInTheDocument()
    expect(screen.getByText(/3 recommended tools — enable them in Settings/)).toBeInTheDocument()
    // 01b:156-175 — reasons are template-driven: each one anchors to the
    // chosen task's name ({task} interpolation), not generic copy.
    expect(
      screen.getByText(/"Fix the failing tests" needs to read and write source and test files\./),
    ).toBeInTheDocument()
    // Link-style buttons: settings hint in the tooltip + a trailing chevron
    // affordance; the accessible name still comes from the visible content.
    for (const chip of chips) {
      expect(chip.tagName).toBe('BUTTON')
      expect(chip).toHaveAttribute('title', 'View and enable in Settings')
      expect(within(chip).getByText('chevron_right')).toBeInTheDocument()
    }
    // Clicking deep-links to Settings → Connections without any configure
    // call — recommendation stays read-only (decision 4-B).
    fireEvent.click(chips[0])
    await waitFor(() => expect(screen.getByTestId('connections-pane')).toBeInTheDocument())
    expect(api.configure).not.toHaveBeenCalled()
  })

  it('Done step shows chosen task in summary', async () => {
    wrap()
    // Pick Writing task
    fireEvent.click(screen.getByRole('button', { name: /Comment on the change risks file by file\./ }))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText('Review a pull request')).toBeInTheDocument())
  })

  it('Done step shows Start using Shannon button', async () => {
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByRole('button', { name: /Start using Shannon/ })).toBeInTheDocument())
  })

  it('Stepper labels the two steps', () => {
    wrap()
    const stepper = screen.getByLabelText(/Step 1 of 2: Task/)
    expect(stepper).toBeInTheDocument()
  })

  it('Done step shows advanced mode checkbox unchecked by default', async () => {
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
    const cb = screen.getByLabelText('Enable advanced features') as HTMLInputElement
    expect(cb).toBeInTheDocument()
    expect(cb.checked).toBe(false)
  })

  it('toggles advanced mode checkbox on click', async () => {
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
    const cb = screen.getByLabelText('Enable advanced features') as HTMLInputElement
    fireEvent.click(cb)
    expect(cb.checked).toBe(true)
  })

  it('writes SIDEBAR_MODE_KEY=dev on finish when advanced mode checked', async () => {
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
    fireEvent.click(screen.getByLabelText('Enable advanced features'))
    fireEvent.click(screen.getByRole('button', { name: /Start using Shannon/ }))
    expect(window.localStorage.getItem('shannon-sidebar-mode')).toBe('dev')
  })

  it('does NOT write SIDEBAR_MODE_KEY when advanced mode unchecked', async () => {
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: /Start using Shannon/ }))
    expect(window.localStorage.getItem('shannon-sidebar-mode')).toBeNull()
  })

  it('calls seedSampleData on finish (onboarding sample data)', async () => {
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: /Start using Shannon/ }))
    await waitFor(() => {
      expect(api.seedSampleData).toHaveBeenCalled()
    })
  })

  it('calls seedSampleData on Skip (covers the skip path too)', async () => {
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Skip welcome/ }))
    await waitFor(() => {
      expect(api.seedSampleData).toHaveBeenCalled()
    })
  })

  it('navigates even if seedSampleData rejects', async () => {
    const mockSeed = api.seedSampleData as ReturnType<typeof vi.fn>
    mockSeed.mockRejectedValueOnce(new Error('boom'))
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Skip welcome/ }))
    await waitFor(() => {
      expect(api.seedSampleData).toHaveBeenCalled()
    })
  })
})

describe('Welcome — env provider detection (T7.A)', () => {
  beforeEach(() => {
    window.localStorage.clear()
    vi.mocked(api.detectProviderFromEnv).mockResolvedValue(null)
  })

  function wrap() {
    return render(
      <I18nProvider>
        <MemoryRouter>
          <Welcome />
        </MemoryRouter>
      </I18nProvider>
    )
  }

  it('calls detectProviderFromEnv on mount', async () => {
    wrap()
    await waitFor(() => expect(api.detectProviderFromEnv).toHaveBeenCalled())
  })

  it('shows a persistent BYOK badge for env-detected keys plus the privacy footer (design 01:164-170)', async () => {
    vi.mocked(api.detectProviderFromEnv).mockResolvedValue({
      provider: 'anthropic',
      has_api_key: true,
    })
    wrap()
    // The badge is persistent footer chrome — it stays after the one-shot
    // toast would have expired, and lists the detected key(s).
    const badge = await screen.findByTestId('welcome-env-key-badge')
    expect(badge).toHaveTextContent('Local keys detected:')
    expect(badge).toHaveTextContent('ANTHROPIC ✓')
    // The privacy line is always on screen, detection or not.
    expect(screen.getByTestId('welcome-footer')).toHaveTextContent('~/.shannon')
  })

  it('privacy footer renders even when nothing was detected', async () => {
    wrap()
    await waitFor(() => expect(api.detectProviderFromEnv).toHaveBeenCalled())
    expect(screen.getByTestId('welcome-footer')).toHaveTextContent('~/.shannon')
    expect(screen.queryByTestId('welcome-env-key-badge')).not.toBeInTheDocument()
  })

  it('pre-selects Anthropic when env has ANTHROPIC_API_KEY', async () => {
    vi.mocked(api.detectProviderFromEnv).mockResolvedValue({
      provider: 'anthropic',
      has_api_key: true,
    })
    wrap()
    await waitFor(() => expect(api.detectProviderFromEnv).toHaveBeenCalled())
    // envProviderReady is set; the Step 1 Continue button should be enabled
    // without requiring manual provider setup (task = general, picked above).
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    await waitFor(() => {
      const continueBtns = screen.getAllByRole('button', { name: /Continue/ })
      const modelContinue = continueBtns[continueBtns.length - 1]
      expect(modelContinue).not.toBeDisabled()
    })
  })

  it('toasts when Ollama is detected via env', async () => {
    vi.mocked(api.detectProviderFromEnv).mockResolvedValue({
      provider: 'ollama',
      has_api_key: false,
    })
    wrap()
    await waitFor(() => expect(toast.info).toHaveBeenCalled())
  })

  it('shows fallback error toast when setActiveProvider rejects', async () => {
    vi.mocked(api.setActiveProvider).mockRejectedValueOnce(new Error('activate boom'))
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    fireEvent.click(screen.getByTestId('welcome-add-provider'))
    // Switch kind to anthropic + fill label so the modal can submit.
    const kindSelect = screen.getByRole('combobox') as HTMLSelectElement
    fireEvent.change(kindSelect, { target: { value: 'anthropic' } })
    fireEvent.change(screen.getByPlaceholderText('My GLM key'), {
      target: { value: 'Anthropic' },
    })
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => {
      expect(api.setActiveProvider).toHaveBeenCalled()
      expect(toast.error).toHaveBeenCalledWith(
        expect.stringMatching(/Failed to configure provider/),
        expect.objectContaining({ description: expect.stringMatching(/activate boom/) }),
      )
    })
  })

  // === pickDirectory (Step 3 working-dir picker) ===
  //
  // `open` is mocked globally to return null, so the user-cancel path is
  // the default. We override it per-test to exercise the success + failure
  // branches.

  async function reachDoneStep() {
    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
  }

  it('pickDirectory silently no-ops when the user cancels the picker', async () => {
    // Default `open` mock returns null — treat as cancel.
    const { open } = await import('@tauri-apps/plugin-dialog')
    vi.mocked(open).mockResolvedValueOnce(null)
    await reachDoneStep()
    fireEvent.click(screen.getByRole('button', { name: /Choose folder|Choose another/i }))
    await waitFor(() => expect(open).toHaveBeenCalled())
    // No configure call, no toast — the cancel branch is a no-op.
    expect(api.configure).not.toHaveBeenCalled()
  })

  it('pickDirectory configures working_dir and refreshes config on selection', async () => {
    const { open } = await import('@tauri-apps/plugin-dialog')
    vi.mocked(open).mockResolvedValueOnce('/Users/test/Code/myproj')
    await reachDoneStep()
    fireEvent.click(screen.getByRole('button', { name: /Choose folder/i }))
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'working_dir', value: '/Users/test/Code/myproj' })
      expect(ctx.refreshConfig).toHaveBeenCalled()
    })
  })

  it('pickDirectory surfaces an error toast when configure rejects', async () => {
    const { open } = await import('@tauri-apps/plugin-dialog')
    vi.mocked(open).mockResolvedValueOnce('/Users/test/Code/myproj')
    vi.mocked(api.configure).mockRejectedValueOnce(new Error('write failed'))
    await reachDoneStep()
    fireEvent.click(screen.getByRole('button', { name: /Choose folder/i }))
    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith(
        expect.stringMatching(/Could not save working directory|working dir|Working directory/i),
        expect.objectContaining({ description: expect.stringMatching(/write failed/) }),
      )
    })
  })

  // === handleAddProviderSaved — multiple save passes ===
  //
  // The Welcome wizard is one-shot for new users, but the modal-launcher
  // design lets the user open the modal, save, then return to Model step
  // and save a *different* provider (without ever leaving the wizard).
  // That second save path must refresh state + advance the same way the
  // first one did, even though `providerSaved` was already true.

  it('accepts a second AddProviderModal save and advances the same way', async () => {
    // First save returns one provider; second save returns a different one.
    vi.mocked(api.saveProvider)
      .mockResolvedValueOnce({
        active_provider_id: 'anthropic-main',
        providers: [
          { id: 'anthropic-main', display_name: 'Anthropic', kind: 'anthropic', has_api_key: false },
        ],
      })
      .mockResolvedValueOnce({
        active_provider_id: 'openai-main',
        providers: [
          { id: 'openai-main', display_name: 'OpenAI', kind: 'openai', has_api_key: false },
        ],
      })

    wrap()
    fireEvent.click(screen.getByRole('button', { name: /Refactor a legacy module/ }))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())

    // Walk back to the combined task+model screen (still inside the wizard).
    fireEvent.click(screen.getByText('← Back'))
    fireEvent.click(screen.getByTestId('welcome-add-provider'))

    // Save again — switch kind + label + click Save.
    const kindSelect = screen.getByRole('combobox') as HTMLSelectElement
    fireEvent.change(kindSelect, { target: { value: 'openai' } })
    fireEvent.change(screen.getByPlaceholderText('My GLM key'), {
      target: { value: 'OpenAI' },
    })
    fireEvent.click(screen.getByText('Save'))

    await waitFor(() => {
      // Both saves fired setActiveProvider — the second with the new id.
      expect(api.setActiveProvider).toHaveBeenCalledWith('openai-main')
    })
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
  })
})