import { useT } from '@/i18n'
import { useNavigate } from 'react-router-dom'
import { Banner } from '@/components/ui/banner'
import type { RefObject } from 'react'
import { useEffect, useMemo, useRef, useState, useCallback } from 'react'
import type { Virtualizer } from '@tanstack/react-virtual'
import { Button } from '@/components/ui/button'
import WelcomeState from '@/components/WelcomeState'
import { MessageBubble, type RegeneratePayload } from '@/components/chat/MessageBubble'
import StreamingResponse from '@/components/chat/StreamingResponse'
import { useChat } from '@/context/ChatContext'
import type { StreamNotice } from '@/context/ChatContext'
import { useCatalog } from '@/context/CatalogContext'
import { useSessions } from '@/context/SessionContext'
import { useComposer } from './ComposerContext'
import * as api from '@/lib/tauri-api'
// Virtualization only kicks in past the threshold. Below it, the overhead
// of measuring/positioning outweighs the win from fewer DOM nodes — and
// jsdom can't provide real dimensions, so tests would render zero items.
// (Exported: Chat's search bar needs the same threshold to pick the jump
// mechanism for a match.)
export const VIRTUALIZE_THRESHOLD = 30

/** djb2 hex — short stable salt for React keys of messages that carry no id. */
function hashContent(s: string): string {
  let h = 5381
  for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) | 0
  return (h >>> 0).toString(36)
}

/**
 * P2-13 (§4-15): virtualizer keys must be stable identity, not index-bound.
 * ChatMessage carries no id, so the most stable available fingerprint is
 * role + timestamp + content hash, deduplicated in list order for the rare
 * identical twins.
 */
function useStableMessageKeys(messages: { role: string; content: string; timestamp: number }[]): string[] {
  return useMemo(() => {
    const seen = new Map<string, number>()
    return messages.map((m) => {
      const base = `${m.role}-${m.timestamp}-${hashContent(m.content)}`
      const n = seen.get(base) ?? 0
      seen.set(base, n + 1)
      return n === 0 ? base : `${base}#${n}`
    })
  }, [messages])
}

/**
 * S1-1 (review 2026-10-05 §3 P-N1): best-effort extraction of a Retry-After
 * delay from the raw provider error text (429 bodies sometimes carry
 * "Retry-After: N" / "try again in Ns"). DISPLAY HINT ONLY — it feeds the
 * rate-limit banner's "{seconds}s" line and is never a classification
 * signal; classification stays engine-side (error_kind).
 * Exported for direct unit testing.
 */
export function parseRetryAfterSeconds(error: string): number | null {
  const patterns = [
    /retry[-\s]?after\D{0,16}(\d{1,5})/i,
    /try\s+again\s+in\s+(\d{1,5})\s*(?:s\b|sec|second)/i,
  ]
  for (const re of patterns) {
    const m = error.match(re)
    if (m) {
      const seconds = Number(m[1])
      if (Number.isFinite(seconds) && seconds > 0) return seconds
    }
  }
  return null
}

/**
 * P2-17 (§4-17): the single aria-live region for run state transitions.
 * The streaming log itself used to be a polite live region (screen-reader
 * token spam) — now only the transitions announce: "generating" when a run
 * starts, "reply complete" when it ends. Exported for direct testability.
 */
export function StreamStatusRegion({ active }: { active: boolean }) {
  const t = useT()
  const [message, setMessage] = useState('')
  const wasActiveRef = useRef(false)
  useEffect(() => {
    if (active) {
      wasActiveRef.current = true
      setMessage(t('chat.stream.status.active'))
    } else if (wasActiveRef.current) {
      wasActiveRef.current = false
      setMessage(t('chat.stream.status.done'))
    }
    // Transitions strictly alternate, so consecutive writes always change
    // the text — a polite live region announces each one; no clear/rewrite
    // dance (and no timer) needed.
  }, [active, t])
  return (
    <div role="status" aria-live="polite" className="sr-only" data-testid="stream-status-region">
      {message}
    </div>
  )
}

/**
 * R5-2: one in-stream retry notice — failover (R3-1) or key rotation
 * (R4-3) — rendered as a subtle system-style line, NOT an error banner:
 * the engine continued on the new target, so the run never failed. Muted,
 * small, distinct icon per kind (`alt_route` vs `vpn_key`); the verbatim
 * engine line rides as the secondary detail (it is replayable from the L0
 * log, so showing it raw keeps the UI honest without re-parsing it).
 * Exported for direct testability.
 */
export function StreamNoticeLine({ notice }: { notice: StreamNotice }) {
  const t = useT()
  const failover = notice.kind === 'failover'
  const label = failover ? t('chat.notice.failover') : t('chat.notice.keyRotation')
  return (
    <div
      data-testid={`stream-notice-${notice.kind}`}
      className="flex items-center gap-xs px-md py-xs rounded-lg border border-outline-variant/20 bg-surface-container-lowest/60 max-w-[90%]"
    >
      <span
        className={`material-symbols-outlined icon-md shrink-0 ${failover ? 'text-secondary' : 'text-tertiary'}`}
        aria-hidden="true"
      >
        {failover ? 'alt_route' : 'vpn_key'}
      </span>
      <span className="font-label-sm text-on-surface-variant shrink-0">{label}</span>
      <span
        className="font-label-sm text-on-surface-variant/70 truncate min-w-0"
        title={notice.message}
      >
        {notice.message}
      </span>
    </div>
  )
}

/**
 * P1-⑤ telemetry: tool_use_id → duration (ms) from the session's L0 trace
 * timeline — one IPC per session, giving every historical tool card its
 * authoritative duration (live cards measure client-side instead).
 * Best-effort: failures just leave the cards without a duration label.
 * A-8 fix: the timeline is also re-read when the session's run SETTLES
 * (isQuerying true→false) — the just-finished turn's authoritative durations
 * only exist backend-side after settle, and the user can keep reading this
 * session without ever switching away (the lookup used to refresh on session
 * switches only). A run START bumps nothing: it adds no history to read.
 */
function useToolDurationLookup(sessionId: string | null, isQuerying: boolean): Map<string, number> {
  const [lookup, setLookup] = useState<Map<string, number>>(() => new Map())
  // One tick per settle transition (not per render) feeds the fetch effect.
  const [settleTick, setSettleTick] = useState(0)
  const wasQueryingRef = useRef(isQuerying)
  useEffect(() => {
    if (wasQueryingRef.current && !isQuerying) setSettleTick(t => t + 1)
    wasQueryingRef.current = isQuerying
  }, [isQuerying])
  useEffect(() => {
    if (!sessionId) {
      setLookup(new Map())
      return
    }
    let cancelled = false
    api.getTraceTimeline(sessionId)
      .then(tl => {
        if (cancelled) return
        const map = new Map<string, number>()
        for (const turn of tl.turns) {
          for (const tool of turn.tools) {
            if (tool.duration_ms != null) map.set(tool.tool_use_id, tool.duration_ms)
          }
        }
        setLookup(map)
      })
      .catch(() => { /* durations are opportunistic */ })
    return () => { cancelled = true }
  }, [sessionId, settleTick])
  return lookup
}

interface MessageAreaProps {
  scrollParentRef: RefObject<HTMLDivElement | null>
  messagesEndRef: RefObject<HTMLDivElement | null>
  virtualizer: Virtualizer<HTMLDivElement, Element>
  setDiffPath: (p: string | null) => void
  setDiffPaths: (p: string[] | null) => void
  /** B1 §4-12: the message a search jump landed on (transient ring). */
  searchFlashIndex?: number | null
  /** B1 §4-8: begin a composer-based edit of the user message at `index`. */
  onEditMessage?: (index: number) => void
}

/** B1 §4-7: everything the LAST assistant message needs for a true
 *  regenerate — rewind to the checkpoint before its preceding user turn and
 *  re-send that turn's text + attachment paths. Null when there is no
 *  preceding user turn or no checkpoint covers it (fresh/demo sessions):
 *  then the button simply does not render, like the rewind affordance.
 *  (Shape: RegeneratePayload, declared in MessageBubble.) */
function regenerateInfoFor(messages: { role: string; content: string; file_attachments?: { path: string }[] }[], lastAssistantIndex: number, checkpointTurns: number[]): RegeneratePayload | null {
  let userIdx = -1
  for (let i = lastAssistantIndex - 1; i >= 0; i--) {
    if (messages[i]?.role === 'user') {
      userIdx = i
      break
    }
  }
  if (userIdx < 0) return null
  const { turnIndex, rewindable } = rewindInfoFor(messages, userIdx, checkpointTurns)
  if (!rewindable) return null
  const userMessage = messages[userIdx]
  return {
    turnIndex,
    content: userMessage.content,
    attachmentPaths: (userMessage.file_attachments ?? []).map(a => a.path),
  }
}

/** True when the user is scrolled away from the very bottom by at least
 *  the threshold below. The chat page surfaces a "scroll to latest" FAB
 *  whenever this is the case — and also when the active run starts so the
 *  user can return to live output after reading an older turn. */
const SCROLL_FROM_BOTTOM_THRESHOLD_PX = 200

// Virtualized + non-virtualized message list. Below the threshold (30
// messages) we render everything in a flat log so jsdom tests still see the
// bubbles — virtualization's measureElement needs a real DOM with height.
// Per-message rewind affordance: a user message at chat index `msgIndex`
// owns conversation turn `count(user messages before it)`. It is rewindable
// when a recorded checkpoint sits at or after that turn — no checkpoints
// (e.g. fresh session, demo mode) means nothing to undo and no button.
function rewindInfoFor(messages: { role: string }[], msgIndex: number, checkpointTurns: number[]): { turnIndex: number; rewindable: boolean } {
  let turnIndex = 0
  for (let i = 0; i < msgIndex; i++) {
    if (messages[i]?.role === 'user') turnIndex++
  }
  const rewindable = checkpointTurns.some(t => t >= turnIndex)
  return { turnIndex, rewindable }
}

export default function MessageArea({
  scrollParentRef,
  messagesEndRef,
  virtualizer,
  setDiffPath,
  setDiffPaths,
  searchFlashIndex,
  onEditMessage,
}: MessageAreaProps) {
  const { messages, streamingText, thinkingText, activeToolCalls, toolProgress, streamNotices, checkpoints, rewindSession, isQuerying } = useChat()
  const { currentSessionId, sessionActivity, switchingSession } = useSessions()
  const durationLookup = useToolDurationLookup(currentSessionId, isQuerying)
  const checkpointTurns = useMemo(() => checkpoints.map(c => c.turn_index), [checkpoints])
  const rewind = useMemo(() => {
    return (msgIndex: number) => {
      const { turnIndex, rewindable } = rewindInfoFor(messages, msgIndex, checkpointTurns)
      return rewindable ? turnIndex : null
    }
  }, [messages, checkpointTurns])
  // B1 §4-7: true regenerate — only the LAST assistant message carries the
  // button, and only when the rewind+resend pipeline can actually run.
  const lastAssistantIndex = useMemo(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i]?.role === 'assistant') return i
    }
    return -1
  }, [messages])
  const regenerateInfo = useMemo(
    () => (lastAssistantIndex >= 0 ? regenerateInfoFor(messages, lastAssistantIndex, checkpointTurns) : null),
    [messages, lastAssistantIndex, checkpointTurns],
  )
  const { error, errorKind, providerStatus } = useCatalog()
  const navigate = useNavigate()
  const t = useT()
  // S1-1: rate-limit banner shows "retry in Ns" when the provider error
  // text carries a parseable delay (see parseRetryAfterSeconds).
  const retryAfterSeconds = useMemo(
    () => (errorKind === 'rate_limit' && error ? parseRetryAfterSeconds(error) : null),
    [error, errorKind],
  )
  const shouldVirtualize = messages.length > VIRTUALIZE_THRESHOLD
  const messageKeys = useStableMessageKeys(messages)
  // P2-17: one run-level status region for the whole flow — the list and
  // the streaming log no longer announce every content change themselves.
  const streamActive = isQuerying || !!streamingText || !!thinkingText || activeToolCalls.length > 0

  // X-2: surface a "scroll to latest" FAB whenever the user is scrolled
  // away from the bottom. Cheap: one passive scroll listener, no re-render
  // unless the boolean actually flips.
  // P2-6: the subscription itself is mount-stable — `update` hangs off the
  // stable ref so a growing `messages.length` only re-runs the distance
  // check (content growth can push the viewport off the bottom), it no
  // longer tears down and re-arms the listener once per message.
  const [showScrollFab, setShowScrollFab] = useState(false)
  const updateScrollFab = useCallback(() => {
    const el = scrollParentRef.current
    if (!el) return
    const dist = el.scrollHeight - el.clientHeight - el.scrollTop
    setShowScrollFab(dist > SCROLL_FROM_BOTTOM_THRESHOLD_PX)
  }, [scrollParentRef])
  useEffect(() => {
    const el = scrollParentRef.current
    if (!el) return
    updateScrollFab()
    el.addEventListener('scroll', updateScrollFab, { passive: true })
    return () => el.removeEventListener('scroll', updateScrollFab)
  }, [scrollParentRef, updateScrollFab])
  useEffect(() => {
    updateScrollFab()
  }, [messages.length, updateScrollFab])
  const scrollToBottom = useCallback(() => {
    const el = scrollParentRef.current
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' })
  }, [scrollParentRef])

  // B1 §4-7: shared bubble props — the regenerate payload rides only on the
  // last assistant message; the edit affordance rides on every user message.
  const bubbleProps = (index: number) => ({
    onViewDiff: setDiffPath,
    onViewDiffMulti: setDiffPaths,
    rewindTurnIndex: rewind(index),
    onRewind: rewindSession,
    durationLookup,
    onEditMessage,
    searchFlash: searchFlashIndex === index,
    regenerate: index === lastAssistantIndex ? regenerateInfo : undefined,
  })

  // Stable ref identity: an inline arrow here would be a NEW function every
  // render, so React would detach (null) + re-attach (el) each row on every
  // render — one wasted idempotent re-measure per row per render. With a
  // stable callback a row measures once on mount (the virtualizer instance
  // from useVirtualizer is useState-held, so the identity only ever tracks
  // that); resizes keep riding the ResizeObserver.
  const measureRow = useCallback(
    (el: HTMLElement | null) => {
      // Defer the measurement off the ref (commit) phase:
      // react-virtual's measureElement notifies with a flushSync
      // rerender, and React refuses a flush while the commit is
      // still in progress — every jump that mounts rows (e.g. a
      // Ctrl+F search landing) console.error'd. A microtask is
      // still ahead of first paint; resizes keep riding the
      // ResizeObserver as before. (el === null on unmount hits
      // measureElement's disconnected-cache GC branch.)
      queueMicrotask(() => virtualizer.measureElement(el))
    },
    [virtualizer],
  )

  return (
    // chat-scroll-container: stable testid for e2e scroll helpers — the
    // virtualized list only mounts the viewport window, so a test that
    // needs a specific row must scroll THIS container to it first.
    <div
      ref={scrollParentRef}
      data-testid="chat-scroll-container"
      className="relative flex-1 overflow-y-auto px-xl pt-lg pb-md"
    >
      <StreamStatusRegion active={streamActive} />
      {messages.length === 0 && !streamingText && <ComposerWelcome />}

      {messages.length > 0 && shouldVirtualize && (
        <div
          style={{ height: `${virtualizer.getTotalSize()}px`, position: 'relative' }}
          aria-label={t('chat.history.aria')}
        >
          {virtualizer.getVirtualItems().map(vItem => {
            const msg = messages[vItem.index]
            return (
              <div
                key={messageKeys[vItem.index]}
                data-index={vItem.index}
                data-message-index={vItem.index}
                ref={measureRow}
                className="pb-lg"
                style={{ position: 'absolute', top: 0, left: 0, width: '100%', transform: `translateY(${vItem.start}px)` }}
              >
                <MessageBubble message={msg} messageIndex={vItem.index} {...bubbleProps(vItem.index)} />
              </div>
            )
          })}
        </div>
      )}

      {messages.length > 0 && !shouldVirtualize && (
        <div aria-label={t('chat.history.aria')}>
          {messages.map((msg, i) => (
            <div key={messageKeys[i]} data-message-index={i} className="pb-lg">
              <MessageBubble message={msg} messageIndex={i} {...bubbleProps(i)} />
            </div>
          ))}
        </div>
      )}

      {/* R5-2: in-stream retry notices (failover / key rotation) — subtle
          system lines for recoveries the engine already handled. They are
          NOT errors (the request continued), so they render muted above the
          streaming reply and survive the run's completion until the session's
          next send, letting the user see how the last answer was served. */}
      {streamNotices.length > 0 && (
        <div data-testid="stream-notices" className="space-y-xs pt-lg">
          {streamNotices.map(n => (
            <StreamNoticeLine key={n.id} notice={n} />
          ))}
        </div>
      )}

      {/* Streaming response */}
      {(streamingText || thinkingText || activeToolCalls.length > 0) && (
        <StreamingResponse
          streamingText={streamingText}
          thinkingText={thinkingText}
          activeToolCalls={activeToolCalls}
          onViewDiff={setDiffPath}
        />
      )}

      {/* Batch C3 (ZCode「已工作 3 分 34 秒」): wall-clock status pill pinned
          to the flow bottom while a run is live — time awareness without
          expanding tool cards. */}
      {isQuerying && <RunStatusLine startedAt={currentSessionId ? sessionActivity[currentSessionId]?.startedAt ?? null : null} activeTool={currentSessionId ? sessionActivity[currentSessionId]?.activeTool ?? null : null} toolProgress={toolProgress} />}

      {/* Review §2-3 + S1-1 (P-N1): failures classified Rust-side on the
          QUERY_FAILED payload (engine's typed error → error_kind) get
          dedicated recovery banners:
            auth (401)       → provider key banner (unchanged behavior);
            quota (402)      → "quota exhausted" + update key / view usage /
                               switch-model hint;
            rate_limit (429) → wait hint (+ "retry in Ns" when the provider
                               text carries a delay) + Retry;
            authz (403)      → "access denied" + Settings pointer.
          All other failures keep the raw error line + Retry. */}
      {error && errorKind === 'auth' ? (
        <Banner
          variant="card"
          tone="error"
          className="mx-auto max-w-md text-error font-label-md"
          data-testid="auth-error-banner"
        >
          <span className="material-symbols-outlined icon-md text-error">key_alert</span>
          <span className="flex-1 text-center">
            {t('chat.error.auth.title', {
              provider: providerStatus?.display_name
                ?? providerStatus?.active_provider_id
                ?? t('chat.error.auth.fallbackProvider'),
            })}
            <span className="block font-body-sm text-on-surface-variant mt-xs">
              {t('chat.error.auth.body')}
            </span>
          </span>
          <Button
            type="button"
            variant="ghost"
            className="mt-sm text-error hover:bg-error/10 text-label-md cursor-pointer"
            onClick={() => navigate('/settings/models')}
          >
            {t('chat.error.auth.updateKey')}
          </Button>
          <ComposerRetryButton />
        </Banner>
      ) : error && errorKind === 'quota' ? (
        <Banner
          variant="card"
          tone="error"
          className="mx-auto max-w-md text-error font-label-md"
          data-testid="quota-error-banner"
        >
          <span className="material-symbols-outlined icon-md text-error">payments</span>
          <span className="flex-1 text-center">
            {t('chat.error.quota.title')}
            <span className="block font-body-sm text-on-surface-variant mt-xs">
              {t('chat.error.quota.body')}
            </span>
          </span>
          <div className="mt-sm flex items-center justify-center gap-xs">
            <Button
              type="button"
              variant="ghost"
              className="text-error hover:bg-error/10 text-label-md cursor-pointer"
              onClick={() => navigate('/settings/models')}
            >
              {t('chat.error.quota.updateKey')}
            </Button>
            <Button
              type="button"
              variant="ghost"
              className="text-error hover:bg-error/10 text-label-md cursor-pointer"
              onClick={() => navigate('/usage')}
            >
              {t('chat.error.quota.viewUsage')}
            </Button>
          </div>
          <ComposerRetryButton />
        </Banner>
      ) : error && errorKind === 'rate_limit' ? (
        <Banner
          variant="card"
          tone="error"
          className="mx-auto max-w-md text-error font-label-md"
          data-testid="rate-limit-error-banner"
        >
          <span className="material-symbols-outlined icon-md text-error">hourglass_top</span>
          <span className="flex-1 text-center">
            {t('chat.error.rateLimit.title')}
            <span className="block font-body-sm text-on-surface-variant mt-xs">
              {t('chat.error.rateLimit.body')}
            </span>
            {retryAfterSeconds !== null && (
              <span className="block font-body-sm text-on-surface-variant mt-xs">
                {t('chat.error.rateLimit.retryAfter', { seconds: retryAfterSeconds })}
              </span>
            )}
          </span>
          <ComposerRetryButton />
        </Banner>
      ) : error && errorKind === 'authz' ? (
        <Banner
          variant="card"
          tone="error"
          className="mx-auto max-w-md text-error font-label-md"
          data-testid="authz-error-banner"
        >
          <span className="material-symbols-outlined icon-md text-error">lock</span>
          <span className="flex-1 text-center">
            {t('chat.error.authz.title')}
            <span className="block font-body-sm text-on-surface-variant mt-xs">
              {t('chat.error.authz.body')}
            </span>
          </span>
          <Button
            type="button"
            variant="ghost"
            className="mt-sm text-error hover:bg-error/10 text-label-md cursor-pointer"
            onClick={() => navigate('/settings/models')}
          >
            {t('chat.error.authz.checkKey')}
          </Button>
          <ComposerRetryButton />
        </Banner>
      ) : error ? (
        <Banner
          variant="card"
          tone="error"
          className="mx-auto max-w-md text-error font-label-md"
        >
          <span className="material-symbols-outlined icon-md text-error">error</span>
          <span className="flex-1 text-center">{error}</span>
          <ComposerRetryButton />
        </Banner>
      ) : null}

      <div ref={messagesEndRef} />

      {showScrollFab && messages.length > 0 && (
        <Button
          type="button"
          variant="outline"
          aria-label={t('chat.scrollToLatest.aria')}
          title={t('chat.scrollToLatest.aria')}
          onClick={scrollToBottom}
          // G1: solid — a transient affordance doesn't earn one of the four
          // per-screen backdrop-filter slots.
          className="sticky bottom-md left-full -translate-x-full ml-sm w-10 h-10 rounded-full bg-surface-container-lowest border border-outline-variant/30 shadow-e3 hover:bg-primary-container hover:border-primary/40 text-on-surface hover:text-primary transition-all flex items-center justify-center"
        >
          <span className="material-symbols-outlined icon-md" aria-hidden="true">arrow_downward</span>
        </Button>
      )}

      {/* B1 P2-3: session-swap skeleton — shown only while a switch IPC is in
          flight (AppContext never sets the flag for same-session remounts),
          so opening a session reads as instant-and-loading instead of stale. */}
      {/* G1: veil (遮罩) over the message list while switching — scrim-class,
          intentional direct backdrop-blur, exempt from the material rule. */}
      {switchingSession && (
        <div
          data-testid="session-switch-overlay"
          aria-busy="true"
          role="status"
          className="absolute inset-0 z-raised flex items-center justify-center bg-surface-container-lowest/60 backdrop-blur-[2px]"
        >
          <div className="flex flex-col items-center gap-sm">
            <span className="material-symbols-outlined text-primary animate-spin">progress_activity</span>
            <span className="font-label-sm text-on-surface-variant">{t('chat.session.switching')}</span>
          </div>
        </div>
      )}
    </div>
  )
}

// Leaf consumers of the composer context — keeping them out of MessageArea's
// render means the message list does not re-render on every keystroke.
function ComposerWelcome() {
  const { setInput } = useComposer()
  return <WelcomeState onSelectPrompt={setInput} />
}

/**
 * Batch C3: sticky status pill for a live run —「已工作 3分34秒 · 正在 bash」.
 * The startedAt/activeTool pair comes from the same SessionActivity the
 * sidebar rail consumes; a 1s tick drives the elapsed label while mounted.
 * P2-19: when the backend streams QUERY_TOOL_PROGRESS, the pill grows a
 * compact percentage chip (`· 45%`) next to the tool name and the backend's
 * progress_message, truncated with the full text in `title`. Both are raw
 * backend data (not UI chrome) — no new i18n keys; announcements ride the
 * existing role="status" region. Additive only: the pre-existing roles/
 * testids/aria structure is unchanged.
 */
export function RunStatusLine({ startedAt, activeTool, toolProgress }: { startedAt: number | null; activeTool: string | null; toolProgress?: { progress?: number; message?: string } | null }) {
  const t = useT()
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(id)
  }, [])
  const elapsed = startedAt != null ? formatWorked(Math.max(0, now - startedAt)) : null
  // Only a sane 0..=100 percentage renders — out-of-range/NaN payloads are
  // ignored instead of shown as garbage next to the tool name.
  const pct = toolProgress?.progress
  const pctText = typeof pct === 'number' && Number.isFinite(pct) && pct >= 0 && pct <= 100
    ? `${Math.round(pct)}%`
    : null
  const progressMsg = toolProgress?.message?.trim() ?? ''
  return (
    // G1: solid pill — same budget reasoning as the scroll FAB above.
    <div
      role="status"
      aria-live="polite"
      data-testid="run-status-line"
      className="sticky bottom-0 mt-md mx-auto w-fit max-w-full flex items-center gap-xs px-md py-xs rounded-full bg-surface-container-lowest border border-outline-variant/30 shadow-e1"
    >
      <span className="size-1.5 rounded-full bg-secondary animate-pulse shrink-0" aria-hidden="true" />
      <span className="font-label-sm text-on-surface-variant whitespace-nowrap">
        {elapsed != null && t('chat.status.worked', { time: elapsed })}
        {activeTool && t('chat.status.tool', { tool: activeTool })}
      </span>
      {pctText && (
        <span
          data-testid="run-progress-pct"
          className="font-label-sm text-on-surface-variant tabular-nums whitespace-nowrap shrink-0"
        >
          · {pctText}
        </span>
      )}
      {progressMsg && (
        <span
          data-testid="run-progress-message"
          title={progressMsg.slice(0, 200)}
          className="font-label-sm text-on-surface-variant truncate max-w-[16rem]"
        >
          {progressMsg}
        </span>
      )}
    </div>
  )
}

/** Compact running-clock label: 42s · 3m34s · 1h12m — keeps seconds so the
 *  pill visibly ticks (unlike the sidebar's coarser elapsed badge). */
function formatWorked(ms: number): string {
  const sec = Math.max(0, Math.floor(ms / 1000))
  if (sec < 60) return `${sec}s`
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min}m${String(sec % 60).padStart(2, '0')}s`
  return `${Math.floor(min / 60)}h${min % 60}m`
}

// B0 P1-3: the composer is cleared on send, so a retry gated on composer
// text was a guaranteed no-op. Retry now resends the LAST USER MESSAGE of
// the conversation — the same mechanism as the budget banner's "continue
// once". With no previous user message there is nothing to resend, so the
// button hides entirely.
function ComposerRetryButton() {
  const { messages, sendMessage } = useChat()
  const t = useT()
  const lastUser = useMemo(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i]?.role === 'user') return messages[i]
    }
    return null
  }, [messages])
  if (!lastUser) return null
  // A-3 fix: the retry resend carries the last user message's attachment
  // paths — previously it re-sent the text only and silently dropped them.
  const paths = (lastUser.file_attachments ?? []).map(a => a.path)
  return (
    <Button
      variant="ghost"
      className="mt-sm text-error hover:bg-error/10 text-label-md cursor-pointer"
      onClick={() => void sendMessage(lastUser.content, paths.length > 0 ? paths : undefined)}
    >
      {t('chat.error.retry')}
    </Button>
  )
}
