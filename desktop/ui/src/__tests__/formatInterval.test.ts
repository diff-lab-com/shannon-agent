import { describe, expect, it } from 'vitest'
import { formatInterval } from '@/lib/formatInterval'

// Stub translator: mirrors the en.json wording so the bucketing is observable.
const t = (id: string, values?: Record<string, number>): string => {
  switch (id) {
    case 'schedule.hourly':
      return 'Hourly'
    case 'schedule.everySeconds':
      return `Every ${values?.count} seconds`
    case 'schedule.everyMinute':
      return 'Every minute'
    case 'schedule.everyMinutes':
      return `Every ${values?.count} minutes`
    case 'schedule.everyHours':
      return `Every ${values?.count} hours`
    case 'schedule.everyDays':
      return `Every ${values?.count} days`
    default:
      return id
  }
}

describe('formatInterval', () => {
  it('formats seconds below a minute', () => {
    expect(formatInterval(30, t)).toBe('Every 30 seconds')
  })

  it('formats the single minute without a count', () => {
    expect(formatInterval(60, t)).toBe('Every minute')
  })

  it('formats minutes', () => {
    expect(formatInterval(900, t)).toBe('Every 15 minutes')
  })

  it('formats hours and rounds partial hours up to at least 1', () => {
    expect(formatInterval(3600, t)).toBe('Every 1 hours')
    expect(formatInterval(5400, t)).toBe('Every 2 hours')
  })

  it('formats days', () => {
    expect(formatInterval(86400, t)).toBe('Every 1 days')
    expect(formatInterval(172800, t)).toBe('Every 2 days')
  })

  it('falls back to hourly wording for non-positive or invalid input', () => {
    expect(formatInterval(0, t)).toBe('Hourly')
    expect(formatInterval(-5, t)).toBe('Hourly')
    expect(formatInterval(Number.NaN, t)).toBe('Hourly')
  })
})
