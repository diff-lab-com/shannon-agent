import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import Welcome, { shouldShowWelcome, markWelcomeSeen, WELCOME_SEEN_KEY } from '@/pages/Welcome'

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

vi.mock('@/lib/tauri-api', () => ({
  configure: vi.fn().mockResolvedValue(undefined),
  switchProvider: vi.fn().mockResolvedValue(undefined),
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
      <MemoryRouter>
        <Welcome />
      </MemoryRouter>
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
})
