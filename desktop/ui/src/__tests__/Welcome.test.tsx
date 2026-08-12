import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'
import Welcome, { shouldShowWelcome, markWelcomeSeen, WELCOME_SEEN_KEY } from '@/pages/Welcome'
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

describe('Welcome component — 4-step flow', () => {
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

  // Step 0 — Task
  it('renders task picker as step 1', () => {
    wrap()
    expect(screen.getByText('What will you use Shannon for?')).toBeInTheDocument()
    expect(screen.getByText('Code')).toBeInTheDocument()
    expect(screen.getByText('Writing')).toBeInTheDocument()
    expect(screen.getByText('Research')).toBeInTheDocument()
    expect(screen.getByText('General')).toBeInTheDocument()
  })

  it('defaults task selection to General', () => {
    wrap()
    expect(screen.getByRole('button', { name: /General/ })).toHaveAttribute('aria-pressed', 'true')
  })

  it('marks task as pressed when clicked', () => {
    wrap()
    const codeCard = screen.getByRole('button', { name: /Build apps, write scripts, debug and refactor\./ })
    fireEvent.click(codeCard)
    expect(codeCard).toHaveAttribute('aria-pressed', 'true')
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
    fireEvent.click(screen.getByText('Continue →'))
    expect(screen.getByText('Choose your AI provider')).toBeInTheDocument()
    expect(screen.getByTestId('welcome-add-provider')).toBeInTheDocument()
    // Legacy picker surface is gone.
    expect(screen.queryByText('OpenAI')).not.toBeInTheDocument()
    expect(screen.queryByText('DeepSeek')).not.toBeInTheDocument()
    expect(screen.queryByLabelText('API key')).not.toBeInTheDocument()
  })

  it('Back button on Model step returns to Task step', () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    fireEvent.click(screen.getByText('← Back'))
    expect(screen.getByText('What will you use Shannon for?')).toBeInTheDocument()
  })

  it('shows task-aware recommendation in Model subtitle', () => {
    wrap()
    // Default task is 'general' → recommends Anthropic
    fireEvent.click(screen.getByText('Continue →'))
    expect(screen.getByText(/For General, we recommend Anthropic\./)).toBeInTheDocument()
  })

  it('disables Continue on Model step until provider saved or env key detected', () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    const continueButtons = screen.getAllByRole('button', { name: /Continue/ })
    const modelContinue = continueButtons[continueButtons.length - 1]
    expect(modelContinue).toBeDisabled()
    expect(screen.getByText('Click Add provider to continue.')).toBeInTheDocument()
  })

  it('opens AddProviderModal when the Add provider button is clicked', () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    expect(screen.queryByTestId('add-provider-modal')).not.toBeInTheDocument()
    fireEvent.click(screen.getByTestId('welcome-add-provider'))
    expect(screen.getByTestId('add-provider-modal')).toBeInTheDocument()
  })

  it('closes AddProviderModal when the modal cancel button is clicked', () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
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
    fireEvent.click(screen.getByText('Continue →'))
    await saveProviderViaModal()
    await waitFor(() => {
      expect(api.saveProvider).toHaveBeenCalled()
      expect(api.setActiveProvider).toHaveBeenCalledWith('anthropic-main')
    })
    // After saving, we land on the Tools step (Step 2).
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
  })

  // Step 2 — Tools (unchanged surface; reached by the modal-save path above)
  it('advances through Model step to Tools step when env has key for recommended provider', async () => {
    vi.mocked(api.detectProviderFromEnv).mockResolvedValue({
      provider: 'anthropic',
      has_api_key: true,
    })
    wrap()
    // env detection fires on mount; let it resolve.
    await waitFor(() => expect(api.detectProviderFromEnv).toHaveBeenCalled())
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => {
      const continueBtns = screen.getAllByRole('button', { name: /Continue/ })
      const modelContinue = continueBtns[continueBtns.length - 1]
      expect(modelContinue).not.toBeDisabled()
    })
    fireEvent.click(screen.getAllByRole('button', { name: /Continue/ }).at(-1)!)
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
  })

  it('shows Recommended badge on task-relevant tools', async () => {
    wrap()
    // Pick Code task → recommends filesystem/git/playwright
    fireEvent.click(screen.getByRole('button', { name: /Build apps, write scripts, debug and refactor\./ }))
    fireEvent.click(screen.getByText('Continue →'))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
    expect(screen.getAllByText('Recommended').length).toBeGreaterThanOrEqual(1)
  })

  it('toggles tool checkbox off when clicked', async () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
    const fsCheckbox = await waitFor(() => screen.getByLabelText('Enable Filesystem') as HTMLInputElement)
    // Initially checked for general task (filesystem recommended)
    const initial = fsCheckbox.checked
    fireEvent.click(fsCheckbox)
    expect(fsCheckbox.checked).toBe(!initial)
  })

  // Step 3 — Done
  it('reaches Done step with summary and shortcuts', async () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
    expect(screen.getByText('Your setup')).toBeInTheDocument()
    expect(screen.getByText('Shortcuts')).toBeInTheDocument()
  })

  it('Done step shows chosen task in summary', async () => {
    wrap()
    // Pick Writing task
    fireEvent.click(screen.getByRole('button', { name: /Draft docs, articles, posts, and emails\./ }))
    fireEvent.click(screen.getByText('Continue →'))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => expect(screen.getByText('Writing')).toBeInTheDocument())
  })

  it('Done step shows Start using Shannon button', async () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => expect(screen.getByRole('button', { name: /Start using Shannon/ })).toBeInTheDocument())
  })

  it('Stepper labels all 4 steps', () => {
    wrap()
    const stepper = screen.getByLabelText(/Step 1 of 4: Task/)
    expect(stepper).toBeInTheDocument()
  })

  it('Done step shows advanced mode checkbox unchecked by default', async () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
    const cb = screen.getByLabelText('Enable advanced features') as HTMLInputElement
    expect(cb).toBeInTheDocument()
    expect(cb.checked).toBe(false)
  })

  it('toggles advanced mode checkbox on click', async () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
    const cb = screen.getByLabelText('Enable advanced features') as HTMLInputElement
    fireEvent.click(cb)
    expect(cb.checked).toBe(true)
  })

  it('writes SIDEBAR_MODE_KEY=dev on finish when advanced mode checked', async () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
    fireEvent.click(screen.getByLabelText('Enable advanced features'))
    fireEvent.click(screen.getByRole('button', { name: /Start using Shannon/ }))
    expect(window.localStorage.getItem('shannon-sidebar-mode')).toBe('dev')
  })

  it('does NOT write SIDEBAR_MODE_KEY when advanced mode unchecked', async () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: /Start using Shannon/ }))
    expect(window.localStorage.getItem('shannon-sidebar-mode')).toBeNull()
  })

  it('calls seedSampleData on finish (onboarding sample data)', async () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Continue →'))
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

  it('pre-selects Anthropic when env has ANTHROPIC_API_KEY', async () => {
    vi.mocked(api.detectProviderFromEnv).mockResolvedValue({
      provider: 'anthropic',
      has_api_key: true,
    })
    wrap()
    await waitFor(() => expect(api.detectProviderFromEnv).toHaveBeenCalled())
    // envHasKey is set; the Step 1 Continue button should be enabled when
    // the env-detected provider matches the recommended one (default task =
    // general → anthropic).
    fireEvent.click(screen.getByText('Continue →'))
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
    fireEvent.click(screen.getByText('Continue →'))
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
    fireEvent.click(screen.getByText('Continue →'))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Continue →'))
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
    fireEvent.click(screen.getByText('Continue →'))
    await saveProviderViaModal()
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())

    // Walk back to Model step (still inside the wizard).
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
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
  })
})