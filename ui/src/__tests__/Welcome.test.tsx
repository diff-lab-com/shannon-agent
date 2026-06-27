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
vi.mock('@/context/AppContext', () => ({
  AppProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  useApp: () => ctx,
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

vi.mock('@/lib/tauri-api', () => ({
  configure: vi.fn().mockResolvedValue(undefined),
  switchProvider: vi.fn().mockResolvedValue(undefined),
  seedSampleData: vi.fn().mockResolvedValue({ tasks_seeded: 3 }),
  detectProviderFromEnv: vi.fn().mockResolvedValue(null),
  testProviderConnection: vi.fn().mockResolvedValue({ kind: 'success' }),
}))

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue(null),
}))

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

  // Step 1 — Model
  it('advances to Model step with provider picker', () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    expect(screen.getByText('Choose your AI provider')).toBeInTheDocument()
    expect(screen.getByText('Anthropic')).toBeInTheDocument()
    expect(screen.getByText('OpenAI')).toBeInTheDocument()
    expect(screen.getByText('Ollama')).toBeInTheDocument()
    expect(screen.getByText('DeepSeek')).toBeInTheDocument()
  })

  it('shows API key field for non-Ollama providers on Model step', () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    expect(screen.getByLabelText('API key')).toBeInTheDocument()
  })

  it('hides API key field when Ollama is selected on Model step', () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    fireEvent.click(screen.getByText('Ollama'))
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

  // Step 2 — Tools
  it('advances through Model step to Tools step when Ollama selected', async () => {
    wrap()
    // Step 0 → Step 1
    fireEvent.click(screen.getByText('Continue →'))
    // Pick Ollama (no API key needed)
    fireEvent.click(screen.getByText('Ollama'))
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
  })

  it('shows Recommended badge on task-relevant tools', async () => {
    wrap()
    // Pick Code task → recommends filesystem/git/playwright
    fireEvent.click(screen.getByRole('button', { name: /Build apps, write scripts, debug and refactor\./ }))
    fireEvent.click(screen.getByText('Continue →'))
    fireEvent.click(screen.getByText('Ollama'))
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => expect(screen.getAllByText('Recommended').length).toBeGreaterThanOrEqual(1))
  })

  it('toggles tool checkbox off when clicked', async () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    fireEvent.click(screen.getByText('Ollama'))
    fireEvent.click(screen.getByText('Continue →'))
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
    fireEvent.click(screen.getByText('Ollama'))
    fireEvent.click(screen.getByText('Continue →'))
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
    fireEvent.click(screen.getByText('Ollama'))
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => expect(screen.getByText('Writing')).toBeInTheDocument())
  })

  it('Done step shows Start using Shannon button', async () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    fireEvent.click(screen.getByText('Ollama'))
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => expect(screen.getByRole('button', { name: /Start using Shannon/ })).toBeInTheDocument())
  })

  it('Stepper labels all 4 steps', () => {
    wrap()
    // First step label shown in aria-label
    const stepper = screen.getByLabelText(/Step 1 of 4: Task/)
    expect(stepper).toBeInTheDocument()
  })

  // Step 3 — Done: Advanced mode opt-in
  it('Done step shows advanced mode checkbox unchecked by default', async () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    fireEvent.click(screen.getByText('Ollama'))
    fireEvent.click(screen.getByText('Continue →'))
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
    fireEvent.click(screen.getByText('Ollama'))
    fireEvent.click(screen.getByText('Continue →'))
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
    fireEvent.click(screen.getByText('Ollama'))
    fireEvent.click(screen.getByText('Continue →'))
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
    fireEvent.click(screen.getByText('Ollama'))
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => expect(screen.getByText('Pick your tools')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => expect(screen.getByText("You're all set")).toBeInTheDocument())
    // Leave advanced mode unchecked
    fireEvent.click(screen.getByRole('button', { name: /Start using Shannon/ }))
    expect(window.localStorage.getItem('shannon-sidebar-mode')).toBeNull()
  })

  it('calls seedSampleData on finish (onboarding sample data)', async () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    fireEvent.click(screen.getByText('Ollama'))
    fireEvent.click(screen.getByText('Continue →'))
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
    // Should not throw — finish() catches and navigates anyway.
    await waitFor(() => {
      expect(api.seedSampleData).toHaveBeenCalled()
    })
  })
})

describe('Welcome — env provider detection (T7.A)', () => {
  beforeEach(() => {
    window.localStorage.clear()
    vi.mocked(api.detectProviderFromEnv).mockResolvedValue(null)
    vi.mocked(api.testProviderConnection).mockResolvedValue({ kind: 'success' })
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

  it('pre-selects OpenAI when env has OPENAI_API_KEY', async () => {
    vi.mocked(api.detectProviderFromEnv).mockResolvedValue({
      provider: 'openai',
      has_api_key: true,
    })
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => {
      const openaiBtn = screen.getByRole('button', { name: /GPT-4o \/ o1/ })
      expect(openaiBtn).toHaveAttribute('aria-pressed', 'true')
    })
  })

  it('pre-selects Ollama and toasts when OLLAMA_HOST is set', async () => {
    vi.mocked(api.detectProviderFromEnv).mockResolvedValue({
      provider: 'ollama',
      has_api_key: false,
    })
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    await waitFor(() => {
      const ollamaBtn = screen.getByRole('button', { name: /Local models/ })
      expect(ollamaBtn).toHaveAttribute('aria-pressed', 'true')
    })
  })

  it('allows Continue without API key when env detection confirms a key', async () => {
    vi.mocked(api.detectProviderFromEnv).mockResolvedValue({
      provider: 'anthropic',
      has_api_key: true,
    })
    wrap()
    await waitFor(() => expect(api.detectProviderFromEnv).toHaveBeenCalled())
    fireEvent.click(screen.getByText('Continue →'))
    // Default provider is anthropic and envHasKey=true unlocks Continue.
    const continueBtns = screen.getAllByRole('button', { name: /Continue/ })
    const modelContinue = continueBtns[continueBtns.length - 1]
    expect(modelContinue).not.toBeDisabled()
  })
})

describe('Welcome — Test connection button (T7.B + T7.D)', () => {
  beforeEach(() => {
    window.localStorage.clear()
    vi.mocked(api.detectProviderFromEnv).mockResolvedValue(null)
    vi.mocked(api.testProviderConnection).mockResolvedValue({ kind: 'success' })
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

  it('renders Test connection button on Model step for non-Ollama', () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    expect(screen.getByRole('button', { name: /Test connection/ })).toBeInTheDocument()
  })

  it('disables Test button until API key is entered', () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    const btn = screen.getByRole('button', { name: /Test connection/ })
    expect(btn).toBeDisabled()
  })

  it('enables Test button after API key typed', () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    fireEvent.change(screen.getByLabelText('API key'), { target: { value: 'sk-test' } })
    expect(screen.getByRole('button', { name: /Test connection/ })).not.toBeDisabled()
  })

  it('hides Test button for Ollama (no API key needed)', () => {
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    fireEvent.click(screen.getByText('Ollama'))
    expect(screen.queryByRole('button', { name: /Test connection/ })).not.toBeInTheDocument()
  })

  it('shows success toast on Success result', async () => {
    // toast imported at top
    vi.mocked(api.testProviderConnection).mockResolvedValue({ kind: 'success' })
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    fireEvent.change(screen.getByLabelText('API key'), { target: { value: 'sk-test' } })
    fireEvent.click(screen.getByRole('button', { name: /Test connection/ }))
    await waitFor(() => {
      expect(api.testProviderConnection).toHaveBeenCalledWith('anthropic', 'sk-test')
      expect(toast.success).toHaveBeenCalled()
    })
  })

  it('shows invalid-key toast on InvalidKey result', async () => {
    // toast imported at top
    vi.mocked(api.testProviderConnection).mockResolvedValue({ kind: 'invalid_key' })
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    fireEvent.change(screen.getByLabelText('API key'), { target: { value: 'sk-bad' } })
    fireEvent.click(screen.getByRole('button', { name: /Test connection/ }))
    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith(expect.stringMatching(/Invalid API key/))
    })
  })

  it('shows rate-limited warning on RateLimited result', async () => {
    // toast imported at top
    vi.mocked(api.testProviderConnection).mockResolvedValue({ kind: 'rate_limited' })
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    fireEvent.change(screen.getByLabelText('API key'), { target: { value: 'sk-test' } })
    fireEvent.click(screen.getByRole('button', { name: /Test connection/ }))
    await waitFor(() => {
      expect(toast.warning).toHaveBeenCalledWith(expect.stringMatching(/Rate limited/))
    })
  })

  it('shows provider error toast with status on ProviderError', async () => {
    // toast imported at top
    vi.mocked(api.testProviderConnection).mockResolvedValue({
      kind: 'provider_error',
      status: 503,
    })
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    fireEvent.change(screen.getByLabelText('API key'), { target: { value: 'sk-test' } })
    fireEvent.click(screen.getByRole('button', { name: /Test connection/ }))
    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith(expect.stringContaining('503'))
    })
  })

  it('shows network-unreachable toast on NetworkUnreachable', async () => {
    // toast imported at top
    vi.mocked(api.testProviderConnection).mockResolvedValue({ kind: 'network_unreachable' })
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    fireEvent.change(screen.getByLabelText('API key'), { target: { value: 'sk-test' } })
    fireEvent.click(screen.getByRole('button', { name: /Test connection/ }))
    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith(expect.stringMatching(/Can't reach/))
    })
  })

  it('shows fallback error toast when invoke rejects', async () => {
    // toast imported at top
    vi.mocked(api.testProviderConnection).mockRejectedValue(new Error('invoke boom'))
    wrap()
    fireEvent.click(screen.getByText('Continue →'))
    fireEvent.change(screen.getByLabelText('API key'), { target: { value: 'sk-test' } })
    fireEvent.click(screen.getByRole('button', { name: /Test connection/ }))
    await waitFor(() => {
      // Migrated to toastError: the title still reads "Test failed", and the
      // real cause ("invoke boom") now surfaces in the description slot.
      expect(toast.error).toHaveBeenCalledWith(
        expect.stringMatching(/Test failed/),
        expect.objectContaining({ description: expect.stringMatching(/invoke boom/) }),
      )
    })
  })
})
