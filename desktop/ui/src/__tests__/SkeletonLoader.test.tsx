import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { Skeleton, CardSkeleton, ListSkeleton, MetricsSkeleton, RowSkeleton } from '@/components/SkeletonLoader'

describe('SkeletonLoader', () => {
  it('renders base skeleton with custom class', () => {
    const { container } = render(<Skeleton className="w-10 h-10" />)
    expect(container.firstChild).toHaveClass('animate-pulse', 'w-10', 'h-10')
  })

  it('CardSkeleton renders header and content lines', () => {
    const { container } = render(<CardSkeleton />)
    const skeletons = container.querySelectorAll('.animate-pulse')
    expect(skeletons.length).toBeGreaterThanOrEqual(3)
  })

  it('ListSkeleton renders correct count of items', () => {
    const { container } = render(<ListSkeleton count={5} />)
    const items = container.querySelectorAll('.flex.items-center.gap-md')
    expect(items).toHaveLength(5)
  })

  it('MetricsSkeleton renders 4 metric cards', () => {
    const { container } = render(<MetricsSkeleton />)
    const cards = container.querySelectorAll('.bg-surface-container-lowest')
    expect(cards).toHaveLength(4)
  })

  it('RowSkeleton renders correct count of rows', () => {
    const { container } = render(<RowSkeleton count={6} />)
    const rows = container.querySelectorAll('.flex.items-center.gap-md')
    expect(rows).toHaveLength(6)
  })

  it('RowSkeleton defaults to 4 rows', () => {
    const { container } = render(<RowSkeleton />)
    const rows = container.querySelectorAll('.flex.items-center.gap-md')
    expect(rows).toHaveLength(4)
  })
})
