import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { Chart, parseChartSpec } from '@/components/chat/Chart'

describe('parseChartSpec', () => {
  it('parses a valid bar spec', () => {
    const spec = parseChartSpec('{"type":"bar","data":[{"label":"a","value":1}]}')
    expect(spec?.type).toBe('bar')
    expect(spec?.data).toHaveLength(1)
  })

  it('returns null for invalid JSON', () => {
    expect(parseChartSpec('not json')).toBeNull()
  })

  it('returns null for missing data array', () => {
    expect(parseChartSpec('{"type":"bar"}')).toBeNull()
  })

  it('returns null for unsupported chart type', () => {
    expect(parseChartSpec('{"type":"scatter","data":[]}')).toBeNull()
  })
})

describe('Chart component', () => {
  it('renders a chart title and SVG for bar type', () => {
    render(
      <Chart
        spec={{
          type: 'bar',
          title: 'Sales by Quarter',
          data: [
            { label: 'Q1', value: 10 },
            { label: 'Q2', value: 20 },
          ],
        }}
      />,
    )
    expect(screen.getByText('Sales by Quarter')).toBeInTheDocument()
    expect(document.querySelector('svg')).not.toBeNull()
  })

  it('renders pie chart with percentage labels', () => {
    const { container } = render(
      <Chart
        spec={{
          type: 'pie',
          title: 'Share',
          data: [
            { label: 'A', value: 25 },
            { label: 'B', value: 75 },
          ],
        }}
      />,
    )
    expect(container.querySelector('svg')).not.toBeNull()
    expect(screen.getByText('25%')).toBeInTheDocument()
    expect(screen.getByText('75%')).toBeInTheDocument()
  })

  it('renders line chart polyline when multiple points', () => {
    const { container } = render(
      <Chart
        spec={{
          type: 'line',
          data: [
            { label: 'a', value: 1 },
            { label: 'b', value: 2 },
            { label: 'c', value: 3 },
          ],
        }}
      />,
    )
    expect(container.querySelector('polyline')).not.toBeNull()
  })

  it('renders error message when data is empty', () => {
    render(<Chart spec={{ type: 'bar', data: [] }} />)
    expect(screen.getByText(/needs at least one data point/i)).toBeInTheDocument()
  })
})
