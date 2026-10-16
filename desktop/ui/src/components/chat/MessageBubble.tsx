import { useState, memo, useEffect, useMemo, useSyncExternalStore } from 'react'
import { useIntl } from 'react-intl'
import { useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import { messageFeedbackKey } from '@/lib/feedbackKey'
import type { FeedbackRating } from '@/lib/tauri-api'
import { convertFileSrc } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'
import { Modal } from '@/components/ui/modal'
import { useChat } from '@/context/ChatContext'
import { useSessions } from '@/context/SessionContext'
import { useCatalog } from '@/context/CatalogContext'
import * as api from '@/lib/tauri-api'
import { Markdown } from '@/components/chat/Markdown'
import { summarizeDiffLineStats, type DiffLineStats } from '@/components/chat/diffStats'
import { InlineDiffCard } from '@/components/chat/InlineDiffCard'
import {
  Message,
  MessageAvatar,
  MessageContent,
  ResponseStream,
  Reasoning,
  ActionToolbar,
  Tool,
  ToolHeader,
  ToolContent,
} from '@/components/ai-elements'
import { readShowThinkingPref, shouldShowThinking } from '@/lib/thinkingPref'
import {
  ensureToolReadOnlyMap,
  getToolReadOnlyMapVersion,
  groupToolSegments,
  readGroupingPrefs,
  subscribeToolReadOnlyMap,
  type ToolGroupUnit,
} from '@/lib/toolGrouping'
import { ToolGroupCard } from '@/components/chat/ToolGroupCard'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { ResearchReportModal } from '@/components/chat/ResearchReportModal'
import { ArtifactChipList } from '@/components/artifact/ArtifactChip'
import { detectArtifacts } from '@/components/artifact/detectArtifact'
import { FileRefChip } from '@/components/shared/FileRefChip'
import { basenameOf, extractToolInputPath, FILE_MUTATING_TOOLS } from '@/lib/fileRefs'
import { FileCard } from '@/components/chat/FileCard'
import type { ChatMessage, ToolCall, FileAttachment, TimelineTurn } from '@/types'
import type { InjectedMemory } from '@/lib/tauri-api'
import { useTurnUsage } from '@/hooks/useTurnUsage'
import { cn } from '@/lib/utils'

/** B1 §4-7: payload for a TRUE regenerate — rewind to the checkpoint before
 *  the preceding user turn, then re-send that turn's text (plus its
 *  attachment paths; the rewind cannot restore files by itself, so they are
 *  explicitly re-attached on the resend). */
export interface RegeneratePayload {
  turnIndex: number
  content: string
  attachmentPaths: string[]
}

interface MessageBubbleProps {
  message: ChatMessage
  messageIndex: number
  isBranch?: boolean
  onViewDiff: (path: string) => void
  onViewDiffMulti?: (paths: string[]) => void
  /** Conversation turn this message owns, when it is rewindable (/rewind). */
  rewindTurnIndex?: number | null
  onRewind?: (turnIndex: number) => Promise<void>
  /** P1-⑤ telemetry: tool_use_id → duration (ms) from the session's L0
   *  trace timeline — the authoritative durations for historical messages
   *  (live tool calls carry their own client-measured duration_ms). */
  durationLookup?: Map<string, number>
  /** R3 §4 遗留项: per-turn cost windows from the SAME trace-timeline read
   *  as durationLookup (MessageArea derives both from one IPC). Absent in
   *  isolated renders — the cost segment then simply never renders. */
  costWindows?: TurnCostWindow[]
  /** T1 (R3 §4.1 Reasoning 时长): per-turn reasoning spans from the same
   *  trace-timeline read as costWindows. Absent in isolated renders — the
   *  committed thinking header then keeps the duration-less 「已思考」. */
  reasoningWindows?: TurnReasoningWindow[]
  /** B1 §4-7: present only on the LAST assistant message of the session —
   *  its presence is the regenerate button's visibility condition. */
  regenerate?: RegeneratePayload | null
  /** B1 §4-8: open the composer-based edit flow for this user message. */
  onEditMessage?: (index: number) => void
  /** B1 §4-12: transient highlight ring while a search jump lands here. */
  searchFlash?: boolean
  /** Settings R3 T9: computed by the list parent (firstAssistantOfTurnFlags
   *  over the whole message array) — true on the first assistant message
   *  after the most recent user message. Gates the collapsed thinking block
   *  under the 'first' display tier ('all' ignores it, 'none' hides both). */
  isFirstAssistantOfTurn?: boolean
}

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg'])

function isImagePath(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase()
  return ext != null && IMAGE_EXTENSIONS.has(ext)
}

/* ─────── Header — avatar + role + timestamp + turn meta ─────── */

/**
 * 补1 (R2 审查 §7.1 · 02-chat msg-meta): per-turn aggregate segments shown
 * after the header timestamp on assistant bubbles — the design's
 * 「… · 12 个工具调用 · 2.4 分钟」tail. Every field is DERIVED from data the
 * bubble already holds; the renderer drops any field that could not be
 * derived (honesty contract: never a 0, never a placeholder). Cost rides in
 * via the R3 §4 遗留项 cost-window lookup — see TurnCostWindow.
 */
export interface TurnMeta {
  /** Tool calls across the WHOLE turn (summed over the turn's messages). */
  toolCount: number
  /** lastTs − firstTs across the turn's messages (ms); null when the
   *  timestamps don't yield a positive, finite span. */
  durationMs: number | null
  /** R3 §4 遗留项: the turn's TimelineTurn.cost_usd, matched by timestamp
   *  window against the session's trace timeline (MessageArea's lookup).
   *  null when no window overlaps this turn — the segment then does not
   *  render (absent/0 costs never produce a window upstream). */
  costUsd: number | null
  /** T1 (R3 §4.1 Reasoning 时长): the turn's TimelineTurn
   *  .reasoning_duration_ms, matched by the same timestamp-window overlap as
   *  costUsd. Feeds the committed thinking block's 「已思考 N 秒」 header
   *  (NOT this header's meta tail); null when no reasoning window overlaps —
   *  the chip then keeps the duration-less 「已思考」 (honesty contract). */
  reasoningMs: number | null
  /** G4b (2026-10-15 parity review §5-G4b): the turn's token usage projected
   *  from the trace timeline (matchTurnUsage over useTurnUsage's rows) —
   *  feeds the hover detail strip, NOT the header meta tail. null when no
   *  timeline turn overlaps this message's window (reloaded history's
   *  load-time stamps match nothing — the strip then never renders, the same
   *  honest degradation the cost segment shows). */
  usage: TurnUsageStats | null
}

/**
 * R3 §4 遗留项: one turn's spendable window from the trace timeline —
 * TimelineTurn.start/end_ts_ns (epoch NS) narrowed to epoch MS so it can be
 * matched against ChatMessage timestamps directly. Built ONLY for turns whose
 * cost_usd is present and > 0: a missing or zero cost produces NO window, so
 * the bubble can never render a fabricated "$0.000" segment.
 */
export interface TurnCostWindow {
  startMs: number
  endMs: number
  costUsd: number
}

/**
 * Match a turn's message window [firstTs, lastTs] (ms) against the timeline's
 * cost windows. OVERLAP — not containment — is the test on purpose: the
 * optimistic user message and the backend's turn/start event are written a
 * few ms apart, so a strict containment would silently drop real matches.
 * Why windows instead of turn NUMBERS: the engine's envelope turn counter
 * (session_log/writer.rs) is monotonic across /rewind — after truncating the
 * L0 log the next send gets a number ABOVE the pre-rewind count while the
 * conversation only holds fewer user turns, so "conversation index + 1" would
 * mis-attribute another turn's spend. Timestamps cannot lie across a rewind.
 * First overlap wins (timeline turns are sequential, non-overlapping).
 */
export function matchTurnCost(windows: TurnCostWindow[] | undefined | null, firstTs: number, lastTs: number): number | null {
  if (!Array.isArray(windows)) return null
  for (const w of windows) {
    if (w.startMs <= lastTs && w.endMs >= firstTs) return w.costUsd
  }
  return null
}

/**
 * T1 (R3 §4.1 Reasoning 时长): one turn's projected reasoning span from the
 * SAME trace-timeline read as TurnCostWindow — TimelineTurn
 * .reasoning_duration_ms (first→last thinking-flagged `assistant/chunk` ts,
 * backend-folded). Built ONLY for turns where the backend projected a value:
 * a turn with no thinking events carries NO window, so the bubble's
 * committed 「已思考」 header can never invent a duration (honesty contract).
 */
export interface TurnReasoningWindow {
  startMs: number
  endMs: number
  reasoningMs: number
}

/**
 * T1: the same overlap (not containment) match matchTurnCost uses — first
 * window wins (timeline turns are sequential, non-overlapping).
 */
export function matchTurnReasoning(windows: TurnReasoningWindow[] | undefined | null, firstTs: number, lastTs: number): number | null {
  if (!Array.isArray(windows)) return null
  for (const w of windows) {
    if (w.startMs <= lastTs && w.endMs >= firstTs) return w.reasoningMs
  }
  return null
}

/* ─────── G4b (2026-10-15 parity review §5-G4b) — hover usage details ─────── */

/**
 * G4b: one turn's usage projection — the numbers the hover detail strip
 * shows under an assistant bubble. GRANULARITY RULING (2026-10-15 recon):
 * message-level usage does not exist on any wire the UI reads — the session
 * log folds every LLM request frame of a user-visible turn into ONE
 * TimelineTurn, and the live `query:usage` payload drops the absolute cache
 * counts (it carries a hit-rate only). The strip therefore always renders
 * TURN-granularity figures and titles itself 「本轮」 (chat.message.usageTurn
 * .title) — the honesty contract applied at the wording level.
 */
export interface TurnUsageStats {
  inputTokens: number
  outputTokens: number
  cacheReadTokens: number
  cacheCreationTokens: number
  /** Turn wall-clock span (TimelineTurn end − start, ms). Includes the
   *  turn's tool executions. null on a zero-width window — the duration and
   *  tok/s segments then stay hidden (never a 0, never a fabricated span). */
  durationMs: number | null
  /** output_tokens ÷ (durationMs ÷ 1000) — turn-AVERAGE throughput. The
   *  denominator is the wall-clock span, so tool-heavy turns read low; it is
   *  presented under the same turn-level title, not as a generation-speed
   *  measurement. null unless output > 0 and the span reaches 1s (below the
   *  displayed duration granularity the figure is noise). */
  tokensPerSecond: number | null
}

/**
 * First timeline turn whose window overlaps [firstTs, lastTs] (epoch ms) —
 * the SAME overlap (not containment) rule as matchTurnCost (optimistic user
 * message vs. turn/start drift); timeline turns are sequential and
 * non-overlapping, so first hit wins.
 */
export function matchTurnUsage(turns: TimelineTurn[] | undefined | null, firstTs: number, lastTs: number): TimelineTurn | null {
  if (!Array.isArray(turns)) return null
  for (const turn of turns) {
    const startMs = turn.start_ts_ns / 1e6
    const endMs = turn.end_ts_ns / 1e6
    if (startMs <= lastTs && endMs >= firstTs) return turn
  }
  return null
}

/**
 * tok/s derivation with the honesty gates inline: no output, no span, a
 * sub-second span or a non-finite result all yield null — the segment is
 * OMITTED, never shown as 0 or ∞.
 */
export function turnTokensPerSecond(outputTokens: number, durationMs: number | null): number | null {
  if (outputTokens <= 0 || durationMs == null || !Number.isFinite(durationMs) || durationMs < 1000) return null
  const tps = outputTokens / (durationMs / 1000)
  if (!Number.isFinite(tps) || tps <= 0) return null
  return tps >= 100 ? Math.round(tps) : Math.round(tps * 10) / 10
}

/** TimelineTurn → the strip's stats (span math + tok/s derivation). */
export function turnUsageStats(turn: TimelineTurn): TurnUsageStats {
  const durationMs = Number.isFinite(turn.start_ts_ns) && Number.isFinite(turn.end_ts_ns) && turn.end_ts_ns > turn.start_ts_ns
    ? Math.round((turn.end_ts_ns - turn.start_ts_ns) / 1e6)
    : null
  return {
    inputTokens: turn.input_tokens,
    outputTokens: turn.output_tokens,
    cacheReadTokens: turn.cache_read_tokens,
    cacheCreationTokens: turn.cache_creation_tokens,
    durationMs,
    tokensPerSecond: turnTokensPerSecond(turn.output_tokens, durationMs),
  }
}

/** Turn cost → the design's 「$0.087」 shape. Sub-cent and non-finite inputs
 *  yield null — OMITTED, never shown as $0.000 (honesty contract). Under a
 *  dollar the design's three decimals are kept (cents carry the signal at
 *  turn granularity); at or above, the usual currency rendering. The locale
 *  is pinned to en-US on purpose: the design figure is a literal "$0.087",
 *  and a locale-default formatter renders "US$…"/"0,08 …" elsewhere — the
 *  segment is a design constant, not a localized sentence. */
const COST_SUB_DOLLAR_FORMAT = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 3,
  maximumFractionDigits: 3,
})
const COST_FORMAT = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' })

export function turnCostValue(usd: number): string | null {
  if (!Number.isFinite(usd) || usd <= 0) return null
  return (usd < 1 ? COST_SUB_DOLLAR_FORMAT : COST_FORMAT).format(usd)
}

/**
 * R3 §4 遗留项: reasoning-duration derivation for a COMPLETED thinking block
 * — 「已思考 N 秒」 from the reasoning block's own start/end timestamps.
 * DATA-SOURCE STATUS (2026-10-15): the L0 log DOES timestamp every thinking
 * delta (assistant/chunk events carry ts_ns + a thinking flag,
 * shannon-types/session_event.rs), but no per-block wire field reaches this
 * bubble — ChatMessage.thinking is a bare string (types/index.ts:160) and
 * the live QUERY_THINKING payload carries query_id + content only
 * (types/index.ts:41). With no inputs the helper returns null and the header
 * falls back to the duration-less 「已思考」 (chat.streaming.thought) — never
 * an invented 0.
 *
 * T1 (same day): the TURN-LEVEL projection DID land —
 * TimelineTurn.reasoning_duration_ms (the backend fold of the turn's
 * thinking-chunk ts span) rides the trace-timeline IPC as TurnReasoningWindows.
 * reasoningDurationFromMs consumes it below; this helper stays for a future
 * per-block field (the seconds math is shared).
 */
export function reasoningDurationValue(startMs?: number | null, endMs?: number | null): { seconds: number } | null {
  if (startMs == null || endMs == null) return null
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs) || endMs < startMs) return null
  const sec = Math.round((endMs - startMs) / 1000)
  if (sec < 1) return null
  return { seconds: sec }
}

/**
 * T1 (R3 §4.1): the landed projection field's consumer — a turn's
 * reasoning_duration_ms → 「已思考 N 秒」. Same displayed-granularity
 * discipline as reasoningDurationValue: absent/invalid/non-positive ms and
 * sub-second spans (rounding below 1s) yield null — the header falls back
 * to the duration-less 「已思考」, never shows a 0.
 */
export function reasoningDurationFromMs(ms?: number | null): { seconds: number } | null {
  if (ms == null || !Number.isFinite(ms) || ms <= 0) return null
  const sec = Math.round(ms / 1000)
  if (sec < 1) return null
  return { seconds: sec }
}

/** Turn duration → the design's 「N 秒 / N 分钟」 granularity. Sub-second
 *  spans round to 0 and yield null — a duration below the displayed
 *  granularity is OMITTED, never shown as 0 (honesty contract). */
export function turnDurationValue(ms: number): { seconds: number } | { minutes: number } | null {
  if (!Number.isFinite(ms) || ms <= 0) return null
  const sec = Math.round(ms / 1000)
  if (sec < 1) return null
  if (sec < 60) return { seconds: sec }
  return { minutes: Math.round((ms / 60_000) * 10) / 10 }
}

function MessageHeader({
  role,
  timestamp,
  isBranch,
  turnMeta,
  done,
}: {
  role: 'user' | 'assistant' | 'tool'
  timestamp?: number
  isBranch?: boolean
  /** 补1: aggregate meta for assistant turns — null/undefined renders no
   *  meta segments (user/tool headers never receive one). */
  turnMeta?: TurnMeta | null
  /** R3 §4 遗留项: a SUCCESSFULLY completed assistant turn (never
   *  interrupted). Renders the design's neutral 「完成」 chip at the head of
   *  the meta tail (02 页 msg-meta 首段 `st ok 完成`, 读作中性而非绿色 —
   *  同款克制见 ToolCallDisplay W10). Failed/stopped turns keep their
   *  bespoke bubble-foot markers and get NO chip here (不重复叠加). Lives
   *  OUTSIDE the turn-meta group on purpose: a bare successful turn with no
   *  derivable tools/duration/cost still earns the verdict chip, while the
   *  group keeps its own all-or-nothing honesty contract. */
  done?: boolean
}) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, number | string>) => intl.formatMessage({ id }, values)
  const label =
    role === 'user' ? t('chat.message.header.user')
      : role === 'tool' ? t('chat.message.header.tool')
      : t('chat.message.header.assistant')
  const time = timestamp ? new Date(timestamp).toLocaleTimeString(intl.locale, { hour: '2-digit', minute: '2-digit' }) : ''
  // 补1 (02-chat msg-meta): the secondary tail after the timestamp. Each
  // segment renders only when its value was actually derived; with neither
  // derivable the whole tail (and its separator) stays unrendered. Placement
  // note: this line lives in the header row, the ActionToolbar is a separate
  // flow row at the bubble's foot — one can never cover the other. flex-wrap
  // keeps a long tail (chip + tools + cost + duration) inside the bubble
  // column on narrow panes instead of squeezing the row.
  const showDone = role === 'assistant' && done === true
  const toolsText = turnMeta != null && turnMeta.toolCount > 0
    ? t('chat.message.turnMeta.toolCalls', { count: turnMeta.toolCount })
    : null
  const duration = turnMeta?.durationMs != null ? turnDurationValue(turnMeta.durationMs) : null
  const durationText = duration == null
    ? null
    : 'seconds' in duration
      ? t('chat.message.turnMeta.durationSec', { seconds: duration.seconds })
      : t('chat.message.turnMeta.durationMin', { minutes: duration.minutes })
  // R3 §4 遗留项: the cost segment rides AFTER the duration (追加语义);
  // turnCostValue drops absent/0/non-finite costs upstream.
  const costValue = turnMeta?.costUsd != null ? turnCostValue(turnMeta.costUsd) : null
  const costText = costValue != null ? t('chat.message.turnMeta.cost', { cost: costValue }) : null
  const hasGroup = toolsText != null || durationText != null || costText != null
  // P2-17 (B1): the whole header used to be aria-hidden, leaving screen
  // readers unable to tell who spoke or when. The role label and <time> are
  // real content now; only the decorative separators and icon stay hidden.
  // The 完成 chip is likewise real content (its own text reads fine to AT);
  // the group's aria-label stays the numeric aggregate.
  return (
    <div className="flex flex-wrap items-center gap-xs text-label-xs text-on-surface-variant mb-xs">
      <span className="font-label-xs uppercase tracking-wide font-medium">{label}</span>
      {isBranch && (
        <>
          <span aria-hidden="true">·</span>
          <span className="material-symbols-outlined icon-xs" aria-hidden="true">fork_right</span>
          <span>{t('chat.message.branch')}</span>
        </>
      )}
      {time && (
        <>
          <span aria-hidden="true">·</span>
          <time dateTime={new Date(timestamp!).toISOString()} className="font-mono">{time}</time>
        </>
      )}
      {(showDone || hasGroup) && (
        <>
          <span aria-hidden="true">·</span>
          {showDone && (
            // a11y: the icon is decorative; the 完成 text is the content.
            <span
              data-testid="turn-meta-done"
              className="flex items-center gap-[2px] shrink-0 px-xs py-[1px] rounded-sm bg-surface-container-high text-on-surface-variant font-label-xs"
            >
              <span className="material-symbols-outlined icon-xs" aria-hidden="true">check_circle</span>
              {t('chat.message.turnMeta.done')}
            </span>
          )}
          {hasGroup && (
            <>
              {showDone && <span aria-hidden="true">·</span>}
              {/* a11y: the visual fragments are decorative; AT gets ONE coherent
                  sentence via the group's aria-label (汇总), not two bare
                  fragments ("2.4 分钟" alone has no antecedent). */}
              <span
                role="group"
                aria-label={t('chat.message.turnMeta.aria', { detail: [toolsText, durationText, costText].filter(Boolean).join(' · ') })}
                data-testid="turn-meta"
                className="flex items-center gap-xs text-on-surface-variant min-w-0"
              >
                {toolsText != null && <span aria-hidden="true">{toolsText}</span>}
                {toolsText != null && durationText != null && <span aria-hidden="true">·</span>}
                {durationText != null && <span aria-hidden="true" className="tabular-nums">{durationText}</span>}
                {durationText != null && costText != null && <span aria-hidden="true">·</span>}
                {costText != null && <span aria-hidden="true" className="font-mono tabular-nums">{costText}</span>}
              </span>
            </>
          )}
        </>
      )}
    </div>
  )
}

/* ─────── W3-4 — memory citation chips on an assistant answer ─────── */

/** "From your memories": one chip per memory the turn's prompt carried
 *  (the per-turn snapshot `send_message` returns). A chip with provenance
 *  jumps to the session that produced the memory — the same switchSession
 *  jump ContextBreakdownCard uses; entries without a source session
 *  (manual / imported) render as inert chips. Renders NOTHING when the
 *  list is empty or missing (temporary-chat bypass, zero injections,
 *  reloaded history). */
function MemoryCitationChips({ memories }: { memories?: InjectedMemory[] }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { currentSessionId, switchSession } = useSessions()
  if (!memories || memories.length === 0) return null
  return (
    <div
      className="flex flex-wrap items-center gap-xs mb-xs"
      data-testid="memory-citations"
    >
      <span className="font-label-xs text-on-surface-variant/70 uppercase tracking-wide">
        {t('chat.message.memoryCitations.title')}
      </span>
      {memories.map(m => {
        const jumpable = Boolean(m.sourceSessionId) && m.sourceSessionId !== currentSessionId
        const chipBody = (
          <>
            <span className="material-symbols-outlined icon-xs shrink-0" aria-hidden="true">psychology</span>
            <span className="max-w-[200px] truncate">{m.title}</span>
          </>
        )
        return jumpable ? (
          <button
            key={m.id}
            type="button"
            data-testid={`memory-citation-jump-${m.id}`}
            title={t('chat.context.injectedMemories.jump')}
            aria-label={`${t('chat.message.memoryCitations.jumpAria')}: ${m.title}`}
            className="inline-flex items-center gap-1 px-xs py-[1px] rounded-full bg-primary-container/40 text-on-primary-container text-label-xs font-medium hover:bg-primary/10/70 transition-colors cursor-pointer max-w-full min-w-0"
            onClick={() => { void switchSession(m.sourceSessionId!) }}
          >
            {chipBody}
          </button>
        ) : (
          <span
            key={m.id}
            data-testid={`memory-citation-${m.id}`}
            title={m.title}
            className="inline-flex items-center gap-1 px-xs py-[1px] rounded-full bg-surface-container-high text-on-surface-variant text-label-xs max-w-full min-w-0"
          >
            {chipBody}
          </span>
        )
      })}
    </div>
  )
}

/**
 * G4b (2026-10-15 parity review §5-G4b · 竞品 LobeChat 生成明细 / Chatbox
 * token 标注 / Cherry Studio tok/s): the assistant bubble's hover detail
 * strip — ↓in · ↑out · ↻cache(读/写) · 耗时 · tok/s · ≈$ — pinned to the
 * bubble's bottom edge, revealed on hover or when focus enters the bubble
 * (keyboard / mobile focus-within; the same trigger contract as the
 * ActionToolbar). Renders NOTHING without a matched timeline turn's usage
 * (absent = hidden): reloaded history whose load-time stamps match no turn
 * window stays clean. Turn-granularity by wire shape — the title says
 * 「本轮」. Presentation discipline: bare mono text row, no card chrome, no
 * shadow; every figure full-contrast (text-on-surface-variant / text-success
 * at full value — the axe color-contrast lesson), fragments aria-hidden with
 * ONE coherent group label, mirroring the header turn-meta grammar.
 * The cost segment reuses the G3 estimate convention (「≈{cost}」) and only
 * renders when the turn's cost is present and > 0 (unknown rate = no
 * figure); a LOCAL session adds the success-toned 「本地模型 · $0」 segment
 * (free by construction) — never instead of real usage, always on top of it.
 */
function MessageUsageStrip({ usage, costUsd, isLocal }: { usage: TurnUsageStats; costUsd: number | null; isLocal: boolean }) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, number | string>) => intl.formatMessage({ id }, values)
  const nf = useMemo(() => new Intl.NumberFormat(intl.locale), [intl.locale])
  const hasCache = usage.cacheReadTokens + usage.cacheCreationTokens > 0
  // The strip exists only on real token traffic — a matched turn with all
  // zero counts renders nothing (nothing to audit).
  if (usage.inputTokens <= 0 && usage.outputTokens <= 0 && !hasCache) return null
  const duration = usage.durationMs != null ? turnDurationValue(usage.durationMs) : null
  const durationText = duration == null
    ? null
    : 'seconds' in duration
      ? t('chat.message.turnMeta.durationSec', { seconds: duration.seconds })
      : t('chat.message.turnMeta.durationMin', { minutes: duration.minutes })
  // G3 estimate convention: 「≈$0.0214」 only when a real cost exists.
  const costValue = costUsd != null ? turnCostValue(costUsd) : null
  const costText = costValue != null ? t('chat.message.turnMeta.cost', { cost: costValue }) : null
  const speedText = usage.tokensPerSecond != null ? `${usage.tokensPerSecond} tok/s` : null
  const localText = isLocal ? t('chat.message.usageTurn.localFree') : null
  const detail = [
    t('timeline.stat.tokens.aria', { input: nf.format(usage.inputTokens), output: nf.format(usage.outputTokens) }),
    hasCache ? `${t('usage.stat.cache')} ${nf.format(usage.cacheReadTokens)}/${nf.format(usage.cacheCreationTokens)}` : null,
    durationText,
    speedText,
    costText,
    localText,
  ].filter(Boolean).join(' · ')
  return (
    <div
      data-testid="message-usage-strip"
      role="group"
      aria-label={t('chat.message.turnMeta.aria', { detail })}
      title={t('chat.message.usageTurn.title')}
      className="absolute left-0 top-full mt-[2px] z-10 flex max-w-full items-center gap-xs overflow-hidden whitespace-nowrap rounded-sm bg-surface-container-lowest px-xs py-[1px] font-mono text-label-xs tabular-nums text-on-surface-variant opacity-0 pointer-events-none transition-opacity group-hover:opacity-100 group-focus-within:opacity-100 focus-within:opacity-100"
    >
      <span aria-hidden="true" title={t('chat.context.inputTokens')}>↓ {nf.format(usage.inputTokens)}</span>
      <span aria-hidden="true">·</span>
      <span aria-hidden="true" title={t('chat.context.outputTokens')}>↑ {nf.format(usage.outputTokens)}</span>
      {hasCache && (
        <>
          <span aria-hidden="true">·</span>
          <span aria-hidden="true" title={t('usage.stat.cache')}>↻ {nf.format(usage.cacheReadTokens)}/{nf.format(usage.cacheCreationTokens)}</span>
        </>
      )}
      {durationText != null && <span aria-hidden="true" className="shrink-0">· {durationText}</span>}
      {speedText != null && <span aria-hidden="true" className="shrink-0">· {speedText}</span>}
      {costText != null && <span aria-hidden="true" className="shrink-0">· {costText}</span>}
      {localText != null && (
        <span aria-hidden="true" className="shrink-0 text-success">· {localText}</span>
      )}
    </div>
  )
}

function AttachmentPreview({ attachment }: { attachment: FileAttachment }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [open, setOpen] = useState(false)
  const isImage = isImagePath(attachment.path)

  // office Wave 1 (A5+B8a): non-image attachments render as a full FileCard
  // (open / reveal / save-as) — the old chip → lightbox detour whose only
  // action was "Open externally" wasted a click on the common case. The
  // lightbox stays image-only.
  if (!isImage) {
    return (
      <FileCard
        name={attachment.name}
        path={attachment.path}
        sizeBytes={attachment.size}
        source="attachment"
        extraction={attachment.extraction}
      />
    )
  }

  const handleClick = () => setOpen(true)

  return (
    <>
      <Button
        variant="outline"
        onClick={handleClick}
        className="group/att inline-flex items-center gap-xs px-sm py-xs h-auto bg-surface-container-low hover:bg-surface-container text-on-surface-variant hover:text-primary"
        title={attachment.path}
        aria-label={t('chat.message.attachment.open')}
      >
        <img
          src={convertFileSrc(attachment.path)}
          alt={attachment.name}
          className="h-8 w-8 rounded-sm object-cover shrink-0"
          onError={(e) => { (e.currentTarget as HTMLImageElement).style.display = 'none' }}
        />
        <span className="font-label-sm max-w-[160px] truncate">{attachment.name}</span>
      </Button>

      {/* Lightbox scrim panel — the dark fill must beat the glass-overlay
          base the Modal now carries, hence the important modifier (custom
          glass utilities sort after core utilities in the layer). */}
      <Modal
        open={open}
        onClose={() => setOpen(false)}
        size="full"
        showCloseButton={false}
        title={attachment.name}
        closeLabel={t('chat.message.attachment.close')}
        className="bg-scrim-strong! backdrop-blur-sm p-lg"
      >
        <img
          src={convertFileSrc(attachment.path)}
          alt={attachment.name}
          className="max-h-[90vh] max-w-[90vw] object-contain rounded-lg shadow-e5"
          onClick={(e) => e.stopPropagation()}
        />
        <Button
          variant="ghost"
          onClick={() => setOpen(false)}
          aria-label={t('chat.message.attachment.close')}
          className="absolute top-md right-md text-on-surface-variant hover:text-on-surface bg-surface-container-lowest/80 rounded-full p-sm"
        >
          <span className="material-symbols-outlined">close</span>
        </Button>
      </Modal>
    </>
  )
}

export const MessageBubble = memo(function MessageBubble({ message, messageIndex, isBranch, onViewDiff, onViewDiffMulti, rewindTurnIndex, onRewind, durationLookup, costWindows, reasoningWindows, regenerate, onEditMessage, searchFlash, isFirstAssistantOfTurn }: MessageBubbleProps) {
  const isUser = message.role === 'user'
  const [isBranching, setIsBranching] = useState(false)
  const [pendingBranch, setPendingBranch] = useState(false)
  const [pendingRewind, setPendingRewind] = useState(false)
  const [isRewinding, setIsRewinding] = useState(false)
  const [isRegenerating, setIsRegenerating] = useState(false)
  const [reportOpen, setReportOpen] = useState(false)
  const { sendMessage, feedback, recordFeedback, isQuerying, messages: listMessages } = useChat()
  const { currentSessionId, switchSession, refreshSessions } = useSessions()
  // G4b: the session's per-turn usage rows (trace_timeline, module-cached —
  // one IPC per session across every bubble on screen, busted when the run
  // settles). isLocal feeds the 「本地模型 · $0」 strip segment.
  const usageInfo = useTurnUsage(currentSessionId ?? null, isQuerying)
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  // Settings R3 T11: the Explore/Terminal/Changes grouping classifies calls
  // via the read-only map seeded from `list_tools`; that seed is a one-time
  // async IPC, so subscribe to its arrival and re-group when it lands
  // (until then classifyTool's name heuristics carry the decision).
  // The snapshot value itself is unused — subscribing is what re-renders
  // this bubble on map mutation.
  useSyncExternalStore(subscribeToolReadOnlyMap, getToolReadOnlyMapVersion)
  useEffect(() => {
    void ensureToolReadOnlyMap()
    // fire-once latch inside ensureToolReadOnlyMap; the version subscription
    // above re-renders this bubble when the seed lands (map mutation).
  }, [])

  // PM-12: the rating lives in the persisted per-session store, not in a
  // local useState that died with the component tree.
  const feedbackKey = messageFeedbackKey(message.timestamp, message.content)
  const myRating = feedback[feedbackKey] ?? null

  const toggleFeedback = (rating: FeedbackRating) => {
    recordFeedback(feedbackKey, myRating === rating ? null : rating)
      .catch((e) => toastError(t('chat.message.feedback.failed'), e))
  }

  const handleCopy = () => {
    navigator.clipboard.writeText(message.content).catch((e) => toastError(t('chat.toast.copyFailed'), e))
  }

  // B1 §4-7: TRUE regenerate — rewind the session to the checkpoint before
  // the preceding user turn, then re-send that turn verbatim (text +
  // attachment paths). Direct execution with a toast; no modal, and the
  // canned-prompt fake from the first round is gone.
  const handleRegenerate = async () => {
    if (!regenerate || !onRewind || isRegenerating) return
    setIsRegenerating(true)
    try {
      await onRewind(regenerate.turnIndex)
      // P2-1: sendMessage RESOLVES false when the backend refuses the resend
      // (budget / concurrent-query / goal guards — it never throws; the error
      // surface is already up via setChatError). The success toast must only
      // fire on acceptance — the matching correct branch is commitEdit in
      // Chat.tsx.
      const ok = await sendMessage(
        regenerate.content,
        regenerate.attachmentPaths.length > 0 ? regenerate.attachmentPaths : undefined,
      )
      if (ok) toast.success(t('chat.message.regenerate.started'))
    } catch (error) {
      toastError(t('chat.message.regenerate.failed'), error)
    } finally {
      setIsRegenerating(false)
    }
  }

  const confirmRewind = async () => {
    if (rewindTurnIndex == null || !onRewind) return
    setPendingRewind(false)
    setIsRewinding(true)
    try {
      await onRewind(rewindTurnIndex)
      toast.success(t('chat.message.rewind.success'))
    } catch (error) {
      toastError(t('chat.message.rewind.failed'), error)
    } finally {
      setIsRewinding(false)
    }
  }

  const handleBranch = () => {
    if (!currentSessionId || isBranching) return
    setPendingBranch(true)
  }

  const confirmBranch = async () => {
    if (!currentSessionId) return
    setPendingBranch(false)
    setIsBranching(true)
    try {
      const newSession = await api.branchSession(currentSessionId, messageIndex)
      await refreshSessions()
      await switchSession(newSession.id)
      toast.success(t('chat.message.branch.success'))
    } catch (error) {
      toastError(t('chat.message.branch.failed'), error)
    } finally {
      setIsBranching(false)
    }
  }

  const hasAttachments = message.file_attachments && message.file_attachments.length > 0
  const hasReport = !!message.research_report
  // P2-14 (§4-15): detectArtifacts runs a full regex sweep over the body —
  // memoize on content so streaming re-renders of unchanged messages skip it.
  const detectedArtifacts = useMemo(
    () => (!isUser ? detectArtifacts(message.content) : []),
    [isUser, message.content],
  )

  // G4b: the strip consumes the matched turn's tokens; keeping the rows in a
  // local lets the memo depend on the array identity, not the wrapper object.
  const usageTurns = usageInfo.turns
  // 补1 (R2 审查 §7.1 · 02-chat msg-meta): assistant-turn aggregate — the
  // tool-call count and the wall-clock span of the conversation turn this
  // message belongs to. Turn math matches the list parents' convention
  // (firstAssistantOfTurnFlags / rewindInfoFor): a turn spans from the last
  // user message at/before this message up to (excluding) the NEXT user
  // message; non-user roles interleave freely. Count sums every non-user
  // message's tool_calls in that window (one committed assistant message
  // carries the whole turn's calls); duration is lastTs − firstTs over the
  // same window. HONESTY CONTRACT: a dimension that cannot be derived
  // (no/invalid timestamps, zero-width span, zero calls) is simply absent
  // from TurnMeta — never a 0, never a placeholder.
  //
  // R3 §4 遗留项 — turn cost IS now part of the aggregate: MessageArea
  // projects TimelineTurn.cost_usd (trace_timeline IPC, the same read as the
  // tool-duration lookup) into TurnCostWindows keyed by TIME, and this memo
  // matches its turn window against them (matchTurnCost — timestamp overlap,
  // rewind-proof; the engine's envelope turn numbers are NOT usable after a
  // rewind). Absent/0 costs never produce a window upstream, so a turn
  // without spend data matches nothing and the segment stays unrendered.
  // Degradation note: reloaded history carries load-time stamps
  // (commands_sessions.rs chrono_timestamp per message), so its zero-width
  // windows match nothing — the same honest degradation the duration segment
  // already shows on reloaded history.
  const turnMeta = useMemo<TurnMeta | null>(() => {
    if (isUser || message.role === 'tool') return null
    // Only trust the context list when this bubble IS that list's row
    // (identity) — isolated renders (tests, previews) fall back to the
    // single message so the derivation stays over real data either way.
    const list = listMessages != null && Array.isArray(listMessages) && listMessages[messageIndex] === message
      ? listMessages
      : [message]
    let startIdx = 0
    for (let i = Math.min(messageIndex, list.length - 1); i >= 0; i--) {
      if (list[i]?.role === 'user') { startIdx = i; break }
    }
    let endIdx = list.length - 1
    for (let i = messageIndex + 1; i < list.length; i++) {
      if (list[i]?.role === 'user') { endIdx = i - 1; break }
    }
    let toolCount = 0
    let lastTs = 0
    const firstRaw = list[startIdx]?.timestamp
    const firstTs = typeof firstRaw === 'number' && Number.isFinite(firstRaw) && firstRaw > 0 ? firstRaw : null
    for (let i = startIdx; i <= endIdx; i++) {
      const m = list[i]
      if (!m) continue
      if (Array.isArray(m.tool_calls)) toolCount += m.tool_calls.length
      if (typeof m.timestamp === 'number' && Number.isFinite(m.timestamp) && m.timestamp > lastTs) lastTs = m.timestamp
    }
    const durationMs = firstTs != null && lastTs > firstTs ? lastTs - firstTs : null
    const costUsd = firstTs != null
      ? matchTurnCost(costWindows, firstTs, Math.max(lastTs, firstTs))
      : null
    // T1 (R3 §4.1 Reasoning 时长): the SAME window match as the cost leg,
    // against reasoningWindows from the same timeline read. Feeds the
    // committed thinking block's header, not this header's meta tail — the
    // field rides on TurnMeta purely so both derivations share one span scan.
    const reasoningMs = firstTs != null
      ? matchTurnReasoning(reasoningWindows, firstTs, Math.max(lastTs, firstTs))
      : null
    // G4b: the turn's token usage — same overlap match, against the hook's
    // timeline rows. Feeds the hover strip only (the header tail stays
    // tools/duration/cost; the strip owns tokens + duration + tok/s).
    const usageTurn = firstTs != null
      ? matchTurnUsage(usageTurns, firstTs, Math.max(lastTs, firstTs))
      : null
    const usage = usageTurn != null ? turnUsageStats(usageTurn) : null
    if (toolCount === 0 && durationMs == null && costUsd == null && reasoningMs == null && usage == null) return null
    return { toolCount, durationMs, costUsd, reasoningMs, usage }
  }, [isUser, message, messageIndex, listMessages, costWindows, reasoningWindows, usageTurns])

  if (isUser) {
    return (
      <>
      <Message from="user" className={cn('flex justify-end', searchFlash && 'search-flash rounded-2xl')}>
        <MessageContent className="max-w-[80%]">
          <MessageHeader role="user" timestamp={message.timestamp} isBranch={isBranch} />
          {hasAttachments && (
            <div className="flex flex-wrap gap-xs mb-xs justify-end">
              {message.file_attachments!.map((att, i) => (
                <AttachmentPreview key={i} attachment={att} />
              ))}
            </div>
          )}
          <div className="bg-primary-fixed text-on-primary-fixed px-lg py-md rounded-2xl rounded-tr-none shadow-e1">
            <p className="font-body-md whitespace-pre-wrap">
              {/* P2-1 (§4): user messages stay plain text, but pasted URLs
                  become clickable — still no block-level markdown, so the
                  user's own words keep their exact line breaks. */}
              <LinkifiedText text={message.content} />
            </p>
          </div>
          <ActionToolbar className="gap-sm mt-xs justify-end opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 focus-within:opacity-100 transition-opacity">
            <Button
              aria-label={t('chat.message.copy.aria')}
              onClick={handleCopy}
              className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
            >
              <span className="material-symbols-outlined icon-md" aria-hidden="true">content_copy</span>
            </Button>
            {/* B1 §4-8: composer-based edit — same rewindability gate as the
                rewind button (a checkpoint must exist at or after this turn,
                otherwise there is nothing to rewind onto). */}
            {onEditMessage && rewindTurnIndex != null && (
              <Button
                aria-label={t('chat.message.edit.aria')}
                onClick={() => onEditMessage(messageIndex)}
                disabled={isQuerying}
                className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                title={t('chat.message.edit.button')}
              >
                <span className="material-symbols-outlined icon-md" aria-hidden="true">edit</span>
              </Button>
            )}
            <Button
              aria-label={t('chat.message.branch.aria')}
              onClick={handleBranch}
              disabled={isBranching || !currentSessionId}
              className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
              title={t('chat.message.branch.button')}
            >
              <span className="material-symbols-outlined icon-md" aria-hidden="true">
                {isBranching ? 'hourglass_empty' : 'fork_right'}
              </span>
            </Button>
            {rewindTurnIndex != null && (
              <Button
                aria-label={t('chat.message.rewind.aria')}
                onClick={() => setPendingRewind(true)}
                disabled={isRewinding}
                className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                title={t('chat.message.rewind.button')}
              >
                <span className="material-symbols-outlined icon-md" aria-hidden="true">
                  {isRewinding ? 'hourglass_empty' : 'undo'}
                </span>
              </Button>
            )}
          </ActionToolbar>
        </MessageContent>
      </Message>
      <ConfirmDialog
        open={pendingBranch}
        title={t('chat.message.branch.confirm.title')}
        message={t('chat.message.branch.confirm.message')}
        confirmLabel={t('chat.message.branch.button')}
        cancelLabel={t('chat.message.branch.confirm.cancel')}
        busy={isBranching}
        onConfirm={() => void confirmBranch()}
        onCancel={() => setPendingBranch(false)}
      />
      <ConfirmDialog
        open={pendingRewind}
        title={t('chat.message.rewind.confirm.title')}
        message={t('chat.message.rewind.confirm.message')}
        confirmLabel={t('chat.message.rewind.confirm.confirm')}
        cancelLabel={t('chat.message.rewind.confirm.cancel')}
        busy={isRewinding}
        onConfirm={() => void confirmRewind()}
        onCancel={() => setPendingRewind(false)}
      />
    </>
  )
}

  /* Assistant + Tool variants — share most of the chrome; tool messages
   * get a slightly muted style and don't carry the like / regen /
   * report buttons (those actions only make sense for assistant text). */
  const isTool = message.role === 'tool'

  // Settings R3 T9: a committed assistant message's `thinking` field renders
  // as the same collapsed Reasoning block the live stream uses, gated by the
  // three-tier display pref (lib/thinkingPref). Hydration note: the engine's
  // session projection does not persist thinking yet, so reloaded history
  // carries none today — this renders whenever the field IS present
  // (hydration data or future persistence) and stays silent otherwise.
  const showThinking = !isTool && shouldShowThinking(
    readShowThinkingPref(),
    !!message.thinking,
    isFirstAssistantOfTurn === true,
  )

  // R3 §4 遗留项 (Reasoning 头分态): a COMMITTED block belongs to a settled
  // turn, so the live 「思考中…」 label (chat.streaming.thinking —
  // StreamingResponse only) would lie here. Completed header: 「已思考 N 秒」
  // when the turn's reasoning duration is derivable, 「已思考」 otherwise
  // (never an invented duration).
  // T1 (2026-10-15): the turn-level projection DID land —
  // TimelineTurn.reasoning_duration_ms rides the trace-timeline IPC into
  // turnMeta.reasoningMs (timestamp-window match, the same one as the cost
  // leg); reasoningDurationFromMs refuses absent/sub-second values so the
  // header shows the duration ONLY when the backend projected one.
  const reasoningDur = reasoningDurationFromMs(turnMeta?.reasoningMs) ?? reasoningDurationValue()
  const reasoningHeader = reasoningDur != null
    ? intl.formatMessage({ id: 'chat.streaming.thoughtSeconds' }, { seconds: reasoningDur.seconds })
    : intl.formatMessage({ id: 'chat.streaming.thought' })

  return (
    <Message from={isTool ? 'system' : 'assistant'} className={cn('flex gap-md max-w-4xl group', searchFlash && 'search-flash rounded-2xl')}>
      <MessageAvatar from="assistant" icon={isTool ? 'build' : 'smart_toy'} />
      {/* relative anchors the G4b hover usage strip (top-full = the bubble's
          bottom edge); it renders only on assistant answers. */}
      <MessageContent className="space-y-md flex-1 relative">
        <MessageHeader role={isTool ? 'tool' : 'assistant'} timestamp={message.timestamp} turnMeta={isTool ? null : turnMeta} done={isTool ? undefined : message.interrupted !== true} />
        {/* W3-4: citation chips for the memories this turn's prompt carried —
            hidden entirely for a bypass / zero-injection turn (empty list). */}
        {!isTool && <MemoryCitationChips memories={message.injected_memories} />}
        {/* Settings R3 T9: collapsed thinking above the answer, matching the
            live stream's placement (StreamingResponse renders it as the
            first block of the content column). */}
        {showThinking && (
          // R3 §4 遗留项 (Reasoning 头分态): committed header is the COMPLETED
          // state — see reasoningHeader above; the live 「思考中…」 stays in
          // StreamingResponse.
          <Reasoning header={reasoningHeader} defaultOpen={false}>
            <p className="whitespace-pre-wrap">{message.thinking}</p>
          </Reasoning>
        )}
        {/* Aurora redesign 2026-10 (02 页 P1 去卡片化): the assistant body is
            avatar + BARE text flow (shannon-ui.css `.msg-ai .body`) — no
            border/bg/shadow card. Visual hierarchy belongs to the content
            blocks themselves (tool cards, diff cards, code blocks), which
            keep their own chrome below. */}
        <div className="min-w-0">
          <ResponseStream className="font-body-md text-on-surface prose prose-sm max-w-none prose-p:my-xs prose-pre:bg-surface-container prose-pre:p-md prose-pre:rounded-lg prose-code:text-primary prose-code:before:content-[''] prose-code:after:content-['']">
            <Markdown>{message.content}</Markdown>
          </ResponseStream>
          {detectedArtifacts.length > 0 && (
            <div className="mt-md">
              <ArtifactChipList artifacts={detectedArtifacts} />
            </div>
          )}
          {message.tool_calls && message.tool_calls.length > 0 && (
            <div className="mt-md space-y-sm">
              {(() => {
                const changedPaths = message.tool_calls
                  .filter(tc => tc.status === 'completed' && !tc.is_error)
                  .map(tc => extractFilePath(tc.tool_name, tc.tool_input))
                  .filter((p): p is string => p != null)
                const uniquePaths = Array.from(new Set(changedPaths))
                return uniquePaths.length > 0 ? (
                  <FileChangesCard
                    paths={uniquePaths}
                    rewindable={rewindTurnIndex != null && onRewind != null}
                    onReview={() => onViewDiff(uniquePaths[0])}
                    onReviewAll={uniquePaths.length > 1 ? () => onViewDiffMulti?.(uniquePaths) : undefined}
                    onUndo={() => setPendingRewind(true)}
                  />
                ) : null
              })()}
              {/* P2-⑨ (ZCode delta): a run of consecutive same-tool failures is
                  prefaced by a retry-chain banner linking to the turn timeline
                  — the long-horizon "failed → retried → recovered" narrative
                  stays readable without expanding every card. */}
              {/* Settings R3 T11 (C6): committed history folds runs of
                  consecutive same-kind calls (Explore / Terminal / Changes)
                  into ToolGroupCards — see lib/toolGrouping. Streaming keeps
                  per-card rendering (controller ruling R10: grouping a live
                  stream re-folds the tail every flush). The existing special
                  cards — retry chains, subagent blocks — keep their bespoke
                  rendering and break any adjacent group. */}
              {(() => {
                const tcs = message.tool_calls
                const nodes: React.ReactNode[] = []
                const renderTool = (tc: ToolCall, key: string) =>
                  tc.tool_name === 'agent_spawn' ? (
                    <SubagentBlock key={key} toolCall={tc} />
                  ) : (
                    <ToolCallDisplay
                      key={key}
                      toolCall={tc}
                      onViewDiff={onViewDiff}
                      durationMs={durationLookup?.get(tc.tool_use_id)}
                    />
                  )
                // Pass 1 — the P2-⑨ retry-chain walk, now emitting units:
                // the banner is an ORDERED MARKER (emitted at its sequence
                // position in pass 2 — fix round 1: hoisting it into the
                // node list early piled every banner at the top of the tool
                // area, breaking the failure→retry narrative); its chain
                // members stay non-groupable, and everything else is a
                // grouping candidate except subagent spawns and calls that
                // carry an artifact FileCard (folding would hide the card).
                const units: ToolGroupUnit<ToolCall, React.ReactNode>[] = []
                let i = 0
                while (i < tcs.length) {
                  const tc = tcs[i]
                  if (tc.status === 'error') {
                    let j = i
                    while (
                      j + 1 < tcs.length &&
                      tcs[j + 1].status === 'error' &&
                      tcs[j + 1].tool_name === tc.tool_name
                    ) { j++ }
                    const chainLen = j - i + 1
                    if (chainLen >= 2) {
                      // D7: inline each attempt's first error line so the
                      // failure→retry→recovery narrative reads without
                      // expanding every card.
                      const reasons = tcs.slice(i, j + 1).map(tc => {
                        const line = (tc.result ?? '').split('\n').find(l => l.trim()) ?? ''
                        return line.trim().slice(0, 120)
                      })
                      units.push({
                        type: 'marker',
                        marker: (
                          <RetryChainBanner
                            key={`chain-${tc.tool_use_id}`}
                            count={chainLen}
                            reasons={reasons}
                          />
                        ),
                      })
                      for (let k = i; k <= j; k++) units.push({ type: 'tool', tc: tcs[k], groupable: false })
                      i = j + 1
                      continue
                    }
                  }
                  units.push({
                    type: 'tool',
                    tc,
                    groupable: tc.tool_name !== 'agent_spawn' && !carriesArtifactCard(tc),
                  })
                  i++
                }
                // Pass 2 — ordered emission: adjacent same-kind merge, gated
                // on the per-kind switches (lib/toolGrouping
                // readGroupingPrefs, default ON); markers pass through at
                // their sequence position and reopen the segmentation.
                for (const seg of groupToolSegments(units, readGroupingPrefs())) {
                  if (seg.type === 'group') {
                    const items = seg.items
                    nodes.push(
                      <ToolGroupCard
                        key={`group-${seg.kind}-${items[0].tool_use_id}`}
                        kind={seg.kind}
                        count={items.length}
                        firstToolName={items[0].tool_name}
                        lastToolName={items[items.length - 1].tool_name}
                      >
                        {items.map(tc => (
                          <ToolCallDisplay
                            key={tc.tool_use_id}
                            toolCall={tc}
                            onViewDiff={onViewDiff}
                            durationMs={durationLookup?.get(tc.tool_use_id)}
                          />
                        ))}
                      </ToolGroupCard>,
                    )
                  } else if (seg.type === 'marker') {
                    nodes.push(seg.marker)
                  } else {
                    nodes.push(renderTool(seg.tc, seg.tc.tool_use_id))
                  }
                }
                return nodes
              })()}
            </div>
          )}
        </div>
        {/* D6 (keep the partial output) + OBS1 (unify the failed half): a
            cancelled or failed run's committed partial answer is visibly
            marked — the reply stays, but it is not a complete one. One chip
            family, two tones: the neutral surface chip reads "stopped" (user
            stop), the error-toned one reads "failed" (the turn failed
            mid-step) — the same shapes the wire's `interrupted` +
            `interrupted_reason` carry. Style mirrors the sandbox-denied
            pill; the failed tone reuses the error-container pair. */}
        {!isTool && message.interrupted === true && message.interrupted_reason === 'failed' && (
          <div
            className="flex items-center gap-xs self-start px-xs py-[1px] rounded-sm bg-error-container text-on-error-container font-label-xs w-fit"
            data-testid="message-failed-marker"
          >
            <span className="material-symbols-outlined icon-xs" aria-hidden="true">error</span>
            {t('chat.message.failedPartial')}
          </div>
        )}
        {!isTool && message.interrupted === true && message.interrupted_reason !== 'failed' && (
          <div
            className="flex items-center gap-xs self-start px-xs py-[1px] rounded-sm bg-surface-container-high text-on-surface-variant font-label-xs w-fit"
            data-testid="message-stopped-marker"
          >
            <span className="material-symbols-outlined icon-xs" aria-hidden="true">stop_circle</span>
            {t('chat.message.stopped')}
          </div>
        )}
        <ActionToolbar className="opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 focus-within:opacity-100 transition-opacity">
          <Button aria-label={t('chat.message.copy.aria')} onClick={handleCopy} className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30">
            <span className="material-symbols-outlined icon-md" aria-hidden="true">content_copy</span>
          </Button>
          {!isTool && (
            <>
              <Button aria-label={t('chat.message.like.aria')} aria-pressed={myRating === 'up'} onClick={() => toggleFeedback('up')} className={cn('flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30', myRating === 'up' ? 'text-primary' : 'text-on-surface-variant')}>
                <span className="material-symbols-outlined icon-md" aria-hidden="true">{myRating === 'up' ? 'thumb_up' : 'thumb_up_off_alt'}</span>
              </Button>
              <Button aria-label={t('chat.message.dislike.aria')} aria-pressed={myRating === 'down'} onClick={() => toggleFeedback('down')} className={cn('flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30', myRating === 'down' ? 'text-error' : 'text-on-surface-variant')}>
                <span className="material-symbols-outlined icon-md" aria-hidden="true">{myRating === 'down' ? 'thumb_down' : 'thumb_down_off_alt'}</span>
              </Button>
              {/* B1 §4-7: true regenerate renders ONLY on the last assistant
                  message (presence of the `regenerate` payload), and only
                  when the session is idle. */}
              {regenerate && (
                <Button
                  aria-label={t('chat.message.regenerate.aria')}
                  onClick={() => void handleRegenerate()}
                  disabled={isQuerying || isRegenerating}
                  className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-40"
                >
                  <span className="material-symbols-outlined icon-md" aria-hidden="true">
                    {isRegenerating ? 'hourglass_empty' : 'refresh'}
                  </span>
                </Button>
              )}
              <Button
                aria-label={t('chat.message.branch.aria')}
                onClick={handleBranch}
                disabled={isBranching || !currentSessionId}
                className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                title={t('chat.message.branch.button')}
              >
                <span className="material-symbols-outlined icon-md" aria-hidden="true">
                  {isBranching ? 'hourglass_empty' : 'fork_right'}
                </span>
              </Button>
            </>
          )}
          {hasReport && (
            <Button
              aria-label={t('chat.message.report.aria')}
              onClick={() => setReportOpen(true)}
              className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
            >
              <span className="material-symbols-outlined icon-md" aria-hidden="true">article</span>
              <span className="text-label-sm">{t('chat.message.report')}</span>
            </Button>
          )}
        </ActionToolbar>
        {hasReport && (
          <ResearchReportModal
            report={message.research_report!}
            open={reportOpen}
            onClose={() => setReportOpen(false)}
          />
        )}
        {/* G4b: hover/focus usage detail — pinned below the bubble's bottom
            edge (absolute, zero layout footprint). Only assistant answers;
            only when a timeline turn's usage actually matched. */}
        {!isTool && turnMeta?.usage != null && (
          <MessageUsageStrip usage={turnMeta.usage} costUsd={turnMeta.costUsd} isLocal={usageInfo.isLocal} />
        )}
      </MessageContent>
    </Message>
  )
})

// FILE_MUTATING_TOOLS moved to lib/fileRefs (§4 P0-B) so chips, the dock's
// disk-artifact hook and this diff gate share one definition.

/**
 * Batch C2 (2026-09-20 delta analysis): the in-chat change summary grew
 * "+x −y" line counts and an in-place 撤销 (rewind to this turn's checkpoint,
 * reusing the user-message confirm flow) — the ZCode diff-card grammar.
 *
 * R2 审查 G8 (docs/reviews/2026-10-14-design-parity-audit-r2.md §4): 回合级
 * 汇总卡与 InlineDiffCard(单文件卡)同屏会把路径/增删重复一遍——文件数 >2 时
 * 默认折叠为一行「N 个文件 · +x −y」(评审/撤销操作保留),点开才展开路径列表;
 * ≤2 个文件维持现状(路径内联,一眼可读)。
 */
const FILE_CHANGES_COLLAPSE_THRESHOLD = 2

function FileChangesCard({ paths, rewindable, onReview, onReviewAll, onUndo }: {
  paths: string[]
  rewindable: boolean
  onReview: () => void
  onReviewAll?: () => void
  onUndo: () => void
}) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, number>) => intl.formatMessage({ id }, values)
  const [stats, setStats] = useState<DiffLineStats | null>(null)
  const collapsible = paths.length > FILE_CHANGES_COLLAPSE_THRESHOLD
  const [pathsExpanded, setPathsExpanded] = useState(false)

  useEffect(() => {
    let cancelled = false
    summarizeDiffLineStats(paths).then(s => { if (!cancelled) setStats(s) })
    return () => { cancelled = true }
    // paths identity is stable per message (derived from tool_calls)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [paths.join('\n')])

  const hasCounts = stats != null && (stats.additions > 0 || stats.deletions > 0)
  return (
    // Aurora 2026-10 语义色归位: additions/acceptance wear the success token
    // (moss green, shannon-ui.css `.plus`) — tertiary/amber is reserved for
    // warnings. Deletions stay error.
    <div className="px-md py-xs rounded-lg bg-success-container border border-success/20" data-testid="file-changes-card">
      <div className="flex items-center justify-between gap-sm">
        <div className="flex items-center gap-sm min-w-0">
          <span className="material-symbols-outlined icon-sm text-success shrink-0" aria-hidden="true">difference</span>
          {collapsible ? (
            // G8 collapsed summary doubles as the disclosure toggle — the
            // review/undo actions below stay visible either way.
            <button
              type="button"
              data-testid="file-changes-card-toggle"
              aria-expanded={pathsExpanded}
              onClick={() => setPathsExpanded(v => !v)}
              className="flex items-center gap-xs font-label-sm text-on-surface truncate cursor-pointer rounded-sm hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 min-w-0"
            >
              <span className="truncate">{t('chat.message.filesChanged.count', { count: paths.length })}</span>
              {hasCounts && (
                <span className="font-mono text-label-xs tabular-nums shrink-0">
                  <span aria-hidden="true">· </span>
                  <span className="text-success">+{stats!.additions}</span>{' '}
                  <span className="text-error">−{stats!.deletions}</span>
                </span>
              )}
              <span className="material-symbols-outlined icon-sm text-on-surface-variant shrink-0" aria-hidden="true">
                {pathsExpanded ? 'expand_less' : 'expand_more'}
              </span>
            </button>
          ) : (
            <>
              <span className="font-label-sm text-on-surface truncate">
                {t('chat.message.filesChanged', { count: paths.length })}
              </span>
              <span className="font-label-xs text-on-surface-variant truncate font-mono" title={paths.join('\n')}>
                {paths.join(', ')}
              </span>
              {hasCounts && (
                <span
                  className="font-mono text-label-xs tabular-nums shrink-0"
                  aria-label={t('chat.message.diffStats.aria', { additions: stats!.additions, deletions: stats!.deletions })}
                >
                  <span className="text-success">+{stats!.additions}</span>{' '}
                  <span className="text-error">−{stats!.deletions}</span>
                </span>
              )}
            </>
          )}
        </div>
        <div className="flex items-center gap-xs shrink-0">
          <Button
            variant="ghost"
            size="sm"
            className="gap-xs px-sm py-xs text-success hover:bg-success/10"
            onClick={() => (paths.length > 1 && onReviewAll ? onReviewAll() : onReview())}
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">difference</span>
            {paths.length > 1 ? t('chat.message.reviewAll') : t('chat.message.diff')}
          </Button>
          {rewindable && (
            <Button
              variant="ghost"
              size="sm"
              aria-label={t('chat.message.rewind.aria')}
              title={t('chat.message.rewind.button')}
              className="gap-xs px-sm py-xs text-on-surface-variant hover:text-error hover:bg-error/10"
              onClick={onUndo}
            >
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">undo</span>
              {t('chat.message.rewind.button')}
            </Button>
          )}
        </div>
      </div>
      {collapsible && pathsExpanded && (
        <ul className="mt-xs space-y-[2px]" data-testid="file-changes-card-paths">
          {paths.map(p => (
            <li key={p} className="flex items-start gap-xs font-label-xs text-on-surface-variant min-w-0">
              <span className="font-mono text-on-surface-variant shrink-0" aria-hidden="true">·</span>
              <span className="font-mono truncate" title={p}>{p}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

function extractFilePath(toolName: string, input: unknown): string | null {
  const raw = extractToolInputPath(input)
  if (!raw) return null
  return FILE_MUTATING_TOOLS.has(toolName) ? raw : null
}

/** A COMPLETED file-mutating call with a path input renders an interactive
 *  artifact FileCard under its tool block (office Wave 1 A5 — the exact
 *  `canDiff` signal ToolCallDisplay gates its card on). The card is the
 *  user's interaction surface (preview / batch run / diff), so such a call
 *  must stay a plain card: folding it into a ToolGroupCard hid the artifact
 *  behind the collapsed group (T11 grouping regression). */
function carriesArtifactCard(tc: ToolCall): boolean {
  return extractFilePath(tc.tool_name, tc.tool_input) != null && tc.status === 'completed' && !tc.is_error
}

/** P2-⑨: banner preceding a run of consecutive same-tool failures — links
 *  to the session's turn timeline where the retry narrative is visualized. */
function RetryChainBanner({ count, reasons = [] }: { count: number; reasons?: string[] }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const navigate = useNavigate()
  const { currentSessionId } = useSessions()
  return (
    <div className="px-md py-xs rounded-lg bg-error-container border border-error/20" data-testid="retry-chain-banner">
      <div className="flex items-center gap-sm">
        <span className="material-symbols-outlined icon-sm text-error shrink-0" aria-hidden="true">replay</span>
        <span className="font-label-sm text-error flex-1 truncate">
          {intl.formatMessage({ id: 'chat.message.retryChain' }, { count })}
        </span>
        {currentSessionId && (
          <Button
            variant="ghost"
            size="sm"
            className="shrink-0 gap-xs px-sm py-xs text-on-surface-variant hover:text-primary"
            onClick={() => navigate(`/timeline/${currentSessionId}`)}
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">timeline</span>
            {t('chat.message.retryChain.view')}
          </Button>
        )}
      </div>
      {reasons.length > 0 && (
        <ul className="mt-xs space-y-[2px]">
          {reasons.map((line, idx) => (
            <li key={idx} className="flex items-start gap-xs font-label-xs text-on-surface-variant">
              <span className="font-mono text-on-surface-variant shrink-0" aria-hidden="true">
                {intl.formatMessage({ id: 'chat.message.retryChain.attempt' }, { n: idx + 1 })}
              </span>
              <span className="truncate" title={line}>{line}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

/** P1-⑤ telemetry: compact wall-clock label — 842 ms · 5.2 s · 1m04s. */
export function formatToolDuration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)} ms`
  const s = ms / 1000
  if (s < 60) return `${s < 10 ? s.toFixed(1) : Math.round(s)} s`
  const m = Math.floor(s / 60)
  return `${m}m${String(Math.round(s % 60)).padStart(2, '0')}s`
}

export const ToolCallDisplay = memo(function ToolCallDisplay({ toolCall, onViewDiff, durationMs: durationMsProp }: { toolCall: ToolCall; onViewDiff: (path: string) => void; durationMs?: number }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  // P1-⑤: error cards default open — the failure text is the content the
  // user asked about; healthy calls stay collapsed (ZCode delta ⑤).
  const [expanded, setExpanded] = useState(toolCall.is_error === true)
  const statusIcon = toolCall.status === 'running' ? 'hourglass_empty' : toolCall.status === 'error' ? 'error' : 'check_circle'
  // R2 审查 W10 (docs/reviews/2026-10-14-design-parity-audit-r2.md §3.1):
  // 完成对勾是中性形态(02 页 .tool .st ok 的克制读法)——琥珀只留高危/暂停,
  // 绿色只留 diff 新增/接受;error 红与 running 态维持原样。
  const statusColor = toolCall.status === 'running' ? 'text-secondary' : toolCall.status === 'error' ? 'text-error' : 'text-on-surface-variant'
  const filePath = extractFilePath(toolCall.tool_name, toolCall.tool_input)
  const canDiff = filePath != null && toolCall.status === 'completed' && !toolCall.is_error
  const durationMs = toolCall.duration_ms ?? durationMsProp
  // P1-⑤: the engine's §4.12 metadata — surface a sandbox-denied verdict on
  // the card itself instead of burying it in the expanded JSON.
  const sandboxDenied = (() => {
    const meta = toolCall.meta
    if (!meta || typeof meta !== 'object') return false
    return (meta as Record<string, unknown>).classification === 'sandbox_denied'
  })()

  return (
    <>
      <Tool name={toolCall.tool_name} status={toolCall.status} className="p-sm">
      <ToolHeader onClick={() => setExpanded(!expanded)}>
        <span className={cn('material-symbols-outlined icon-sm', statusColor, toolCall.status === 'running' ? 'animate-spin' : '')}>{statusIcon}</span>
        <span className="font-label-md text-on-surface flex-1 truncate">{toolCall.tool_name}</span>
        {sandboxDenied && (
          <span
            role="img"
            aria-label={t('chat.tool.sandboxDenied')}
            title={t('chat.tool.sandboxDenied')}
            className="flex items-center gap-[2px] shrink-0 px-xs py-[1px] rounded-sm bg-error-container text-on-error-container font-label-xs"
          >
            <span className="material-symbols-outlined icon-xs" aria-hidden="true">shield</span>
            {t('chat.tool.sandboxDenied')}
          </span>
        )}
        {toolCall.status !== 'running' && durationMs != null && (
          <span className="font-mono text-label-xs tabular-nums text-on-surface-variant/80 shrink-0" aria-hidden="true">
            {formatToolDuration(durationMs)}
          </span>
        )}
        {toolCall.status !== 'running' && (toolCall.tokens_used ?? 0) > 0 && (
          <span
            className="font-mono text-label-xs tabular-nums text-on-surface-variant/80 shrink-0"
            title={t('chat.tool.tokens.title')}
          >
            {toolCall.tokens_used!.toLocaleString()} tok
          </span>
        )}
        {/* Aurora redesign 2026-10: the header's inline "Diff" chip moved
            into the InlineDiffCard strip below the tool row (02-chat.html
            内联 diff 卡) — same action, same aria contract
            (chat.message.diff.aria), one row instead of a cramped chip. */}
        <span className="material-symbols-outlined icon-sm text-on-surface-variant" aria-hidden="true">{expanded ? 'expand_less' : 'expand_more'}</span>
      </ToolHeader>
      {expanded && (
        <ToolContent>
          {toolCall.tool_input ? (
            <ToolInputSummary input={toolCall.tool_input} />
          ) : null}
          {toolCall.result && (
            toolCall.is_error ? (
              <pre className="text-body-sm p-sm rounded-lg overflow-x-auto max-h-[200px] bg-error-container text-on-error-container">{toolCall.result}</pre>
            ) : (
              <div className="text-body-sm p-sm rounded-lg overflow-x-auto max-h-[200px] bg-surface-container text-on-surface-variant prose prose-sm max-w-none prose-pre:bg-surface-container-lowest prose-pre:p-sm prose-pre:rounded-sm prose-code:text-primary prose-code:before:content-[''] prose-code:after:content-['']">
                <Markdown>{toolCall.result}</Markdown>
              </div>
            )
          )}
        </ToolContent>
      )}
      </Tool>
      {/* Aurora redesign 2026-10 (02-chat.html 内联 diff 卡): a COMPLETED
          file-mutating call with a path input renders the compact diff card
          at the tool card's position — mono path + "+N −N" (only when the
          getFileDiff computation actually yields counts) + 「打开 Diff」,
          which reuses this bubble's onViewDiff (Chat.tsx setDiffPath →
          RightDock diff tab). The card carries the aria label the old
          header Diff chip had (chat.message.diff.aria), so e2e/AT anchors
          survive the move. */}
      {canDiff && (
        <InlineDiffCard path={filePath!} onOpenDiff={() => onViewDiff(filePath!)} />
      )}
      {/* office Wave 1 (A5): the artifact FileCard stays directly under the
          diff card — the OS actions (open / reveal / save as / preview /
          batch run) and the Files-page index registration live there. No
          heuristic path scraping of results; failed / running writes never
          render either card. */}
      {canDiff && (
        <FileCard
          name={basenameOf(filePath!)}
          path={filePath!}
          source="generated"
          // B7' — engine-written file: the "Review changes" button docks the
          // single-file diff in the RightDock (same diffPath path the
          // InlineDiffCard's action drives).
          onReviewDiff={() => onViewDiff(filePath!)}
        />
      )}
    </>
  )
})

/**
 * Render a tool call's input in a human-readable summary instead of dumping
 * the full JSON. Picks the most meaningful field per tool (bash → command,
 * file write → path + content preview), keeps short inputs as a single line
 * for visual rhythm, and offers a "查看详情" toggle for longer payloads.
 *
 * Falls back to compact JSON for unknown tools.
 */
function ToolInputSummary({ input }: { input: unknown }) {
  const obj = (input ?? {}) as Record<string, unknown>
  const command = pickString(obj, ['command', 'cmd', 'shell_command'])
  const filePath = pickString(obj, ['path', 'file_path', 'filepath', 'notebook_path'])
  const content = pickString(obj, ['content', 'text', 'source', 'body'])
  const query = pickString(obj, ['query', 'pattern', 'q'])

  // bash / shell: command is the story
  if (command != null) {
    return <ToolInputPrimary label="command" body={command} />
  }
  // file write: path chip (P0-B — clickable when the file exists) + preview
  if (filePath != null || content != null) {
    if (filePath != null) {
      return (
        <div className="text-body-sm bg-surface-container px-sm py-xs rounded-lg max-h-[200px] overflow-x-auto flex flex-wrap items-baseline gap-x-xs gap-y-1">
          <span className="font-mono text-on-surface-variant">file:</span>
          <FileRefChip raw={filePath} />
          {content != null && <span className="font-mono text-on-surface-variant">— {summarize(content)}</span>}
        </div>
      )
    }
    return <ToolInputPrimary label="content" body={summarize(content!)} />
  }
  // search: just the query
  if (query != null) {
    return <ToolInputPrimary label="query" body={query} />
  }

  // Unknown tool — fall back to compact JSON
  return (
    <pre className="text-body-sm text-on-surface-variant bg-surface-container p-sm rounded-lg overflow-x-auto max-h-[200px]">
      {JSON.stringify(obj, null, 2)}
    </pre>
  )
}

function pickString(obj: Record<string, unknown>, keys: string[]): string | null {
  for (const key of keys) {
    const v = obj[key]
    if (typeof v === 'string' && v.length > 0) return v
  }
  return null
}

/** First line + char count when truncated, so a 4 KB file write stays one row. */
function summarize(s: string): string {
  const newline = s.indexOf('\n')
  const firstLine = newline >= 0 ? s.slice(0, newline) : s
  const extraLines = s.split('\n').length - 1
  const extraChars = s.length - firstLine.length
  if (newline < 0 && extraChars === 0) return firstLine
  const head = firstLine.length > 120 ? `${firstLine.slice(0, 120)}…` : firstLine
  return extraLines > 0
    ? `${head}  (+${extraLines} ${extraLines === 1 ? 'line' : 'lines'})`
    : `${head}  (${extraChars} chars)`
}

const SHORT_LINE_LIMIT = 80
function ToolInputPrimary({ label, body }: { label: string; body: string }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [showAll, setShowAll] = useState(false)
  const isShort = body.length <= SHORT_LINE_LIMIT && !body.includes('\n')
  if (isShort) {
    return (
      <div className="text-body-sm bg-surface-container px-sm py-xs rounded-lg max-h-[200px] overflow-x-auto">
        <span className="font-mono text-on-surface-variant mr-xs">{label}:</span>
        <span className="font-mono text-on-surface">{body}</span>
      </div>
    )
  }
  return (
    <div className="bg-surface-container rounded-lg overflow-hidden">
      <pre className="text-body-sm text-on-surface px-sm py-xs overflow-x-auto max-h-[200px] whitespace-pre-wrap break-words font-mono">{showAll ? body : firstLines(body, 3)}</pre>
      <div className="flex items-center gap-sm px-sm py-xs border-t border-outline-variant/15">
        <button
          type="button"
          className="font-label-xs text-primary hover:underline focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary/40 rounded-sm px-xs -mx-xs"
          onClick={() => setShowAll(v => !v)}
        >
          {showAll ? intl.formatMessage({ id: 'chat.tool.input.collapse' }) : intl.formatMessage({ id: 'chat.tool.input.expand' })}
        </button>
        <span className="font-label-xs text-on-surface-variant/70">{intl.formatMessage({ id: 'chat.tool.input.length' }, { chars: body.length })}</span>
      </div>
      <span className="sr-only">{t('chat.tool.input.srHint')}</span>
    </div>
  )
}

function firstLines(s: string, n: number): string {
  const lines = s.split('\n')
  if (lines.length <= n) return s
  return `${lines.slice(0, n).join('\n')}\n…`
}

/**
 * P1-⑥ (ZCode delta): first-class collapsible block for `agent_spawn` tool
 * calls — sub-agent runs render as their own timeline section (name, model,
 * team, spawn prompt + result summary) instead of a generic tool card.
 * Engine note: with agent teams enabled (B2), the registry bridges spawn
 * lifecycle to `subagent:start` / `subagent:stop` — while the spawn tool is
 * running, the block surfaces the live registry agent id via `subagentLive`.
 */
export const SubagentBlock = memo(function SubagentBlock({ toolCall }: { toolCall: ToolCall }) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) => intl.formatMessage({ id }, values)
  const { subagentLive } = useSessions()
  const { config } = useCatalog()
  const [expanded, setExpanded] = useState(false)
  const input = (toolCall.tool_input ?? {}) as Record<string, unknown>
  const name = typeof input.name === 'string' ? input.name : ''
  const model = typeof input.model === 'string' ? input.model : null
  const team = typeof input.team === 'string' ? input.team : null
  const maxTurns = typeof input.max_turns === 'number' ? input.max_turns : null
  const systemPrompt = typeof input.system_prompt === 'string' ? input.system_prompt : ''
  const statusIcon = toolCall.status === 'running' ? 'hourglass_empty' : toolCall.status === 'error' ? 'error' : 'check_circle'
  // R2 审查 W10: subagent 卡完成对勾同样去琥珀 → 中性 on-surface-variant。
  const statusColor = toolCall.status === 'running' ? 'text-secondary' : toolCall.status === 'error' ? 'text-error' : 'text-on-surface-variant'
  const durationMs = toolCall.duration_ms

  return (
    <div
      className="rounded-xl border border-primary/20 bg-primary-container overflow-hidden"
      data-testid="subagent-block"
    >
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        aria-expanded={expanded}
        className="w-full flex items-center gap-sm px-sm py-xs text-left cursor-pointer hover:bg-primary/10 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
      >
        <span className="material-symbols-outlined icon-sm text-primary shrink-0" aria-hidden="true">account_tree</span>
        <span className="font-label-md text-on-surface flex-1 truncate">
          {t('chat.subagent.title', { name: name || t('chat.subagent.unnamed') })}
        </span>
        {model && (
          <span className="font-mono text-label-xs text-on-surface-variant px-xs py-[1px] rounded-sm bg-surface-container shrink-0" aria-hidden="true">{model}</span>
        )}
        {team && (
          <span className="font-label-xs text-on-surface-variant px-xs py-[1px] rounded-sm bg-surface-container shrink-0" aria-hidden="true">{team}</span>
        )}
        {toolCall.status === 'running' && subagentLive && (
          <span className="font-mono text-label-xs px-xs py-[1px] rounded-sm bg-primary-container text-on-primary-container shrink-0 flex items-center gap-xs" aria-live="polite">
            <span className="size-1.5 rounded-full bg-primary animate-pulse" aria-hidden="true" />
            {t('chat.subagent.registryId', { id: subagentLive.agentId })}
          </span>
        )}
        {toolCall.status !== 'running' && durationMs != null && (
          <span className="font-mono text-label-xs tabular-nums text-on-surface-variant/80 shrink-0" aria-hidden="true">
            {formatToolDuration(durationMs)}
          </span>
        )}
        <span className={cn('material-symbols-outlined icon-sm shrink-0', statusColor, toolCall.status === 'running' ? 'animate-spin' : '')} aria-hidden="true">{statusIcon}</span>
        <span className="material-symbols-outlined icon-sm text-on-surface-variant" aria-hidden="true">{expanded ? 'expand_less' : 'expand_more'}</span>
      </button>
      {expanded && (
        <div className="px-sm pb-sm space-y-sm">
          {maxTurns != null && (
            <p className="font-label-sm text-on-surface-variant">{t('chat.subagent.maxTurns', { count: maxTurns })}</p>
          )}
          {/* B2 follow-up — surface the parent's approval policy so the
              user can see what sandbox/permissions the sub-agent runs under. */}
          {config?.approval_mode && (
            <p className="font-label-sm text-on-surface-variant" data-testid="subagent-inherit-mode">
              {t('chat.subagent.inheritsMode', { mode: config.approval_mode })}
            </p>
          )}
          {systemPrompt && (
            <pre className="text-body-sm text-on-surface-variant bg-surface-container p-sm rounded-lg overflow-x-auto max-h-[160px] whitespace-pre-wrap">{systemPrompt}</pre>
          )}
          {toolCall.result && (
            <pre className={cn(
              'text-body-sm p-sm rounded-lg overflow-x-auto max-h-[200px]',
              toolCall.is_error ? 'bg-error-container text-on-error-container' : 'bg-surface-container text-on-surface-variant',
            )}>{toolCall.result}</pre>
          )}
        </div>
      )}
    </div>
  )
})

/** P2-1 (§4 P2): render user-message text with bare URLs clickable. Split
 *  is purely textual — no markdown parsing, so the user's own formatting
 *  (line breaks, asterisks, underscores) survives untouched. */
const URL_RE = /https?:\/\/[^\s<>"')\]]+/g

export function LinkifiedText({ text }: { text: string }) {
  const parts: (string | { url: string })[] = []
  let last = 0
  for (const match of text.matchAll(URL_RE)) {
    const idx = match.index ?? 0
    // Trailing sentence punctuation reads as part of the URL otherwise
    // ("see https://a.com." would open with the dot).
    const url = match[0].replace(/[.,;:!?)\]}'"]+$/, '')
    const urlEnd = idx + url.length
    if (idx > last) parts.push(text.slice(last, idx))
    parts.push({ url })
    last = urlEnd
  }
  if (last < text.length) parts.push(text.slice(last))
  if (parts.length === 1 && typeof parts[0] === 'string') {
    return <>{parts[0]}</>
  }
  return (
    <>
      {parts.map((part, i) =>
        typeof part === 'string' ? (
          part
        ) : (
          <a
            key={i}
            href={part.url}
            target="_blank"
            rel="noopener noreferrer"
            // Opening is handled by the global link interceptor (P0-A) —
            // an onClick here would fire a second openLink per click.
            className="underline underline-offset-2 decoration-current/50 hover:decoration-current break-all cursor-pointer"
          >
            {part.url}
          </a>
        ),
      )}
    </>
  )
}
