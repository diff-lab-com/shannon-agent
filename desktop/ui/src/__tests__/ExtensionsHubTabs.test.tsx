import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import Extensions from '@/pages/Extensions'

function renderWithRoute(path: string) {
  return render(
    <I18nProvider>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/*" element={<Extensions />} />
        </Routes>
      </MemoryRouter>
    </I18nProvider>
  )
}

describe('Extensions hub tabs (2026-09 marketplace simplification)', () => {
  it('renders the two primary tabs (Featured / Installed) only', () => {
    renderWithRoute('/extensions/featured')
    expect(screen.getByText('Featured')).toBeInTheDocument()
    expect(screen.getByText('Installed')).toBeInTheDocument()
    // The old seven-tab taxonomy row is gone: type-specific pages hide
    // behind the 管理 menu instead of competing as top-level tabs.
    expect(screen.queryByText('MCP Servers')).not.toBeInTheDocument()
    expect(screen.queryByText('Skills')).not.toBeInTheDocument()
    expect(screen.queryByText('Data Sources')).not.toBeInTheDocument()
    expect(screen.queryByText('Plugins')).not.toBeInTheDocument()
  })

  it('lists the five type-specific managers inside the Manage menu', () => {
    renderWithRoute('/extensions/featured')
    fireEvent.click(screen.getByRole('button', { name: /Manage/ }))
    expect(screen.getByRole('menuitem', { name: 'MCP Servers' })).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: 'Skills' })).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: 'Agents' })).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: 'Data Sources' })).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: 'Plugins' })).toBeInTheDocument()
  })

  it('renders the marketplace subtitle', () => {
    renderWithRoute('/extensions/featured')
    expect(screen.getByText(/Install MCP servers, skills, agents and data sources/)).toBeInTheDocument()
  })

  it('still renders default search placeholder on featured route', () => {
    renderWithRoute('/extensions/featured')
    expect(screen.getByPlaceholderText('Search extensions...')).toBeInTheDocument()
  })

  it('does not show dead CTA on featured route', () => {
    renderWithRoute('/extensions/featured')
    expect(screen.queryByText(/Create Agent/)).not.toBeInTheDocument()
    expect(screen.queryByText(/Add Source/)).not.toBeInTheDocument()
  })

  // P0-5: the "Create Agent" and "Add Source" CTAs used to navigate to the
  // current page (no-op). They were removed; the install/create flows live
  // inside each tab's content (e.g. the install dialog in Agents.tsx).
  it('does not show dead CTA on agents route', () => {
    renderWithRoute('/extensions/agents')
    expect(screen.queryByText(/Create Agent/)).not.toBeInTheDocument()
  })

  it('does not show dead CTA on datasources route', () => {
    renderWithRoute('/extensions/datasources')
    expect(screen.queryByText(/Add Source/)).not.toBeInTheDocument()
  })
})
