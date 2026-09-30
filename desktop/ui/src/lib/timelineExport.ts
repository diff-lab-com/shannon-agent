// Office Wave 3 C6 — self-contained HTML export for a turn timeline.
// The exported document must render with zero external dependencies, so the
// palette here is literal hex by contract — parent CSS variables cannot
// cross into a standalone file (same rationale as MermaidRenderer's
// documented design-token exception). Escape all dynamic text.

import type { TurnTimeline } from '@/types'

type ReasonTone = 'success' | 'neutral' | 'error'

const REASON_TONES: Record<string, ReasonTone> = {
  'completed': 'success',
  'failed': 'error',
  'timeout': 'error',
  'budget-exceeded': 'error',
  'interrupted': 'neutral',
  'max-turns': 'neutral',
  'unknown': 'neutral',
}

export function reasonTone(reason: string): ReasonTone {
  return REASON_TONES[reason] ?? 'neutral'
}

function formatDuration(ms: number | null | undefined): string {
  if (!ms || ms <= 0) return '—'
  if (ms < 1000) return `${ms}ms`
  const s = ms / 1000
  return `${s.toFixed(s < 10 ? 1 : 0)}s`
}

// Exported documents are language-neutral artifacts: they format in 'en'
// regardless of the app locale (the page's own clock uses the app locale).
function formatTime(tsNs: number, locale: string): string {
  return new Date(tsNs / 1e6).toLocaleTimeString(locale, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

const COST_FORMAT = new Intl.NumberFormat(undefined, {
  style: 'currency',
  currency: 'USD',
})

const nf = new Intl.NumberFormat()

export function timelineToHtml(timeline: TurnTimeline): string {
  const time = (ns: number) => formatTime(ns, 'en')
  const dur = (ms: number | null | undefined) => (ms && ms > 0 ? formatDuration(ms) : '—')
  const toolRows = timeline.turns
    .flatMap(tu => tu.tools.map(tool => ({
      turn: tu.turn,
      name: tool.tool_name,
      at: time(tool.start_ts_ns),
      duration: dur(tool.duration_ms),
      status: tool.is_error ? 'error' : 'ok',
    })))
  const totalTools = toolRows.length
  const totalOutput = timeline.turns.reduce((a, tu) => a + tu.output_tokens, 0)

  const turnSections = timeline.turns
    .map(tu => {
      const tone = tu.reason ? reasonTone(tu.reason) : 'neutral'
      const badgeColor =
        tone === 'success' ? '#1b7f4d; background:#e2f5ea' : tone === 'error' ? '#b3261e; background:#fdeceb' : '#5f6368; background:#f1f3f4'
      const tools = tu.tools.length
        ? tu.tools
            .map(
              tool =>
                `        <li class="tool${tool.is_error ? ' tool-error' : ''}"><span class="tool-name">${escapeHtml(tool.tool_name)}</span> <span class="tool-meta">${time(tool.start_ts_ns)} · ${dur(tool.duration_ms)}${tool.is_error ? ' · error' : ''}</span></li>`,
            )
            .join('\n')
        : '        <li class="tool tool-empty">no tool calls</li>'
      const cost = tu.cost_usd != null ? ` · ${COST_FORMAT.format(tu.cost_usd)}` : ''
      return [
        '    <section class="turn">',
        `      <h2>Turn ${tu.turn} <span class="reason" style="color:${badgeColor}">${escapeHtml(tu.reason ?? '')}</span></h2>`,
        `      <p class="meta">${time(tu.start_ts_ns)} → ${time(tu.end_ts_ns)} · ↓ ${nf.format(tu.input_tokens)} ↑ ${nf.format(tu.output_tokens)}${cost}</p>`,
        '      <ul class="tools">',
        tools,
        '      </ul>',
        '    </section>',
      ].join('\n')
    })
    .join('\n')

  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Timeline ${escapeHtml(timeline.session_id)}</title>
<style>
  body { font-family: system-ui, -apple-system, "Segoe UI", sans-serif; color: #202124; margin: 2rem auto; max-width: 760px; padding: 0 1rem; }
  h1 { font-size: 1.25rem; } h2 { font-size: 1rem; margin-bottom: 0.25rem; }
  .meta, .tool-meta { color: #5f6368; font-size: 0.8rem; }
  .summary { color: #5f6368; font-size: 0.85rem; }
  .reason { font-size: 0.75rem; font-weight: 600; border-radius: 999px; padding: 1px 8px; margin-left: 6px; }
  .turn { border: 1px solid #e0e0e0; border-radius: 8px; padding: 0.75rem 1rem; margin: 0.75rem 0; }
  .tools { list-style: none; padding-left: 0; margin: 0.5rem 0 0; }
  .tool { padding: 2px 0; font-size: 0.85rem; border-bottom: 1px solid #f1f3f4; }
  .tool-empty { color: #9aa0a6; font-style: italic; }
  .tool-error .tool-name { color: #b3261e; font-weight: 600; }
  .footer { margin-top: 1.5rem; color: #9aa0a6; font-size: 0.75rem; }
</style>
</head>
<body>
  <h1>Turn timeline</h1>
  <p class="summary">session <code>${escapeHtml(timeline.session_id)}</code>${timeline.model ? ` · model ${escapeHtml(timeline.model)}` : ''} · ${timeline.turns.length} turns · ${totalTools} tool calls · ↑ ${nf.format(totalOutput)} output tokens</p>
${turnSections}
  <p class="footer">Exported from Shannon on ${new Date().toISOString()}</p>
</body>
</html>
`
}
