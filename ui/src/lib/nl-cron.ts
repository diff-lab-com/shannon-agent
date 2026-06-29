// nl-cron — Phase D P3.1 deliverable.
//
// Frontend-only natural language → cron expression parser. Handles the common
// patterns we expect users to type. Falls back to null when no pattern matches
// so the caller can prompt the user to type a literal cron expression.
//
// Recognized patterns:
//   "every N minutes"           → */N * * * *
//   "every N hours"             → 0 */N * * *
//   "every minute"              → * * * * *
//   "hourly"                    → 0 * * * *
//   "daily at HH:MM"            → M H * * *
//   "daily at HH"               → 0 H * * *
//   "weekly on DAY at HH:MM"    → M H * * DOW
//   "weekday mornings at HH"    → 0 H * * 1-5
//   "every DAY at HH:MM"        → M H * * DOW
//   "every DAYOFWEEK at HH:MM"  → M H * * DOW
//   "monthly on day N at HH:MM" → M H N * *
//   "midnight" / "noon"         → literal time shortcuts

export interface CronDescription {
  /** react-intl message id under the `schedule.*` namespace. */
  id: string
  /** ICU placeholder values for {@link id}. */
  values?: Record<string, string | number>
}

export interface NlCronResult {
  expression: string
  /** Translatable restatement of the parsed schedule, for confirmation UI.
   *  Render via `intl.formatMessage({ id: description.id }, description.values)`. */
  description: CronDescription
}

const WEEKDAYS: Record<string, number> = {
  sunday: 0, sun: 0,
  monday: 1, mon: 1,
  tuesday: 2, tue: 2, Tues: 2,
  wednesday: 3, wed: 3,
  thursday: 4, thu: 4, Thur: 4, Thurs: 4,
  friday: 5, fri: 5,
  saturday: 6, sat: 6,
}

function weekdayTok(s: string): number | null {
  const lc = s.toLowerCase()
  if (WEEKDAYS[lc] !== undefined) return WEEKDAYS[lc]
  // Capitalized variant (Mon, Tue)
  const cap = lc.charAt(0).toUpperCase() + lc.slice(1)
  if (WEEKDAYS[cap] !== undefined) return WEEKDAYS[cap]
  return null
}

function parseTime(s: string): { h: number; m: number } | null {
  const m = s.match(/^(\d{1,2})(?::(\d{2}))?\s*(am|pm)?$/i)
  if (!m) return null
  let h = parseInt(m[1], 10)
  const min = m[2] ? parseInt(m[2], 10) : 0
  const meridiem = m[3]?.toLowerCase()
  if (meridiem === 'pm' && h < 12) h += 12
  if (meridiem === 'am' && h === 12) h = 0
  if (h < 0 || h > 23 || min < 0 || min > 59) return null
  return { h, m: min }
}

function describe(cron: string): CronDescription {
  // Lightweight description for confirmation UI. Returns a translatable
  // descriptor (id under `schedule.*` + ICU values); the caller formats it via
  // intl.formatMessage. Note: weekday renders as an English abbreviation
  // (Sun/Mon/…) in the {day} value — localizing weekday names would need the
  // caller to map dow→name; tracked as a follow-up.
  const parts = cron.split(/\s+/)
  if (parts.length !== 5) return { id: 'schedule.cron', values: { cron } }
  const [min, hour, dom, , dow] = parts
  const time = `${hour.padStart(2, '0')}:${min.padStart(2, '0')}`
  if (hour === '*' && min.startsWith('*/')) {
    return { id: 'schedule.everyMinutes', values: { count: parseInt(min.slice(2), 10) } }
  }
  if (min === '*' && hour === '*') return { id: 'schedule.everyMinute' }
  if (hour.startsWith('*/')) {
    return { id: 'schedule.everyHours', values: { count: parseInt(hour.slice(2), 10) } }
  }
  if (hour === '*' && min === '0') return { id: 'schedule.hourly' }
  if (dow === '1-5') return { id: 'schedule.weekdaysAt', values: { time } }
  if (dow !== '*') {
    const names = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']
    const day = names[parseInt(dow, 10)] ?? dow
    return { id: 'schedule.weeklyOnAt', values: { day, time } }
  }
  if (dom !== '*') return { id: 'schedule.monthlyOnDayAt', values: { day: parseInt(dom, 10), time } }
  return { id: 'schedule.dailyAt', values: { time } }
}

export function parseNlCron(input: string): NlCronResult | null {
  const raw = input.trim().toLowerCase()
  if (!raw) return null

  // every N minutes
  let m = raw.match(/^every\s+(\d+)\s+minutes?$/)
  if (m) {
    const n = Math.max(1, parseInt(m[1], 10))
    if (n > 59) return null
    const expr = n === 1 ? '* * * * *' : `*/${n} * * * *`
    return { expression: expr, description: describe(expr) }
  }

  // every minute
  if (raw === 'every minute' || raw === 'each minute') {
    const expr = '* * * * *'
    return { expression: expr, description: describe(expr) }
  }

  // every N hours
  m = raw.match(/^every\s+(\d+)\s+hours?$/)
  if (m) {
    const n = Math.max(1, parseInt(m[1], 10))
    if (n > 23) return null
    const expr = n === 1 ? '0 * * * *' : `0 */${n} * * *`
    return { expression: expr, description: describe(expr) }
  }

  // hourly
  if (raw === 'hourly') {
    const expr = '0 * * * *'
    return { expression: expr, description: describe(expr) }
  }

  // midnight / noon shortcuts
  if (raw === 'midnight' || raw === 'daily at midnight') {
    const expr = '0 0 * * *'
    return { expression: expr, description: describe(expr) }
  }
  if (raw === 'noon' || raw === 'daily at noon') {
    const expr = '0 12 * * *'
    return { expression: expr, description: describe(expr) }
  }

  // daily at HH:MM (or HH)
  m = raw.match(/^daily\s+at\s+(\d{1,2}(?::\d{2})?\s*(?:am|pm)?)$/)
  if (m) {
    const t = parseTime(m[1].trim())
    if (!t) return null
    const expr = `${t.m} ${t.h} * * *`
    return { expression: expr, description: describe(expr) }
  }

  // every weekday (Mon-Fri) at HH:MM
  m = raw.match(/^(?:weekdays?|weekday mornings?)\s+at\s+(\d{1,2}(?::\d{2})?\s*(?:am|pm)?)$/)
  if (m) {
    const t = parseTime(m[1].trim())
    if (!t) return null
    const expr = `${t.m} ${t.h} * * 1-5`
    return { expression: expr, description: describe(expr) }
  }
  m = raw.match(/^weekday mornings?\s+at\s+(\d{1,2}(?::\d{2})?\s*(?:am|pm)?)$/)
  if (m) {
    const t = parseTime(m[1].trim())
    if (!t) return null
    const expr = `${t.m} ${t.h} * * 1-5`
    return { expression: expr, description: describe(expr) }
  }

  // weekly on DAY at HH:MM
  m = raw.match(/^weekly\s+on\s+(\w+)\s+at\s+(\d{1,2}(?::\d{2})?\s*(?:am|pm)?)$/)
  if (m) {
    const dow = weekdayTok(m[1])
    const t = parseTime(m[2].trim())
    if (dow === null || !t) return null
    const expr = `${t.m} ${t.h} * * ${dow}`
    return { expression: expr, description: describe(expr) }
  }

  // every DAY at HH:MM
  m = raw.match(/^every\s+(\w+)\s+at\s+(\d{1,2}(?::\d{2})?\s*(?:am|pm)?)$/)
  if (m) {
    const dow = weekdayTok(m[1])
    if (dow === null) return null
    const t = parseTime(m[2].trim())
    if (!t) return null
    const expr = `${t.m} ${t.h} * * ${dow}`
    return { expression: expr, description: describe(expr) }
  }

  // monthly on day N at HH:MM
  m = raw.match(/^monthly\s+on\s+(?:day\s+)?(\d{1,2})\s+at\s+(\d{1,2}(?::\d{2})?\s*(?:am|pm)?)$/)
  if (m) {
    const day = parseInt(m[1], 10)
    if (day < 1 || day > 31) return null
    const t = parseTime(m[2].trim())
    if (!t) return null
    const expr = `${t.m} ${t.h} ${day} * *`
    return { expression: expr, description: describe(expr) }
  }

  return null
}
