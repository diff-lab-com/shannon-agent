// formatInterval — humanize a routine interval in seconds for display
// (W2-7: the template browser used to render bare "3600s").
//
// Pure and locale-agnostic: callers pass a translate function so react-intl
// owns the wording (schedule.everySeconds / everyMinute / everyMinutes /
// everyHours / everyDays). Non-positive or non-finite input falls back to
// "hourly" wording via schedule.hourly rather than showing "0s".

type Translate = (id: string, values?: Record<string, number>) => string

export function formatInterval(secs: number, t: Translate): string {
  if (!Number.isFinite(secs) || secs <= 0) return t('schedule.hourly')
  if (secs < 60) return t('schedule.everySeconds', { count: secs })
  const mins = Math.round(secs / 60)
  if (mins < 60) return mins === 1 ? t('schedule.everyMinute') : t('schedule.everyMinutes', { count: mins })
  const hours = secs / 3600
  if (hours < 24) {
    // 90 min -> "Every 2 hours" reads better than 1.5 hours.
    return t('schedule.everyHours', { count: Math.max(1, Math.round(hours)) })
  }
  return t('schedule.everyDays', { count: Math.max(1, Math.round(secs / 86400)) })
}
