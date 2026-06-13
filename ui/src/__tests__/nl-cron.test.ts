// P3.1 nl-cron parser tests.

import { describe, it, expect } from 'vitest'
import { parseNlCron } from '@/lib/nl-cron'

describe('parseNlCron — every-N-minutes patterns', () => {
  it('parses "every 15 minutes"', () => {
    expect(parseNlCron('every 15 minutes')).toEqual({
      expression: '*/15 * * * *',
      description: 'Every 15 minutes',
    })
  })
  it('parses "every minute"', () => {
    expect(parseNlCron('every minute')?.expression).toBe('* * * * *')
  })
  it('normalizes "every 1 minutes" to every-minute', () => {
    expect(parseNlCron('every 1 minutes')?.expression).toBe('* * * * *')
  })
  it('rejects out-of-range minutes', () => {
    expect(parseNlCron('every 99 minutes')).toBeNull()
  })
})

describe('parseNlCron — every-N-hours', () => {
  it('parses "every 6 hours"', () => {
    expect(parseNlCron('every 6 hours')?.expression).toBe('0 */6 * * *')
  })
  it('parses "hourly"', () => {
    expect(parseNlCron('hourly')?.expression).toBe('0 * * * *')
  })
  it('rejects out-of-range hours', () => {
    expect(parseNlCron('every 99 hours')).toBeNull()
  })
})

describe('parseNlCron — daily / time shortcuts', () => {
  it('parses "daily at 09:30"', () => {
    expect(parseNlCron('daily at 09:30')?.expression).toBe('30 9 * * *')
  })
  it('parses "daily at 9am"', () => {
    expect(parseNlCron('daily at 9am')?.expression).toBe('0 9 * * *')
  })
  it('parses "daily at 9pm"', () => {
    expect(parseNlCron('daily at 9pm')?.expression).toBe('0 21 * * *')
  })
  it('parses "midnight"', () => {
    expect(parseNlCron('midnight')?.expression).toBe('0 0 * * *')
  })
  it('parses "noon"', () => {
    expect(parseNlCron('noon')?.expression).toBe('0 12 * * *')
  })
})

describe('parseNlCron — weekday patterns', () => {
  it('parses "weekdays at 9am"', () => {
    expect(parseNlCron('weekdays at 9am')?.expression).toBe('0 9 * * 1-5')
  })
  it('parses "weekday mornings at 8:30"', () => {
    expect(parseNlCron('weekday mornings at 8:30')?.expression).toBe('30 8 * * 1-5')
  })
})

describe('parseNlCron — weekly / day-of-week', () => {
  it('parses "weekly on monday at 10am"', () => {
    expect(parseNlCron('weekly on monday at 10am')?.expression).toBe('0 10 * * 1')
  })
  it('parses "weekly on friday at 17:00"', () => {
    expect(parseNlCron('weekly on friday at 17:00')?.expression).toBe('0 17 * * 5')
  })
  it('parses "every tuesday at 11:30"', () => {
    expect(parseNlCron('every tuesday at 11:30')?.expression).toBe('30 11 * * 2')
  })
  it('handles capitalized weekday token', () => {
    expect(parseNlCron('weekly on Mon at 9am')?.expression).toBe('0 9 * * 1')
  })
  it('rejects unknown weekday in weekly form', () => {
    expect(parseNlCron('weekly on someday at 9am')).toBeNull()
  })
})

describe('parseNlCron — monthly', () => {
  it('parses "monthly on day 1 at 8am"', () => {
    expect(parseNlCron('monthly on day 1 at 8am')?.expression).toBe('0 8 1 * *')
  })
  it('parses "monthly on 15 at 09:00"', () => {
    expect(parseNlCron('monthly on 15 at 09:00')?.expression).toBe('0 9 15 * *')
  })
  it('rejects out-of-range day', () => {
    expect(parseNlCron('monthly on 32 at 8am')).toBeNull()
  })
})

describe('parseNlCron — edge cases', () => {
  it('returns null for empty input', () => {
    expect(parseNlCron('')).toBeNull()
    expect(parseNlCron('   ')).toBeNull()
  })
  it('returns null for unrecognized pattern', () => {
    expect(parseNlCron('do the thing')).toBeNull()
    expect(parseNlCron('every full moon')).toBeNull()
  })
  it('is case-insensitive', () => {
    expect(parseNlCron('DAILY AT 9AM')?.expression).toBe('0 9 * * *')
    expect(parseNlCron('Weekly On Monday At 10AM')?.expression).toBe('0 10 * * 1')
  })
  it('description is provided on every match', () => {
    const out = parseNlCron('daily at 9am')
    expect(out).not.toBeNull()
    expect(typeof out!.description).toBe('string')
    expect(out!.description.length).toBeGreaterThan(0)
  })
})
