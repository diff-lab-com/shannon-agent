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
    await waitFor(() => expect(screen.getByText('Scheduled Tasks')).toBeInTheDocument())
  })

  it('renders new task button', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByText('New Background Task')).toBeInTheDocument())
  })

  it('renders empty state when no tasks', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByText('No tasks yet.')).toBeInTheDocument())
  })

  it('renders calendar widget', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByText('Schedule')).toBeInTheDocument())
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
