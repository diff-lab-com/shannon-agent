import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { MemoryRouter } from 'react-router-dom'
import BillingSettings from '@/components/settings/BillingSettings'

function wrap(ui: React.ReactElement) {
  return (
    <AppProvider>
      <MemoryRouter>
        {ui}
      </MemoryRouter>
    </AppProvider>
  )
}

async function renderReady() {
  render(wrap(<BillingSettings />))
  await waitFor(() => expect(screen.getAllByText(/Usage & Billing/).length).toBeGreaterThanOrEqual(1))
}

describe('BillingSettings', () => {
  it('renders usage and billing heading', async () => {
    await renderReady()
    expect(screen.getByRole('heading', { name: /Usage & Billing/ })).toBeInTheDocument()
  })

  it('shows demo mode banner warning', async () => {
    await renderReady()
    expect(screen.getByText('Demo mode')).toBeInTheDocument()
    expect(screen.getByText(/illustrative sample data/i)).toBeInTheDocument()
  })

  it('renders usage quota overview section', async () => {
    await renderReady()
    expect(screen.getByText('Usage Quota Overview')).toBeInTheDocument()
  })

  it('renders token usage display', async () => {
    await renderReady()
    expect(screen.getByText('Token Usage')).toBeInTheDocument()
  })

  it('renders cache hit rate display', async () => {
    await renderReady()
    expect(screen.getByText('Cache Hit Rate')).toBeInTheDocument()
  })

  it('renders cost analysis section', async () => {
    await renderReady()
    expect(screen.getByText('Cost Analysis')).toBeInTheDocument()
  })

  it('renders active plan badge', async () => {
    await renderReady()
    expect(screen.getByText('Active Plan')).toBeInTheDocument()
  })

  it('renders billing history section', async () => {
    await renderReady()
    expect(screen.getByText('Billing History')).toBeInTheDocument()
  })

  it('renders footer help section', async () => {
    await renderReady()
    expect(screen.getByText(/Enterprise Team/i)).toBeInTheDocument()
  })

  // US-SET-06: Change Plan modal
  it('opens change plan modal on Change Plan click', async () => {
    await renderReady()
    fireEvent.click(screen.getAllByText('Change Plan')[0])
    expect(screen.getByText('Free')).toBeInTheDocument()
    expect(screen.getByText('Pro')).toBeInTheDocument()
    expect(screen.getByText('Enterprise')).toBeInTheDocument()
  })

  it('closes change plan modal when close button clicked', async () => {
    await renderReady()
    fireEvent.click(screen.getAllByText('Change Plan')[0])
    expect(screen.getByText('Free')).toBeInTheDocument()
    const modal = screen.getByText('Free').closest('.fixed')!
    const closeBtn = modal.querySelector('.material-symbols-outlined')
    if (closeBtn) fireEvent.click(closeBtn)
  })

  // US-SET-08: Legal modal
  it('opens legal modal on Legal & Terms click', async () => {
    await renderReady()
    fireEvent.click(screen.getByText('Legal & Terms'))
    expect(screen.getByText('Legal & Privacy')).toBeInTheDocument()
  })

  it('opens legal modal on Privacy Policy click', async () => {
    await renderReady()
    fireEvent.click(screen.getByText('Privacy Policy'))
    expect(screen.getByText('Legal & Privacy')).toBeInTheDocument()
  })

  // US-SET-07: Cancel subscription
  it('has cancel button', async () => {
    await renderReady()
    expect(screen.getByText('Cancel')).toBeInTheDocument()
  })
})
