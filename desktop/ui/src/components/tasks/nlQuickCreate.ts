// nlQuickCreate — pure helpers behind the NlRoutineQuickCreate card
// (design: docs/design/ui-redesign-2026-10/pages/04-tasks.html, 自然语言创建 ·
// 结构化预览: one sentence → trigger/action/notify chips → 调整 / 激活).
//
// parseNlCron (src/lib/nl-cron.ts) is a WHOLE-STRING matcher: the schedule
// clause has to be the entire input. A design-example sentence like
// 「工作日早上 9 点汇总昨日 PR,发到 Discord」 therefore never matches — the
// action tail ("汇总昨日 PR…") rides along and kills every pattern.
//
// splitNlSchedule bridges that honestly and deterministically (zero tokens):
// it only ever CUTS the input at a candidate schedule-clause end, and only
// returns a split when parseNlCron CONFIRMS the candidate. Whatever it cannot
// confidently read as a schedule stays in the action/prompt half — the card
// never pretends to have understood a delivery target ("发到 Discord") that
// the parser did not actually parse.

import { parseNlCron, type NlCronResult } from '@/lib/nl-cron'

export interface NlSplitResult {
  /** The leading clause the parser confirmed as a schedule. */
  scheduleText: string
  /** Everything after it (may be empty). Becomes the routine prompt. */
  restText: string
  /** The parse result for {@link scheduleText}. */
  parsed: NlCronResult
}

/** Strategy B candidate boundary: comma/semicolon segment prefixes. */
const SEGMENT_SPLIT = /[,，;；]/

/** Leading punctuation stripped off the action half before use. */
const LEADING_NOISE = /^[\s,，、;；。.]+/

// Strategy C probes: where a time clause can END inside a run-on sentence.
// Each mirrors a shape parseZhTime / parseTime accepts (kept in sync by the
// component tests); a probe only PROPOSES a cut — the parser still has to
// confirm the candidate, so a stale probe degrades to "no split", never to a
// wrong schedule.
const TIME_CLAUSE_PROBES: RegExp[] = [
  // zh daypart + numeral + 点/點/時/时 + optional 半/一刻/三刻/N分/钟/整
  /(凌晨|清晨|早上|早|上午|中午|午后|下午|傍晚|晚上|夜里|晚間|夜间)?\s*(?:\d{1,2}|[零〇一二两三四五六七八九十]+)\s*[点點時时]\s*(?:半|一刻|三刻|(?:\d{1,2}|[零〇一二两三四五六七八九十]+)\s*分(?:钟)?)?\s*(?:钟|整)?/g,
  // zh monthly day: 每月 N 号/號/日 (the 号 ending is not covered by the time
  // probe, but 「每月 15 号发周报」 is a leading-schedule sentence too)
  /每月\s*(?:的)?\s*(?:\d{1,2}|[零〇一二两三四五六七八九十]{1,3})\s*(?:号|號|日)/g,
  // ASCII times: 09:30 / 9:30 / 9am / 9:30 pm (the bare-digit tail also
  // covers English "daily at 9 <do things>")
  /\d{1,2}(?::\d{2})?(?:\s*(?:am|pm))?/gi,
  // English time-word shortcuts
  /\b(?:midnight|noon)\b/gi,
]

/**
 * Split a one-line input into (schedule clause, action remainder).
 *
 * Order of attempts, cheapest-first:
 *   1. the whole input parses as a schedule → no action half;
 *   2. segment prefixes: the first k comma-separated segments confirm as a
 *      schedule → the rest is the action;
 *   3. time-clause probing: each TIME_CLAUSE_PROBES match proposes a cut
 *      after the time clause → the parser confirms the prefix.
 * Returns null when nothing confirms — the caller must surface its
 * parse-failure guidance instead of guessing.
 */
export function splitNlSchedule(input: string): NlSplitResult | null {
  const text = input.trim()
  if (!text) return null

  const confirmed = (scheduleText: string, restText: string): NlSplitResult | null => {
    const parsed = parseNlCron(scheduleText.trim())
    return parsed ? { scheduleText: scheduleText.trim(), restText: restText.trim(), parsed } : null
  }

  // 1. Whole input is a schedule ("every 15 minutes", 「每天九点」).
  const whole = confirmed(text, '')
  if (whole) return whole

  // 2. Comma/semicolon segment prefixes — the canonical design shape
  //    「工作日早上 9 点,汇总昨日 PR,发到 Discord」.
  const segments = text.split(SEGMENT_SPLIT)
  if (segments.length > 1) {
    for (let k = 1; k < segments.length; k++) {
      const hit = confirmed(segments.slice(0, k).join(', '), segments.slice(k).join(', '))
      if (hit) return hit
    }
  }

  // 3. Time-clause probing for run-on sentences without separators
  //    (「工作日早上 9 点汇总昨日 PR」). All matches of a probe are tried
  //    before the next probe; the earliest confirmed end wins, so the
  //    schedule clause stays as short as the parser can vouch for.
  for (const probe of TIME_CLAUSE_PROBES) {
    probe.lastIndex = 0
    let m: RegExpExecArray | null
    while ((m = probe.exec(text)) !== null) {
      const end = m.index + m[0].length
      const hit = confirmed(text.slice(0, end), text.slice(end).replace(LEADING_NOISE, ''))
      if (hit) return hit
      if (m[0].length === 0) probe.lastIndex += 1 // zero-width guard
    }
  }

  return null
}

/** Maximum characters kept in the auto-derived routine name. */
const NAME_MAX = 32

/**
 * Derive a short routine name from the split: prefer the action half (the
 * schedule reads "when", the action reads "what"). Deterministic truncation
 * with an ellipsis; trailing punctuation is stripped first.
 */
export function deriveRoutineName(scheduleText: string, restText: string): string {
  const source = (restText.trim() || scheduleText.trim()).replace(/[。.!！?？:：,，;；]+$/, '').trim()
  if (!source) return ''
  return source.length > NAME_MAX ? `${source.slice(0, NAME_MAX)}…` : source
}

/** Maximum characters shown in the preview's action chip (title keeps the full text). */
export const ACTION_SNIPPET_MAX = 48

/** Truncated one-line summary for the action chip (full text stays on `title`). */
export function actionSnippet(text: string): string {
  const oneLine = text.replace(/\s+/g, ' ').trim()
  if (oneLine.length <= ACTION_SNIPPET_MAX) return oneLine
  return `${oneLine.slice(0, ACTION_SNIPPET_MAX)}…`
}
