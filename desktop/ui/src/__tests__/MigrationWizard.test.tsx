import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import MigrationWizard from '@/components/migration/MigrationWizard'
import type * as api from '@/lib/tauri-api'
import type { MigrationScanResult } from '@/lib/tauri-api'

// Entry-point tests render Welcome / GeneralSettings, which pull the full
// tauri-api + CatalogContext surface. Partial-mock the api module (the real
// wrappers still hit the globally-mocked invoke) and stub the catalog.
vi.mock('@/lib/tauri-api', async importOriginal => {
  const actual = await importOriginal<typeof api>()
  return {
    ...actual,
    detectProviderFromEnv: vi.fn().mockResolvedValue({ provider: 'anthropic', has_api_key: true }),
    configure: vi.fn().mockResolvedValue(undefined),
    seedSampleData: vi.fn().mockResolvedValue({ tasks_seeded: 3 }),
    listProviders: vi.fn().mockResolvedValue({ active_provider_id: null, providers: [] }),
    setActiveProvider: vi.fn().mockResolvedValue(undefined),
  }
})

vi.mock('@/context/CatalogContext', async importOriginal => {
  const actual = await importOriginal<Record<string, unknown>>()
  return {
    ...actual,
    useCatalog: () => ({
      refreshConfig: vi.fn().mockResolvedValue(undefined),
      refreshStatus: vi.fn().mockResolvedValue(undefined),
      config: { working_dir: '/tmp/test' },
    }),
  }
})

// ─── Fixtures ───────────────────────────────────────────────────────────────

const SCAN_RESULT: MigrationScanResult = {
  source: 'claude-code',
  items: [
    {
      id: 'claude-code:mcp:github',
      kind: 'mcp',
      name: 'github',
      sourcePath: '~/.claude.json',
      targetPath: '~/.shannon/desktop/mcp-servers.json',
      conflict: 'overwrite',
      sizeHint: 5120,
    },
    {
      id: 'claude-code:skill:commit',
      kind: 'skill',
      name: 'commit',
      sourcePath: '~/.claude/skills/commit',
      targetPath: '~/.shannon/skills/commit',
      conflict: 'none',
      sizeHint: 482,
    },
    {
      id: 'claude-code:command:deploy',
      kind: 'command',
      name: 'deploy',
      sourcePath: '~/.claude/commands/deploy.md',
      targetPath: '~/.shannon/commands/deploy.md',
      conflict: 'skip-existing',
      sizeHint: 311,
    },
    {
      id: 'claude-code:memory:project-memory',
      kind: 'memory',
      name: 'CLAUDE.md',
      sourcePath: 'CLAUDE.md',
      targetPath: '~/.shannon/memories',
      conflict: 'none',
      sizeHint: 1024,
    },
  ],
  notFound: ['settings — ~/.claude/settings.json', 'commands — ~/.claude/commands'],
  errors: [{ path: '~/.claude/settings.json', error: 'invalid JSON' }],
}

function makeBackend(overrides: Partial<typeof api> = {}) {
  return {
    migrationScan: vi.fn().mockResolvedValue(SCAN_RESULT),
    migrationPreview: vi.fn().mockResolvedValue({
      perItem: SCAN_RESULT.items.map(a => ({
        id: a.id,
        diffSummary: `preview for ${a.name}`,
      })),
    }),
    migrationApply: vi.fn().mockResolvedValue({ imported: 3, skipped: 1, failed: [] }),
    ...overrides,
  } as unknown as typeof api
}

// `render` auto-wraps with I18nProvider (see __tests__/setup.ts).

async function reachReview(backend: typeof api) {
  render(<MigrationWizard open onClose={() => {}} apiOverride={backend} />)
  fireEvent.click(screen.getByTestId('migration-source-claude-code'))
  await waitFor(() =>
    expect(screen.getByTestId('migration-check-claude-code:skill:commit')).toBeInTheDocument(),
  )
}

describe('MigrationWizard', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders nothing when closed', () => {
    const { container } = render(<MigrationWizard open={false} onClose={() => {}} />)
    expect(container).toBeEmptyDOMElement()
  })

  // ─── Source step ──────────────────────────────────────────────────────────

  it('offers both sources and scans the picked one', async () => {
    const backend = makeBackend()
    render(<MigrationWizard open onClose={() => {}} apiOverride={backend} />)
    expect(screen.getByRole('radio', { name: /Claude Code/ })).toBeInTheDocument()
    expect(screen.getByRole('radio', { name: /ZCode/ })).toBeInTheDocument()
    fireEvent.click(screen.getByTestId('migration-source-zcode'))
    await waitFor(() => expect(backend.migrationScan).toHaveBeenCalledWith('zcode'))
  })

  it('shows a scanning state before results arrive', async () => {
    const backend = makeBackend()
    let resolveScan!: (v: MigrationScanResult) => void
    vi.mocked(backend.migrationScan).mockImplementation(
      () => new Promise(resolve => { resolveScan = resolve }),
    )
    render(<MigrationWizard open onClose={() => {}} apiOverride={backend} />)
    fireEvent.click(screen.getByTestId('migration-source-claude-code'))
    expect(await screen.findByTestId('migration-scanning')).toBeInTheDocument()
    resolveScan(SCAN_RESULT)
    await waitFor(() => expect(screen.getByTestId('migration-review')).toBeInTheDocument())
  })

  it('shows an empty-state message when nothing was found', async () => {
    const backend = makeBackend({
      migrationScan: vi.fn().mockResolvedValue({
        source: 'zcode',
        items: [],
        notFound: ['skills — ~/.zcode/skills'],
        errors: [],
      }),
    })
    render(<MigrationWizard open onClose={() => {}} apiOverride={backend} />)
    fireEvent.click(screen.getByTestId('migration-source-zcode'))
    await waitFor(() => expect(screen.getByText(/Nothing to import/)).toBeInTheDocument())
  })

  it('survives a rejected scan with an empty review list', async () => {
    const backend = makeBackend({ migrationScan: vi.fn().mockRejectedValue(new Error('boom')) })
    render(<MigrationWizard open onClose={() => {}} apiOverride={backend} />)
    fireEvent.click(screen.getByTestId('migration-source-claude-code'))
    await waitFor(() => expect(screen.getByText(/Nothing to import/)).toBeInTheDocument())
  })

  // ─── Review step ──────────────────────────────────────────────────────────

  it('groups the checklist by kind with conflict badges', async () => {
    await reachReview(makeBackend())
    expect(screen.getByRole('heading', { name: 'MCP servers' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Skills' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Commands' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Project memory' })).toBeInTheDocument()
    expect(screen.getByTestId('migration-badge-claude-code:mcp:github')).toHaveTextContent('Conflict')
    expect(screen.getByTestId('migration-badge-claude-code:skill:commit')).toHaveTextContent('New')
    expect(screen.getByTestId('migration-badge-claude-code:command:deploy')).toHaveTextContent(
      'Identical',
    )
  })

  it('selects everything by default and supports select-all / clear-all', async () => {
    await reachReview(makeBackend())
    const applyBtn = screen.getByTestId('migration-apply')
    expect(applyBtn).toHaveTextContent(/4/)
    fireEvent.click(screen.getByRole('button', { name: 'Clear all' }))
    expect(applyBtn).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'Select all' }))
    expect(applyBtn).toHaveTextContent(/4/)
  })

  it('excludes unchecked items from the apply call', async () => {
    const backend = makeBackend()
    await reachReview(backend)
    fireEvent.click(screen.getByTestId('migration-check-claude-code:command:deploy'))
    fireEvent.click(screen.getByTestId('migration-apply'))
    await waitFor(() => expect(backend.migrationApply).toHaveBeenCalled())
    const items = vi.mocked(backend.migrationApply).mock.calls[0][1] as api.MigrationItemInput[]
    expect(items.map(i => i.id)).not.toContain('claude-code:command:deploy')
    expect(items).toHaveLength(3)
  })

  it('expands conflicting rows to show the diff summary', async () => {
    await reachReview(makeBackend())
    const previewFor = (id: string) => screen.queryByTestId(`migration-preview-${id}`)
    // Not expanded initially.
    expect(previewFor('claude-code:mcp:github')).not.toBeInTheDocument()
    fireEvent.click(screen.getByTestId('migration-expand-claude-code:mcp:github'))
    expect(previewFor('claude-code:mcp:github')).toHaveTextContent('preview for github')
    expect(screen.getByTestId('migration-expand-claude-code:mcp:github')).toHaveAttribute(
      'aria-expanded',
      'true',
    )
    // Non-conflicting items have no expander.
    expect(screen.queryByTestId('migration-expand-claude-code:skill:commit')).not.toBeInTheDocument()
  })

  it('sends the rename default only for conflicting items', async () => {
    const backend = makeBackend()
    await reachReview(backend)
    fireEvent.click(screen.getByTestId('migration-apply'))
    await waitFor(() => expect(backend.migrationApply).toHaveBeenCalled())
    const items = vi.mocked(backend.migrationApply).mock.calls[0][1] as api.MigrationItemInput[]
    const github = items.find(i => i.id === 'claude-code:mcp:github')
    expect(github?.conflict).toBe('rename')
    expect(items.find(i => i.id === 'claude-code:skill:commit')?.conflict).toBeUndefined()
  })

  it('lets the user pick overwrite or skip for a conflicting item', async () => {
    const backend = makeBackend()
    await reachReview(backend)
    fireEvent.click(screen.getByTestId('migration-expand-claude-code:mcp:github'))
    const select = screen.getByTestId(
      'migration-conflict-claude-code:mcp:github',
    ) as HTMLSelectElement
    expect(select.value).toBe('rename')
    fireEvent.change(select, { target: { value: 'overwrite' } })
    fireEvent.click(screen.getByTestId('migration-apply'))
    await waitFor(() => expect(backend.migrationApply).toHaveBeenCalled())
    const items = vi.mocked(backend.migrationApply).mock.calls[0][1] as api.MigrationItemInput[]
    expect(items.find(i => i.id === 'claude-code:mcp:github')?.conflict).toBe('overwrite')
  })

  it('lists not-found slots and scan errors in a collapsible section', async () => {
    await reachReview(makeBackend())
    const details = screen.getByTestId('migration-notfound')
    expect(details).toHaveTextContent('Checked but not found')
    expect(details).toHaveTextContent('settings — ~/.claude/settings.json')
    expect(details).toHaveTextContent('~/.claude/settings.json — invalid JSON')
  })

  // ─── Apply + result ───────────────────────────────────────────────────────

  it('renders the result summary with counts and an aria-live status', async () => {
    await reachReview(makeBackend())
    fireEvent.click(screen.getByTestId('migration-apply'))
    const status = await screen.findByRole('status')
    expect(status).toHaveAttribute('aria-live', 'polite')
    expect(status).toHaveTextContent('3 imported')
    expect(status).toHaveTextContent('1 skipped')
    expect(screen.getByTestId('migration-done')).toBeInTheDocument()
  })

  it('lists failures when the backend reports them', async () => {
    const backend = makeBackend({
      migrationApply: vi.fn().mockResolvedValue({
        imported: 1,
        skipped: 1,
        failed: [{ id: 'claude-code:mcp:github', error: 'store unwritable' }],
      }),
    })
    await reachReview(backend)
    fireEvent.click(screen.getByTestId('migration-apply'))
    await waitFor(() =>
      expect(screen.getByTestId('migration-result-failures')).toBeInTheDocument(),
    )
    expect(screen.getByText(/store unwritable/)).toBeInTheDocument()
    expect(screen.getByText('1 failed')).toBeInTheDocument()
  })

  it('shows an all-failed result when the apply command itself rejects', async () => {
    const backend = makeBackend({ migrationApply: vi.fn().mockRejectedValue(new Error('no backend')) })
    await reachReview(backend)
    fireEvent.click(screen.getByTestId('migration-apply'))
    await waitFor(() => expect(screen.getByText('1 failed')).toBeInTheDocument())
    expect(screen.getByText(/no backend/)).toBeInTheDocument()
  })

  // ─── a11y ─────────────────────────────────────────────────────────────────

  it('exposes a modal dialog with an accessible name', async () => {
    render(<MigrationWizard open onClose={() => {}} />)
    expect(screen.getByRole('dialog', { name: 'Import from another tool' })).toHaveAttribute(
      'aria-modal',
      'true',
    )
  })

  it('closes on Escape outside busy phases', async () => {
    const onClose = vi.fn()
    render(<MigrationWizard open onClose={onClose} />)
    fireEvent.keyDown(screen.getByTestId('migration-wizard'), { key: 'Escape' })
    expect(onClose).toHaveBeenCalled()
  })
})

// ─── Entry points (P1-6: Welcome + Settings reuse the same component) ──────

describe('MigrationWizard entry points', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    window.localStorage.clear()
  })

  it('opens from Settings → General ("Import from other tools")', async () => {
    const { AppProvider } = await import('@/context/AppContext')
    const { default: GeneralSettings } = await import('@/components/settings/GeneralSettings')
    render(
      <AppProvider>
        <MemoryRouter>
          <GeneralSettings />
        </MemoryRouter>
      </AppProvider>,
    )
    expect(screen.queryByTestId('migration-wizard')).not.toBeInTheDocument()
    fireEvent.click(screen.getByTestId('settings-migration-open'))
    expect(screen.getByRole('dialog', { name: 'Import from another tool' })).toBeInTheDocument()
    // The same wizard: source picker is the first step.
    expect(screen.getByTestId('migration-source-claude-code')).toBeInTheDocument()
  })

  it('opens from the Welcome final step via the migration entry card', async () => {
    const { default: Welcome } = await import('@/pages/Welcome')
    render(
      <MemoryRouter>
        <Welcome />
      </MemoryRouter>,
    )
    // Walk the flow: general task → model (env key pre-detected) → done.
    fireEvent.click(screen.getByRole('button', { name: /A bit of everything/ }))
    fireEvent.click(screen.getAllByRole('button', { name: /Continue/ })[0])
    await waitFor(() => {
      const modelContinue = screen
        .getAllByRole('button', { name: /Continue/ })
        .at(-1)!
      expect(modelContinue).not.toBeDisabled()
    })
    fireEvent.click(screen.getAllByRole('button', { name: /Continue/ }).at(-1)!)
    await waitFor(() =>
      expect(screen.getByTestId('welcome-migration-entry')).toBeInTheDocument(),
    )
    expect(screen.queryByTestId('migration-wizard')).not.toBeInTheDocument()
    fireEvent.click(screen.getByTestId('welcome-migration-open'))
    expect(screen.getByRole('dialog', { name: 'Import from another tool' })).toBeInTheDocument()
  })
})
