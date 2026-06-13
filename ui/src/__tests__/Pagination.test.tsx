import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { Pagination } from '@/components/ui/pagination'

describe('Pagination', () => {
  it('returns null for single page', () => {
    const { container } = render(<Pagination page={1} totalPages={1} onPageChange={() => {}} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders page buttons for multiple pages', () => {
    render(<Pagination page={1} totalPages={3} onPageChange={() => {}} />)
    expect(screen.getByText('1')).toBeInTheDocument()
    expect(screen.getByText('2')).toBeInTheDocument()
    expect(screen.getByText('3')).toBeInTheDocument()
  })

  it('disables previous on first page', () => {
    render(<Pagination page={1} totalPages={3} onPageChange={() => {}} />)
    const prevBtn = screen.getByLabelText('Previous page')
    expect(prevBtn).toBeDisabled()
  })

  it('disables next on last page', () => {
    render(<Pagination page={3} totalPages={3} onPageChange={() => {}} />)
    const nextBtn = screen.getByLabelText('Next page')
    expect(nextBtn).toBeDisabled()
  })

  it('calls onPageChange when clicking next', () => {
    const onPageChange = vi.fn()
    render(<Pagination page={1} totalPages={3} onPageChange={onPageChange} />)
    fireEvent.click(screen.getByLabelText('Next page'))
    expect(onPageChange).toHaveBeenCalledWith(2)
  })

  it('calls onPageChange when clicking a page number', () => {
    const onPageChange = vi.fn()
    render(<Pagination page={1} totalPages={3} onPageChange={onPageChange} />)
    fireEvent.click(screen.getByText('3'))
    expect(onPageChange).toHaveBeenCalledWith(3)
  })

  it('highlights active page', () => {
    render(<Pagination page={2} totalPages={4} onPageChange={() => {}} />)
    const activeBtn = screen.getByText('2')
    expect(activeBtn.className).toContain('bg-primary')
  })

  it('shows ellipsis for many pages', () => {
    render(<Pagination page={5} totalPages={10} onPageChange={() => {}} />)
    const ellipses = screen.getAllByText('…')
    expect(ellipses.length).toBeGreaterThanOrEqual(1)
  })
})
