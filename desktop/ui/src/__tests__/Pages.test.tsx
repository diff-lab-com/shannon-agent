import { describe, it, expect } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { AppProvider } from '@/context/AppContext'
import { I18nProvider } from '@/i18n'
import Tasks from '@/pages/Tasks'
import OPC from '@/pages/OPC'

function wrap(ui: React.ReactElement) {
  return (
    <I18nProvider>
      <AppProvider>
        <MemoryRouter>
          {ui}
        </MemoryRouter>
      </AppProvider>
    </I18nProvider>
  )
}

describe('Tasks page', () => {
  it('renders page title', async () => {
    render(wrap(<Tasks />))
    // 2026-09 P0-3: page-level h2 was retired; the global Header carries the
    // page title — here we pin the TasksHeader subtitle as the page's
    // distinctive marker.
    await waitFor(() => expect(screen.getByText(/Create and monitor automations/)).toBeInTheDocument())
  })

  // IA T4 + 2026-10 design parity: primary CTA is「新建」; the
  // background-task entry lives in the split-button dropdown.
  it('renders the New primary CTA', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByRole('button', { name: 'New' })).toBeInTheDocument())
  })

  it('renders empty state when no tasks', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByText('No tasks yet.')).toBeInTheDocument())
  })

  // G4a (design 04 右列): the side rail's mini month calendar card replaced
  // the old navigable widget (fixed current month).
  it('renders the side-rail mini month calendar', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByTestId('tasks-side-calendar')).toBeInTheDocument())
  })
})

describe('OPC page', () => {
  it('renders kanban header', async () => {
    render(wrap(<OPC />))
    await waitFor(() => expect(screen.getByText('KANBAN')).toBeInTheDocument())
  })

  it('renders kanban columns', async () => {
    render(wrap(<OPC />))
    await waitFor(() => {
      expect(screen.getByText('Queued')).toBeInTheDocument()
      expect(screen.getByText('In Progress')).toBeInTheDocument()
      expect(screen.getByText('Completed')).toBeInTheDocument()
    })
  })

  it('renders agent swarm section', async () => {
    render(wrap(<OPC />))
    await waitFor(() => expect(screen.getByText('Active Agents')).toBeInTheDocument())
  })
})
