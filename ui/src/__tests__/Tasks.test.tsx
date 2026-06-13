import { describe, it, expect, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { MemoryRouter } from 'react-router-dom'
import Tasks from '@/pages/Tasks'

function wrap(ui: React.ReactElement) {
  return (
    <AppProvider>
      <MemoryRouter>
        {ui}
      </MemoryRouter>
    </AppProvider>
  )
}

describe('Tasks page', () => {
  it('renders scheduled tasks heading', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByText('Scheduled Tasks')).toBeInTheDocument())
  })

  it('renders new background task button', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByText('New Background Task')).toBeInTheDocument())
  })

  it('renders empty state when no tasks', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByText('No tasks yet.')).toBeInTheDocument())
  })

  it('renders calendar schedule widget', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByText('Schedule')).toBeInTheDocument())
  })

  it('renders task completion section', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByText(/Task Completion/i)).toBeInTheDocument())
  })
})
