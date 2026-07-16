import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, act } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import SkillProposalsToast from '../SkillProposalsToast'

const mockUseTauriEvent = vi.fn()

vi.mock('@/hooks/useTauriEventValidated', () => ({
  useTauriEventValidated: (...args: unknown[]) => mockUseTauriEvent(...args),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

type Handler = (event: { payload: { pending_count: number } }) => void

function renderToast(onOpenReview = vi.fn()) {
  let eventHandler: Handler | null = null
  mockUseTauriEvent.mockImplementation((_event, handler: Handler) => {
    eventHandler = handler
  })
  render(
    <I18nProvider>
      <SkillProposalsToast onOpenReview={onOpenReview} />
    </I18nProvider>,
  )
  return {
    emit: (count: number) =>
      act(() => {
        eventHandler?.({ payload: { pending_count: count } })
      }),
    onOpenReview,
  }
}

describe('SkillProposalsToast', () => {
  it('renders nothing when count is zero', () => {
    const { emit } = renderToast()
    emit(0)
    expect(screen.queryByText(/skill suggestion/i)).not.toBeInTheDocument()
  })

  it('renders count when event fires', () => {
    const { emit } = renderToast()
    emit(2)
    expect(screen.getByText(/2 skill suggestions/i)).toBeInTheDocument()
  })

  it('calls onOpenReview when View button is clicked', () => {
    const { emit, onOpenReview } = renderToast()
    emit(1)

    fireEvent.click(screen.getByText('View'))
    expect(onOpenReview).toHaveBeenCalledTimes(1)
  })

  it('hides toast when Close button is clicked', () => {
    const { emit } = renderToast()
    emit(1)

    fireEvent.click(screen.getByText('Close'))
    expect(screen.queryByText(/skill suggestion/i)).not.toBeInTheDocument()
  })
})
