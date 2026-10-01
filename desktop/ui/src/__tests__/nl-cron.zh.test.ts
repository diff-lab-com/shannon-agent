// GB P2-10c — Chinese natural-language cron patterns (方案 B: deterministic
// table, no LLM round-trip). zh-CN and zh-TW glyphs both covered; unchanged
// English behavior is pinned by nl-cron.test.ts.

import { describe, it, expect } from 'vitest'
import { parseNlCron, parseZhNlCron } from '@/lib/nl-cron'

describe('parseNlCron — 中文 每 N 分钟 / 小时', () => {
  it.each([
    ['每分钟', '* * * * *'],
    ['每一分鐘', '* * * * *'],
    ['每5分钟', '*/5 * * * *'],
    ['每15分鐘', '*/15 * * * *'],
    ['每十五分钟', '*/15 * * * *'],
    ['每半小时', '*/30 * * * *'],
    ['每半小時', '*/30 * * * *'],
  ])('%s → %s', (input, expr) => {
    expect(parseNlCron(input)?.expression).toBe(expr)
  })

  it.each([
    ['每小时', '0 * * * *'],
    ['每小時', '0 * * * *'],
    ['每6小时', '0 */6 * * *'],
    ['每六個小时', '0 */6 * * *'],
    ['每12小時', '0 */12 * * *'],
  ])('%s → %s', (input, expr) => {
    expect(parseNlCron(input)?.expression).toBe(expr)
  })

  it('rejects out-of-range intervals', () => {
    expect(parseNlCron('每90分钟')).toBeNull()
    expect(parseNlCron('每25小时')).toBeNull()
  })
})

describe('parseNlCron — 中文 每天 + 时间', () => {
  it.each([
    ['每天上午9点', '0 9 * * *'],
    ['每天上午9点半', '30 9 * * *'],
    ['每天下午3点', '0 15 * * *'],
    ['每天下午3点半', '30 15 * * *'],
    ['每天晚上8点30分', '30 20 * * *'],
    ['每天早上七点一刻', '15 7 * * *'],
    ['每天晚上11点三刻', '45 23 * * *'],
    ['每天9:30', '30 9 * * *'],
    ['每天中午12点', '0 12 * * *'],
    ['每天中午12点半', '30 12 * * *'],
    ['每天凌晨2点', '0 2 * * *'],
    ['每天', '0 9 * * *'], // no time clause → the 09:00 default (preview adjusts)
    ['每日的上午八点', '0 8 * * *'],
    ['每天晚上十一點', '0 23 * * *'], // zh-TW glyphs + 汉字 hour
  ])('%s → %s', (input, expr) => {
    expect(parseNlCron(input)?.expression).toBe(expr)
  })

  it('surfaces a translatable daily description for the confirm preview', () => {
    expect(parseNlCron('每天上午9点')?.description).toEqual({
      id: 'schedule.dailyAt',
      values: { time: '09:00' },
    })
  })
})

describe('parseNlCron — 中文 工作日', () => {
  it.each([
    ['工作日 上午9点', '0 9 * * 1-5'],
    ['每个工作日9点', '0 9 * * 1-5'],
    ['工作日上午9点半', '30 9 * * 1-5'],
    ['工作天', '0 9 * * 1-5'],
  ])('%s → %s', (input, expr) => {
    expect(parseNlCron(input)?.expression).toBe(expr)
  })

  it('describes weekdays through the shared weekdaysAt id', () => {
    expect(parseNlCron('工作日 上午9点')?.description).toEqual({
      id: 'schedule.weekdaysAt',
      values: { time: '09:00' },
    })
  })
})

describe('parseNlCron — 中文 每周X', () => {
  it.each([
    ['每周一 上午9点', '0 9 * * 1'],
    ['每周一下午2点半', '30 14 * * 1'],
    ['每週五 下午6点', '0 18 * * 5'],
    ['每星期日 上午10点', '0 10 * * 0'],
    ['每周天早上8点', '0 8 * * 0'],
    ['每礼拜六 晚上八点', '0 20 * * 6'],
    ['每周3 晚上7点', '0 19 * * 3'],
    ['每周三', '0 9 * * 3'], // no time → 09:00 default
  ])('%s → %s', (input, expr) => {
    expect(parseNlCron(input)?.expression).toBe(expr)
  })

  it('carries dayOfWeek so the caller can localize the name', () => {
    const parsed = parseNlCron('每周一 上午9点')
    expect(parsed?.description).toMatchObject({ id: 'schedule.weeklyOnAt', dayOfWeek: 1 })
  })

  it('rejects non-weekday tokens', () => {
    expect(parseNlCron('每月七')).toBeNull()
    expect(parseZhNlCron('每周八点')).toBeNull() // 八点 is a time, not a weekday
  })
})

describe('parseNlCron — 中文 每月N日', () => {
  it.each([
    ['每月1号 上午9点', '0 9 1 * *'],
    ['每月15日 上午10点半', '30 10 15 * *'],
    ['每月1號', '0 9 1 * *'],
    ['每月二十一号 下午1点', '0 13 21 * *'],
  ])('%s → %s', (input, expr) => {
    expect(parseNlCron(input)?.expression).toBe(expr)
  })

  it('rejects impossible days', () => {
    expect(parseNlCron('每月32号 上午9点')).toBeNull()
  })
})

describe('parseNlCron — 中文 边界', () => {
  it('unrecognized phrases return null (caller prompts for literal cron)', () => {
    expect(parseNlCron('记得提醒我喝水')).toBeNull()
    expect(parseNlCron('每天有时候')).toBeNull()
    expect(parseNlCron('')).toBeNull()
  })

  it('tolerates full-width digits and colon (zh-TW input)', () => {
    expect(parseNlCron('每天上午９点')?.expression).toBe('0 9 * * *')
    expect(parseNlCron('每天9：30')?.expression).toBe('30 9 * * *')
  })

  it('english patterns still parse after the table addition', () => {
    expect(parseNlCron('every 15 minutes')?.expression).toBe('*/15 * * * *')
    expect(parseNlCron('weekly on monday at 09:00')?.expression).toBe('0 9 * * 1')
  })
})
