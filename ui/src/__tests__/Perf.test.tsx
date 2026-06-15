// Tests for W12 Perf developer panel — pure analyzer logic + UI smoke.

import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import Perf, { analyzeTracingJson, parseDurationMs, percentile } from '@/pages/Perf'

describe('parseDurationMs', () => {
  it('parses ns, µs, ms, s units', () => {
    expect(parseDurationMs('500ns')).toBeCloseTo(0.0005, 4)
    expect(parseDurationMs('250µs')).toBeCloseTo(0.25, 2)
    expect(parseDurationMs('10ms')).toBe(10)
    expect(parseDurationMs('2s')).toBe(2000)
    expect(parseDurationMs('42')).toBe(42) // unit-less defaults to ms
  })

  it('returns 0 for unparseable input', () => {
    expect(parseDurationMs(undefined)).toBe(0)
    expect(parseDurationMs('not-a-duration')).toBe(0)
  })
})

describe('percentile', () => {
  it('returns the p50 and p95 from a sorted array', () => {
    const sorted = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    expect(percentile(sorted, 50)).toBe(5)
    expect(percentile(sorted, 95)).toBe(10)
  })

  it('returns 0 for empty input', () => {
    expect(percentile([], 50)).toBe(0)
  })
})

describe('analyzeTracingJson', () => {
  const closeEvent = (
    spanName: string,
    busyMs: number,
    attributes: Record<string, unknown> = {},
    toolName?: string,
  ) => ({
    timestamp: '2026-06-14T10:00:00Z',
    level: 'INFO',
    target: 'shannon_desktop::commands::foo',
    fields: {
      message: 'close',
      'time.busy': `${busyMs}ms`,
      ...(toolName ? { tool_name: toolName } : {}),
    },
    spans: [{ name: spanName, attributes }],
  })

  it('counts unique commands and computes p50/p95/max', () => {
    // Each Tauri command's #[instrument] emits its own span name, so different
    // command functions produce different span names. Attributes carry args.
    const lines = [
      JSON.stringify(closeEvent('send_message', 10, { command: 'send_message' })),
      JSON.stringify(closeEvent('send_message', 20, { command: 'send_message' })),
      JSON.stringify(closeEvent('list_tasks', 5, { command: 'list_tasks' })),
    ].join('\n')
    const r = analyzeTracingJson(lines)
    expect(r.commands).toHaveLength(2)
    const send = r.commands.find(c => c.name === 'send_message')!
    expect(send.count).toBe(2)
    expect(send.max).toBe(20)
    const list = r.commands.find(c => c.name === 'list_tasks')!
    expect(list.count).toBe(1)
  })

  it('extracts slow top 10 spans sorted by duration', () => {
    const lines = [
      JSON.stringify(closeEvent('span', 1)),
      JSON.stringify(closeEvent('span', 50)),
      JSON.stringify(closeEvent('span', 5)),
    ].join('\n')
    const r = analyzeTracingJson(lines)
    expect(r.slowTop10).toHaveLength(3)
    expect(r.slowTop10[0].durationMs).toBe(50)
    expect(r.slowTop10[1].durationMs).toBe(5)
    expect(r.slowTop10[2].durationMs).toBe(1)
  })

  it('counts tool calls by tool_name field', () => {
    const lines = [
      JSON.stringify(closeEvent('span', 1, {}, 'bash')),
      JSON.stringify(closeEvent('span', 1, {}, 'bash')),
      JSON.stringify(closeEvent('span', 1, {}, 'read_file')),
    ].join('\n')
    const r = analyzeTracingJson(lines)
    expect(r.toolCounts).toEqual([
      { name: 'bash', count: 2 },
      { name: 'read_file', count: 1 },
    ])
  })

  it('counts parse errors on malformed lines and skips non-JSON', () => {
    const lines = [
      '{ not valid',
      'random text',
      JSON.stringify(closeEvent('command', 1)),
    ].join('\n')
    const r = analyzeTracingJson(lines)
    expect(r.parseErrors).toBe(1) // only the "{ not valid" line attempts to parse
    expect(r.totalEvents).toBe(1)
  })
})

describe('Perf UI', () => {
  it('shows the empty state before analysis', () => {
    render(<Perf />)
    expect(screen.getByText('No analysis yet')).toBeInTheDocument()
  })

  it('shows error when Analyze clicked with empty input', () => {
    render(<Perf />)
    fireEvent.click(screen.getByRole('button', { name: 'Analyze' }))
    expect(screen.getByRole('alert')).toHaveTextContent('Paste at least one line')
  })

  it('renders summary and slow-top after analyzing input', () => {
    render(<Perf />)
    const ev = {
      timestamp: '2026-06-14T10:00:00Z',
      level: 'INFO',
      target: 'shannon_desktop::commands::foo',
      fields: { message: 'close', 'time.busy': '42ms' },
      spans: [{ name: 'command', attributes: { command: 'send_message' } }],
    }
    fireEvent.change(screen.getByLabelText(/Trace JSON/), { target: { value: JSON.stringify(ev) } })
    fireEvent.click(screen.getByRole('button', { name: 'Analyze' }))
    expect(screen.getByTestId('events-parsed')).toHaveTextContent('1')
    expect(screen.getByTestId('unique-commands')).toHaveTextContent('1')
    expect(screen.getByTestId('slow-duration')).toHaveTextContent('42.00ms')
  })

  it('Clear button resets to empty state', () => {
    render(<Perf />)
    fireEvent.change(screen.getByLabelText(/Trace JSON/), { target: { value: 'garbage' } })
    fireEvent.click(screen.getByRole('button', { name: 'Clear' }))
    expect(screen.getByText('No analysis yet')).toBeInTheDocument()
  })
})
