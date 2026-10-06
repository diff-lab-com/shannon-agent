// Batch E (2026-09-20 delta analysis §批E): the extensions marketplace hub.
//  - E3: the installed icon row hides when nothing is installed and renders
//    one chip per installed addon otherwise
//  - E4: 公开/个人 segmented tabs — public shows the curated vendors grid,
//    personal lists the machine's installed addons

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { MemoryRouter } from 'react-router-dom'
import InstalledIconRow from '@/components/extensions/InstalledIconRow'
import Featured from '@/components/extensions/Featured'

const state = vi.hoisted(() => ({
  installed: [] as Array<{ id: string; kind: string; name: string; enabled: boolean }>,
  vendors: [] as Array<{ slug: string; display_name: string; description: string; icon: string; trust: string; install_kind: { type: string } }>,
}))

vi.mock('@/lib/tauri-api', () => ({
  listInstalledAddons: vi.fn(async () => state.installed),
  listFeaturedVendors: vi.fn(async () => state.vendors),
}))

vi.mock('react-router-dom', async importOriginal => {
  const actual = await importOriginal<object>()
  return {
    ...actual,
    useOutletContext: () => ({ search: '' }),
  }
})

function wrap(ui: React.ReactElement) {
  return (
    <I18nProvider>
      <MemoryRouter>{ui}</MemoryRouter>
    </I18nProvider>
  )
}

describe('InstalledIconRow (E3)', () => {
  beforeEach(() => {
    state.installed = []
  })

  it('hides entirely when nothing is installed', async () => {
    render(wrap(<InstalledIconRow />))
    await waitFor(() => expect(screen.queryByTestId('installed-icon-row')).not.toBeInTheDocument())
  })

  it('renders one chip per installed addon plus overflow', async () => {
    state.installed = Array.from({ length: 3 }, (_, i) => ({
      id: `a${i}`, kind: 'skill', name: `Skill ${i}`, enabled: true,
    }))
    render(wrap(<InstalledIconRow />))
    await screen.findByTestId('installed-icon-row')
    expect(screen.getAllByRole('button')).toHaveLength(3)
    expect(screen.getByTitle('Skill 1 · skill')).toBeInTheDocument()
  })
})

describe('Featured market tabs (E4)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    state.installed = [
      { id: 'my-skill', kind: 'skill', name: 'My Local Skill', enabled: true },
    ]
    state.vendors = [
      {
        slug: 'github',
        display_name: 'GitHub',
        description: 'GitHub connector',
        icon: 'cloud',
        trust: 'official',
        install_kind: { type: 'oauth_remote' },
      },
    ]
  })

  it('defaults to the public tab with the curated vendor grid', async () => {
    render(wrap(<Featured />))
    await screen.findByText('GitHub')
    expect(screen.getByRole('button', { name: 'Public' })).toHaveAttribute('aria-pressed', 'true')
    expect(screen.queryByText('My Local Skill')).not.toBeInTheDocument()
  })

  it('switching to personal lists installed addons instead of vendors', async () => {
    render(wrap(<Featured />))
    await screen.findByText('GitHub')
    fireEvent.click(screen.getByRole('button', { name: 'Personal' }))
    await screen.findByText('My Local Skill')
    expect(screen.queryByText('GitHub')).not.toBeInTheDocument()
  })

  it('shows the empty state on personal when nothing is installed', async () => {
    state.installed = []
    render(wrap(<Featured />))
    await screen.findByText('GitHub')
    fireEvent.click(screen.getByRole('button', { name: 'Personal' }))
    await screen.findByText('No personal extensions yet')
  })
})
