// nl-cron — Phase D P3.1 deliverable; GB P2-10c adds the Chinese table.
//
// Frontend-only natural language → cron expression parser. Handles the common
// patterns we expect users to type. Falls back to null when no pattern matches
// so the caller can prompt the user to type a literal cron expression.
//
// Recognized patterns (English):
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
//
// Recognized patterns (Chinese, zh-CN + zh-TW glyphs — GB P2-10c, deterministic
// table by design; the LLM-based structured parse is the reported option A):
//   每分钟 / 每 minute            → * * * * *
//   每 N 分钟 (半個小時)          → */N * * * *
//   每小时 / 每 N 小时            → 0 * * * * / 0 */N * * *
//   每天/每日 [上午9点/9点半/…]   → M H * * *   (no time → 09:00)
//   工作日/工作天 [time]          → M H * * 1-5
//   每周X/每週X/星期X/礼拜X [time] → M H * * DOW
//   每月 N 号/日 [time]           → M H N * *
// Daypart prefixes (凌晨 早上 上午 中午 下午 晚上 …) shift the hour; X点半 →
// :30, X点一刻 → :15, X点三刻 → :45, 汉字数字 (二十 point) accepted.

export interface CronDescription {
  /** react-intl message id under the `schedule.*` namespace. */
  id: string
  /** ICU placeholder values for {@link id}. */
  values?: Record<string, string | number>
  /** Cron day-of-week (0=Sun..6=Sat) for weekly schedules; the caller
   *  localizes it into the {@code day} value before formatting. */
  dayOfWeek?: number
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
  // intl.formatMessage. Weekly schedules carry `dayOfWeek` (0=Sun..6=Sat) so
  // the caller can localize the weekday into the {day} value.
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
    // Carry the cron dow (0=Sun..6=Sat) as dayOfWeek; the caller localizes
    // it into the {day} value via weekdayName() before formatting.
    return { id: 'schedule.weeklyOnAt', values: { time }, dayOfWeek: parseInt(dow, 10) % 7 }
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

  return parseZhNlCron(input)
}

// ── Chinese pattern table (GB P2-10c, 方案 B) ────────────────────────────
//
// Deterministic, zero-token patterns for zh-CN/zh-TW scheduling phrases.
// Everything reuses the shared describe() so confirmation previews localize
// through the same schedule.* message ids as the English table. Option A
// (one structured LLM parse with regex fallback) stays a report suggestion —
// it needs a prompt round-trip and a token cost this table doesn't have.

/** Full-width → ASCII digits and colon (zh-TW input often uses them). */
function normalizeZhWidth(s: string): string {
  return s
    .replace(/[０-９]/g, d => String.fromCharCode(d.charCodeAt(0) - 0xFEE0))
    .replace(/：/g, ':')
}

/** 汉字数字 → number. Supports 零〇一二两三四五六七八九 + 十 compounds up to 59. */
function zhNumeral(s: string): number | null {
  const digits: Record<string, number> = {
    零: 0, 〇: 0, 一: 1, 二: 2, 两: 2, 三: 3, 四: 4, 五: 5, 六: 6, 七: 7, 八: 8, 九: 9,
  }
  if (/^\d{1,2}$/.test(s)) return parseInt(s, 10)
  if (s === '十') return 10
  const m = s.match(/^(.)?十(.)?$/)
  if (m) {
    const tens = m[1] ? digits[m[1]] : 1
    const ones = m[2] ? digits[m[2]] : 0
    if (tens == null || ones == null) return null
    return tens * 10 + ones
  }
  if ([...s].every(ch => ch in digits)) {
    // Multi-digit run like 三五 is ambiguous — only a single digit is a number.
    return [...s].length === 1 ? digits[s] : null
  }
  return null
}

function zhNum(n: number | null): n is number {
  return n != null
}

/** A parsed wall-clock time. */
interface ZhTime { h: number; m: number }

/** Daypart prefix → hour adjustment. 凌晨/早上/上午 keep or wrap 12→0;
 *  afternoon/evening prefixes add 12 below noon. */
function applyDaypart(prefix: string | undefined, h: number): number {
  switch (prefix) {
    case '凌晨':
    case '清晨':
    case '早上':
    case '早':
    case '上午':
      return h === 12 ? 0 : h
    case '中午':
      return h
    case '午后':
    case '下午':
    case '傍晚':
    case '晚上':
    case '夜里':
    case '晚間':
    case '夜间':
      return h < 12 ? h + 12 : h
    default:
      return h
  }
}

/**
 * Parse "上午9点", "下午3点半", "晚上八点一刻", "12:30", "9点30分" ….
 * Returns null when the string isn't a recognizable time.
 */
function parseZhTime(s: string): ZhTime | null {
  const text = s.trim()
  // Plain HH:MM / HH:MM:SS forms (ASCII after width normalization).
  const colon = text.match(/^(\d{1,2}):(\d{2})(?::\d{2})?\s*(am|pm)?$/)
  if (colon) {
    let h = parseInt(colon[1], 10)
    const min = parseInt(colon[2], 10)
    if (colon[3] === 'pm' && h < 12) h += 12
    if (colon[3] === 'am' && h === 12) h = 0
    if (h > 23 || min > 59) return null
    return { h, m: min }
  }
  const m = text.match(
    /^(凌晨|清晨|早上|早|上午|中午|午后|下午|傍晚|晚上|夜里|晚間|夜间)?\s*(\d{1,2}|[零〇一二两三四五六七八九十]+)\s*[点點時时]\s*(半|一刻|三刻|(\d{1,2}|[零〇一二两三四五六七八九十]+)\s*分(钟)?)?\s*(钟|整)?$/,
  )
  if (!m) return null
  const hour = zhNumeral(m[2])
  if (!zhNum(hour) || hour > 23) return null
  let min = 0
  if (m[3] === '半') min = 30
  else if (m[3] === '一刻') min = 15
  else if (m[3] === '三刻') min = 45
  else if (m[4] != null) {
    const parsed = zhNumeral(m[4])
    if (!zhNum(parsed) || parsed > 59) return null
    min = parsed
  }
  return { h: applyDaypart(m[1], hour), m: min }
}

/** The schedule's optional time clause, or the 09:00 default when absent. */
function zhTimeOr(input: string): ZhTime | null {
  if (!input) return { h: 9, m: 0 }
  return parseZhTime(input)
}

/** Map a weekday token (日/天/一…六/0-6/汉字) to cron DOW, or null. */
function zhWeekday(token: string): number | null {
  switch (token) {
    case '日': case '天': case '0': case '零': return 0
    case '一': case '1': return 1
    case '二': case '2': return 2
    case '三': case '3': return 3
    case '四': case '4': return 4
    case '五': case '5': return 5
    case '六': case '6': return 6
    default: return null
  }
}

function zhResult(expr: string): NlCronResult {
  return { expression: expr, description: describe(expr) }
}

/** Chinese (zh-CN/zh-TW) natural-language cron parser. */
export function parseZhNlCron(input: string): NlCronResult | null {
  const raw = normalizeZhWidth(input.trim())
  if (!raw) return null

  // 每分钟 / 每一分鐘
  if (/^每[一]?分[钟鐘]$/.test(raw)) return zhResult('* * * * *')

  // 每 N 分钟 / 每半小时
  let m = raw.match(/^每\s*(半|(\d{1,2}|[零〇一二两三四五六七八九十]+))\s*分[钟鐘]$/)
  if (m) {
    const n = m[1] === '半' ? 30 : zhNumeral(m[2])
    if (!zhNum(n) || n < 1 || n > 59) return null
    return zhResult(n === 1 || n === 0 ? '* * * * *' : `*/${n} * * * *`)
  }

  // 每小时 / 每 N 小时 / 每半个小时→30分
  m = raw.match(/^每\s*(半)?\s*(\d{1,2}|[零〇一二两三四五六七八九十]+)?\s*(个|個)?\s*(小时|小時)$/)
  if (m) {
    if (m[1]) return zhResult('*/30 * * * *')
    const n = m[2] ? zhNumeral(m[2]) : 1
    if (!zhNum(n) || n < 1 || n > 23) return null
    return zhResult(n === 1 ? '0 * * * *' : `0 */${n} * * *`)
  }

  // 每天/每日/天天 [的] [time] — no time clause defaults to 09:00 (the
  // preview step lets the user adjust before saving).
  m = raw.match(/^(每天|每日|天天)(的)?(.*)$/)
  if (m) {
    const t = zhTimeOr(m[3])
    if (!t) return null
    return zhResult(`${t.m} ${t.h} * * *`)
  }

  // 工作日/工作天/工作日们 [的] [time]
  m = raw.match(/^(每个|每個)?工作(日|天)(的)?(.*)$/)
  if (m) {
    const t = zhTimeOr(m[4])
    if (!t) return null
    return zhResult(`${t.m} ${t.h} * * 1-5`)
  }

  // 每周X / 每週X / 每星期X / 每礼拜X [的] [time]
  m = raw.match(/^每\s*(个|個)?\s*(周|週|星期|禮拜|礼拜)\s*(的)?\s*([日天一二三四五六0-9]+)\s*(的)?(.*)$/)
  if (m) {
    const dow = zhWeekday(m[4])
    if (dow == null) return null
    const t = zhTimeOr(m[6])
    if (!t) return null
    return zhResult(`${t.m} ${t.h} * * ${dow}`)
  }

  // 每月 N 号/日/號 [的] [time]
  m = raw.match(/^每(个|個)?月\s*(的)?\s*(\d{1,2}|[零〇一二两三四五六七八九十]{1,3})\s*(号|號|日)\s*(的)?(.*)$/)
  if (m) {
    const day = zhNumeral(m[3])
    if (!zhNum(day) || day < 1 || day > 31) return null
    const t = zhTimeOr(m[6])
    if (!t) return null
    return zhResult(`${t.m} ${t.h} ${day} * *`)
  }

  return null
}
