import { useState, useRef, useEffect, useCallback, lazy } from 'react'
import { useNavigate, useLocation } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { useVirtualizer } from '@tanstack/react-virtual'
import DiffDialogMulti from '@/components/diff/DiffDialogMulti'
import { useChat } from '@/context/ChatContext'
import { useCatalog } from '@/context/CatalogContext'
import { useSessions } from '@/context/SessionContext'
import { parseSlashInput, type SlashCommand, type SlashResult } from '@/lib/slash/commands'
import { recordInputHistory } from '@/lib/inputHistory'
import { clearDiffStatsCache } from '@/components/chat/diffStats'
import { toastError } from '@/lib/errorToast'
import { setActiveWorkingDir } from '@/lib/fileRefs'
import { useDiskArtifacts } from '@/hooks/useDiskArtifacts'
import { useArtifact } from '@/components/artifact/ArtifactContext'
import { useBudgetGuard } from '@/hooks/useBudgetGuard'
import { useSteerSend } from '@/hooks/useSteerSend'
import { toast } from 'sonner'
import BudgetBanner from '@/components/chat/BudgetBanner'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { TerminalPanel } from '@/components/terminal/TerminalPanel'
import RightDock from './chat/RightDock'
import {
  ApiKeyBanner,
  ChatSearchBar,
  ComposerPanel,
  InlinePanelModal,
  MessageArea,
  ComposerContext,
  // `/index` is load-bearing: bare `./chat` resolves case-insensitively to
  // this file (Chat.tsx) on Windows — a self-import that leaves every
  // barrel binding undefined. Only observable on case-insensitive
  // filesystems, i.e. never on the Linux CI that runs the vitest suite.
} from './chat/index'
import { VIRTUALIZE_THRESHOLD } from './chat/MessageArea'
import type { EditingMessageState } from './chat/ComposerContext'
import type { ProviderStatus } from '@/types'

// QuickFix is a chat-inline tool launched from the composer toolbar (it has
// no standalone route); the Editor exists both inline and as a standalone
// route (palette / mod+5). Lazy-loaded so the main chat bundle stays small.
const QuickFixPanel = lazy(() => import('@/pages/QuickFix'))
const EditorPanel = lazy(() => import('@/pages/EditorPage'))

// ── B1 §4-11 per-session draft storage ───────────────────────────────────
const DRAFT_KEY_PREFIX = 'shannon.draft.'
const DRAFT_DEBOUNCE_MS = 300
const DRAFT_MAX_BYTES = 64 * 1024
// B1 §4-12: how long the search-jump bubble keeps its ring (ms).
const SEARCH_FLASH_MS = 1200

function draftKey(sessionId: string): string {
  return `${DRAFT_KEY_PREFIX}${sessionId}`
}

function readDraft(sessionId: string): { text: string; attachments: string[] } | null {
  try {
    const raw = localStorage.getItem(draftKey(sessionId))
    if (!raw) return null
    const parsed = JSON.parse(raw) as { text?: unknown; attachments?: unknown }
    if (typeof parsed.text !== 'string' || !Array.isArray(parsed.attachments)) return null
    return {
      text: parsed.text,
      attachments: parsed.attachments.filter((a): a is string => typeof a === 'string'),
    }
  } catch { return null }
}

function writeDraft(sessionId: string, text: string, attachments: string[]): 'saved' | 'oversize' | 'failed' {
  try {
    const payload = JSON.stringify({ text, attachments, updatedAt: Date.now() })
    // Size cap: a runaway draft must not crowd the quota for the dock's
    // persisted keys. Oversized drafts simply stay in-memory — A-21 fix:
    // the skip used to be silent; the caller now warns (console + a
    // one-shot toast) instead of letting a reload eat the text unnoticed.
    if (payload.length > DRAFT_MAX_BYTES) return 'oversize'
    localStorage.setItem(draftKey(sessionId), payload)
    return 'saved'
  } catch { return 'failed' /* quota / private mode — drafts are best-effort */ }
}

function clearDraft(sessionId: string): void {
  try { localStorage.removeItem(draftKey(sessionId)) } catch { /* noop */ }
}

/// 2026-09-29 provider review §3-A1: the banner shows ONLY when there is
/// genuinely something to fix —
///   - no active provider AND no env-detected provider (nothing configured), or
///   - an active provider whose credential store entry is missing
///     (except key-less kinds like Ollama).
/// Configured + keyed users (the old dead condition's main victims) never
/// see it. `null` status (command failed / still loading) hides the banner:
/// we nag only on a positive signal, never on a failed read.
export function shouldShowApiKeyBanner(status: ProviderStatus | null | undefined): boolean {
  if (!status) return false
  if (!status.active_provider_id) return !status.env_provider
  return !status.has_api_key && status.kind !== 'ollama'
}

export default function Chat() {
  const {
    messages, streamingText, isQuerying, usage, activeToolCalls,
    sendMessage, cancelQuery, contextPanelOpen, setContextPanelOpen, compactSession,
    promptQueue, dequeuePrompt, enqueuePrompt, rewindSession, checkpoints, runProcess,
  } = useChat()
  const { sessions, currentSessionId, windowSessionId, createSession } = useSessions()
  const { config, providerStatus } = useCatalog()
  // P1-C: file-mutating tool outputs (md/html/svg/mermaid/images written to
  // disk) dock as provenance-tagged artifact tabs.
  useDiskArtifacts(messages)
  const intl = useIntl()
  const t = useCallback((id: string, values?: Record<string, string | number>) => intl.formatMessage({ id }, values), [intl])
  const navigate = useNavigate()
  const location = useLocation()

  // Composer state (draft + attachments) is provided through the page-local
  // ComposerContext — MessageArea / ComposerPanel consume it directly instead
  // of receiving it (plus handlers) through two layers of props.
  const [input, setInput] = useState('')
  const [attachedFiles, setAttachedFiles] = useState<string[]>([])
  const [diffPath, setDiffPath] = useState<string | null>(null)
  const [diffPaths, setDiffPaths] = useState<string[] | null>(null)
  const [quickFixOpen, setQuickFixOpen] = useState(false)
  const [editorOpen, setEditorOpen] = useState(false)
  // Standalone /editor route retired — external entry points (mod+5, command
  // palette, /editor slash) open the chat-inline panel via this event.
  useEffect(() => {
    const open = () => setEditorOpen(true)
    window.addEventListener('shannon:open-editor', open)
    return () => window.removeEventListener('shannon:open-editor', open)
  }, [])

  // US4 (direction A): the terminal toolbar's "send to agent" hands the
  // selection over as a quoted (fenced) block. Prefill follows the same
  // input-state path as the location.state prefill below — replace the
  // draft — then the established `shannon:focus-composer` event moves
  // focus into the composer so typing continues under the block. The
  // terminal drawer stays open.
  useEffect(() => {
    const prefill = (e: Event) => {
      const text = (e as CustomEvent<{ text?: unknown }>).detail?.text
      if (typeof text !== 'string' || text.length === 0) return
      setInput(text)
      window.dispatchEvent(new Event('shannon:focus-composer'))
    }
    window.addEventListener('shannon:composer-prefill', prefill)
    return () => window.removeEventListener('shannon:composer-prefill', prefill)
  }, [])

  // Pre-fill the composer when navigated from elsewhere (e.g. Editor's
  // "Ask AI about this diagnostic" button passes { prefill } in location.state).
  // A-13 fix: the guard used to be a once-per-mount boolean, so a SECOND
  // prefill navigation while Chat stayed mounted (Sidebar/Editor → /chat is
  // a same-route navigation when the user is already on /chat) was silently
  // ignored. Each navigation carries a unique location.key — a prefill now
  // applies once per NAVIGATION, and the replace below clears the state so
  // the same prefill can never re-apply on re-render.
  const lastPrefillKeyRef = useRef<string | null>(null)
  // Set whenever a prefill claims the composer; the mount-time draft restore
  // below must not clobber a prefill applied in the same mount pass (the
  // A-6 follow-up ordering: prefill wins over the boot draft restore).
  const prefillClaimedRef = useRef(false)
  useEffect(() => {
    const prefill = (location.state as { prefill?: string } | null)?.prefill
    if (!prefill || lastPrefillKeyRef.current === location.key) return
    lastPrefillKeyRef.current = location.key
    prefillClaimedRef.current = true
    setInput(prefill)
    navigate(location.pathname, { replace: true, state: null })
  }, [location.state, location.key, location.pathname, navigate])

  // ── B1 §4-11 / P2-1: per-session drafts ────────────────────────────────
  // The draft (text + attachments) used to be one page-level pair of states
  // that cross-contaminated every session and died with the tab. It now
  // persists per session under `shannon.draft.<id>`: debounced write while
  // typing, synchronous flush on switch, cleared when emptied (send).
  const visibleSessionId = windowSessionId ?? currentSessionId
  // A-21 fix: an oversized draft silently never reached localStorage — the
  // user found out only when a reload ate the text. Every skipped write now
  // console.warns and a one-shot toast (once per mount — the debounced
  // writer would otherwise nag on every keystroke past the cap) tells the
  // user the draft is window-bound. The input itself is never blocked: the
  // draft keeps living in the composer state.
  const oversizeDraftToastedRef = useRef(false)
  const persistDraft = useCallback((sessionId: string, text: string, attachments: string[]) => {
    if (writeDraft(sessionId, text, attachments) !== 'oversize') return
    console.warn(`[Chat] draft for session ${sessionId} exceeds the ${Math.round(DRAFT_MAX_BYTES / 1024)}KB persistence cap — kept in memory only, lost on reload`)
    if (oversizeDraftToastedRef.current) return
    oversizeDraftToastedRef.current = true
    toast.warning(t('chat.draft.oversize'))
  }, [t])
  useEffect(() => {
    if (!visibleSessionId) return
    const id = window.setTimeout(() => {
      if (!input.trim() && attachedFiles.length === 0) clearDraft(visibleSessionId)
      else persistDraft(visibleSessionId, input, attachedFiles)
    }, DRAFT_DEBOUNCE_MS)
    return () => window.clearTimeout(id)
  }, [input, attachedFiles, visibleSessionId, persistDraft])

  // ── B1 §4-8: message edit (composer-based) ─────────────────────────────
  // One message editable at a time; the composer is prefilled and a banner
  // identifies the target. Sending rewinds to before that turn and resends
  // the edited text with the composer's CURRENT attachments (A-26 WYSIWYG:
  // the message's attachments load into the composer on entry, so chips the
  // user sees — added or removed — are exactly what resends). Escape/cancel
  // restores the pre-edit draft.
  const [editing, setEditing] = useState<EditingMessageState | null>(null)

  // Restore the incoming session's draft on switch (replacing whatever the
  // previous session left in the composer) and cancel any in-flight message
  // edit — editing is scoped to the session it started in. The prev-ref
  // guard keeps a same-session remount from resetting the composer.
  //
  // A-6 fix follow-up: the MAIN WINDOW now cold-starts BOUND to the active
  // session (AppContext.loadInitialData), so Chat's first render can already
  // sit in the session whose draft is persisted — the old change-only
  // trigger never fired for it (clicking the same session is a no-op), and
  // the §4-11 restart anchor died. The effect's FIRST run therefore restores
  // the arrival session's draft too (mount = arriving in that session; the
  // composer state is brand-new, so there is nothing to flush over it) —
  // unless a location.state prefill claimed the composer in the same pass.
  const prevDraftSessionRef = useRef<string | null | undefined>(undefined)
  useEffect(() => {
    const firstRun = prevDraftSessionRef.current === undefined
    const previousId = prevDraftSessionRef.current
    prevDraftSessionRef.current = visibleSessionId
    if (firstRun) {
      // Mount: restore whatever the arrival session kept (no-op when the
      // boot is unbound — null has no draft). Fix-round 1 (A-6 follow-up):
      // a location.state prefill that applied in this SAME mount pass (the
      // effect above runs first and flips its ref synchronously) owns the
      // composer — the restore must not clobber it with a stale draft; the
      // debounced write below then persists the prefill as the new draft.
      if (!prefillClaimedRef.current) {
        const draft = visibleSessionId ? readDraft(visibleSessionId) : null
        setInput(draft?.text ?? '')
        setAttachedFiles(draft?.attachments ?? [])
      }
      return
    }
    if (previousId === visibleSessionId) return
    // Flush synchronously so the old session's last keystrokes survive.
    // While an edit is in flight the composer holds the MESSAGE text, not
    // the user's draft — flush the pre-edit draft instead, or the switch
    // would permanently overwrite the draft with the edit prefill.
    if (previousId) {
      const prev = editing ? editing.draft : { text: input, attachments: attachedFiles }
      persistDraft(previousId, prev.text, prev.attachments)
    }
    const draft = visibleSessionId ? readDraft(visibleSessionId) : null
    setInput(draft?.text ?? '')
    setAttachedFiles(draft?.attachments ?? [])
    setEditing(null)
    drainBlockedRef.current = false
    // R2 W2-4: the blocked payload is this session's held continue target —
    // it never follows the user into another session.
    setBlockedPayload(null)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visibleSessionId])

  const startEdit = useCallback((index: number) => {
    if (isQuerying || editing) return
    const msg = messages[index]
    if (!msg || msg.role !== 'user') return
    // Same turn math as MessageArea's rewind affordance: the rewind target
    // is the number of user messages before this one; no checkpoint at or
    // after that turn means nothing to rewind onto (fresh/demo session).
    let turnIndex = 0
    for (let i = 0; i < index; i++) {
      if (messages[i]?.role === 'user') turnIndex++
    }
    const rewindable = checkpoints.some(c => c.turn_index >= turnIndex)
    if (!rewindable) return
    setEditing({
      index,
      turnIndex,
      content: msg.content,
      timestamp: msg.timestamp,
      attachmentPaths: (msg.file_attachments ?? []).map(a => a.path),
      draft: { text: input, attachments: attachedFiles },
    })
    setInput(msg.content)
    // A-26 WYSIWYG: the message's own attachments become the composer's
    // chips (addable/removable). What the user sees during the edit is
    // exactly the set that resends on commit; cancelEdit restores the
    // pre-edit draft pair below.
    setAttachedFiles((msg.file_attachments ?? []).map(a => a.path))
  }, [isQuerying, editing, messages, checkpoints, input, attachedFiles])

  const cancelEdit = useCallback(() => {
    if (!editing) return
    setInput(editing.draft.text)
    setAttachedFiles(editing.draft.attachments)
    setEditing(null)
  }, [editing])

  // ── B1 §4-12: in-conversation search ───────────────────────────────────
  const [searchOpen, setSearchOpen] = useState(false)
  const [searchFlashIndex, setSearchFlashIndex] = useState<number | null>(null)
  const searchFlashTimerRef = useRef<number | null>(null)
  const flashMessage = useCallback((index: number | null) => {
    setSearchFlashIndex(index)
    if (searchFlashTimerRef.current != null) window.clearTimeout(searchFlashTimerRef.current)
    if (index != null) {
      searchFlashTimerRef.current = window.setTimeout(() => setSearchFlashIndex(null), SEARCH_FLASH_MS)
    }
  }, [])
  useEffect(() => () => {
    if (searchFlashTimerRef.current != null) window.clearTimeout(searchFlashTimerRef.current)
  }, [])
  // Ctrl/Cmd+F owns the shortcut everywhere on the chat page — the desktop
  // webview has no native find dialog to fall back to. preventDefault even
  // inside editable targets so the composer can't shadow it.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return
      if (e.key !== 'f' && e.key !== 'F') return
      e.preventDefault()
      setSearchOpen(true)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])
  const closeSearch = useCallback(() => setSearchOpen(false), [])


  // Cmd/Ctrl+\ toggles the right dock — same one in AppContext the Header
  // button drives. Mirrors the terminal's Ctrl+` and keeps the keyboard
  // layer self-discoverable from the shortcuts help panel.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.shiftKey || e.altKey) return
      if (e.key !== '\\' && e.key !== '|') return
      const el = e.target as HTMLElement | null
      if (el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable)) return
      e.preventDefault()
      setContextPanelOpen(!contextPanelOpen)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [contextPanelOpen, setContextPanelOpen])

  const [bannerDismissed, setBannerDismissed] = useState(false)

  // P0-4: session-budget advisory/choice bars. "Continue once" exempts
  // exactly one send's pre-turn budget check backend-side (budgetBypass).
  const budgetGuard = useBudgetGuard(currentSessionId)

  // R2 W2-4: the payload the pre-turn guard last refused, held until it is
  // actually delivered. A blocked send hands its full payload (text +
  // attachments) back to the composer AND parks it here, so "Continue once"
  // re-sends the BLOCKED message — never a reverse-found earlier turn (the
  // old behavior replayed an older question once the optimistic rollback
  // had erased the blocked one), and a first-turn block — where no earlier
  // user message exists at all — still has a working action.
  const [blockedPayload, setBlockedPayload] = useState<{ text: string; attachments: string[] } | null>(null)

  const continuePastBudget = useCallback(() => {
    if (blockedPayload) {
      const { text, attachments } = blockedPayload
      setBlockedPayload(null)
      void sendMessage(text, attachments.length > 0 ? attachments : undefined, { budgetBypass: true })
        .then(ok => {
          if (!ok) {
            // Still refused (a different pre-turn guard) — hand the payload
            // back to the composer and the banner, same as a blocked send.
            setInput(text)
            setAttachedFiles(attachments)
            setBlockedPayload({ text, attachments })
          }
        })
      return
    }
    // Degraded fallback (mid-turn cap hit with nothing refused in this UI
    // session): explicitly re-send the last RECORDED user turn — the banner
    // labels this action "resend the last message". Its attachments travel
    // along. An earlier turn is never replayed silently.
    const lastUser = [...messages].reverse().find(m => m.role === 'user')
    if (lastUser) {
      const attachments = (lastUser.file_attachments ?? []).map(a => a.path)
      void sendMessage(lastUser.content, attachments.length > 0 ? attachments : undefined, { budgetBypass: true })
    }
  }, [blockedPayload, messages, sendMessage])

  // W2-4: what the banner's Continue will actually deliver. `null` = nothing
  // to deliver (no refused payload held, no recorded user turn) — the action
  // hides instead of staying a clickable no-op.
  const continueTarget: 'blocked' | 'last-message' | null = blockedPayload
    ? 'blocked'
    : messages.some(m => m.role === 'user') ? 'last-message' : null

  const messagesEndRef = useRef<HTMLDivElement>(null)
  const scrollParentRef = useRef<HTMLDivElement>(null)

  const virtualizer = useVirtualizer({
    count: messages.length,
    getScrollElement: () => scrollParentRef.current,
    estimateSize: () => 200,
    overscan: 4,
    measureElement: typeof window !== 'undefined' && 'ResizeObserver' in window
      ? (el) => el.getBoundingClientRect().height
      : undefined,
  })

  // B0 P1-1: stream auto-follow is now conditional. A passive scroll
  // listener tracks whether the user is at/near the bottom; streaming
  // updates only pull the viewport down while they are. Scrolling back to
  // the bottom re-arms following; MessageArea's "scroll to latest" FAB is
  // the explicit way back. The listener also sees our own programmatic
  // scrolls — they always end at distance 0, so following re-arms itself.
  const stickToBottomRef = useRef(true)
  useEffect(() => {
    const el = scrollParentRef.current
    if (!el) return
    const NEAR_BOTTOM_PX = 80
    const onScroll = () => {
      stickToBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight <= NEAR_BOTTOM_PX
    }
    el.addEventListener('scroll', onScroll, { passive: true })
    return () => el.removeEventListener('scroll', onScroll)
  }, [])

  useEffect(() => {
    // Review 2026-09-16: scrollIntoView raced the virtualizer (the end sentinel
    // is measured before it mounts), leaving the new user bubble mid-viewport.
    // Scroll the virtualized parent directly instead.
    // B0 P1-1: only while the user hasn't scrolled away to read.
    if (!stickToBottomRef.current) return
    const el = scrollParentRef.current
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' })
    else messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages, streamingText])

  const [slashResult, setSlashResult] = useState<SlashResult | null>(null)
  const dismissSlashResult = useCallback(() => setSlashResult(null), [])

  // B0 P2-2: a slash result card (/cost, /context …) is diagnostics about
  // the session it ran in — don't let it follow the user into the next one.
  useEffect(() => {
    setSlashResult(null)
  }, [currentSessionId])

  // B3 §P1-14 (decision §5-6): chat-fence artifact tabs belong to the
  // session that produced them — clear them on session switch. Disk
  // artifacts and web tabs survive. The prev-ref guard keeps a Chat
  // remount on the same session from wiping the dock.
  const { closeChatArtifacts } = useArtifact()
  const prevArtifactSessionRef = useRef(currentSessionId)
  useEffect(() => {
    if (prevArtifactSessionRef.current === currentSessionId) return
    prevArtifactSessionRef.current = currentSessionId
    closeChatArtifacts()
    // B4 P2-20: diff "+x −y" counts are per-path snapshots of THIS session's
    // review state — drop them so the next session re-fetches fresh stats.
    clearDiffStatsCache()
  }, [currentSessionId, closeChatArtifacts])

  const executeSlash = useCallback((cmd: SlashCommand) => {
    void cmd.run({
      navigate,
      sessionId: currentSessionId,
      workingDir: sessions.find(s => s.id === currentSessionId)?.working_dir
        ?? config?.working_dir
        ?? '',
      sessions,
      createSession,
      compactSession,
      showResult: setSlashResult,
      toastError,
      t,
    })
  }, [navigate, currentSessionId, sessions, config?.working_dir, createSession, compactSession, t])

  // B1 §4-8: commit an edit — rewind to before the edited turn, then resend
  // the edited text with the composer's CURRENT attachments (A-26 WYSIWYG:
  // chips added/removed during the edit are honored; a set left untouched
  // resends the message's original paths, and an attachment-less message
  // still commits with zero attachments). A failed rewind keeps editing
  // mode alive so the user can retry or cancel.
  const commitEdit = useCallback(async (newText: string) => {
    if (!editing) return
    try {
      await rewindSession(editing.turnIndex)
    } catch (e) {
      toastError(t('chat.edit.failed'), e)
      return
    }
    setEditing(null)
    const attachments = attachedFiles
    const ok = await sendMessage(newText, attachments.length > 0 ? attachments : undefined)
    if (!ok) {
      // The backend rejected the send AFTER the rewind landed (budget
      // guard, concurrent-query guard, …) — the old turn is already gone,
      // so keep the edited text and the current chips in the composer
      // instead of discarding them (they were never cleared on this path);
      // the debounced draft write persists the recovery.
      setInput(newText)
      return
    }
    setInput('')
    setAttachedFiles([])
    if (visibleSessionId) clearDraft(visibleSessionId)
  }, [editing, attachedFiles, rewindSession, sendMessage, visibleSessionId, t])

  const handleSend = () => {
    const trimmed = input.trim()
    const hasAttachments = attachedFiles.length > 0
    // Any manual send re-arms the queue drain after a rejected-send breaker.
    drainBlockedRef.current = false
    // B0 P0-1: attachments-only sends are real. The backend accepts an empty
    // text alongside attachment paths (send_message never validated
    // emptiness; the engine turns the attachments into content blocks), so
    // an empty text with files now goes through instead of silently
    // no-oping behind an enabled send button.
    if (!trimmed && !hasAttachments) return
    if (trimmed) {
      // Slash commands never reach the model: a bare `/name` runs locally
      // (the desktop's counterpart of the REPL command line), and anything
      // else starting with `/` — typically a pasted absolute path — is sent
      // as plain text. They execute locally regardless of query state —
      // there is nothing to stream, so nothing to queue.
      const slashCommand = parseSlashInput(trimmed)
      if (slashCommand) {
        executeSlash(slashCommand)
        setInput('')
        return
      }
    }
    // B1 §4-8: an in-flight edit replaces the turn (rewind + resend) instead
    // of appending. Blocked while querying — rewinding mid-run is unsafe.
    if (editing) {
      if (isQuerying) return
      // A-25 fix: aligned with the main path's A-9 semantics — clearing the
      // text while chips remain commits an attachments-only edit (empty text
      // + the current composer attachments). Only a double-empty composer is
      // a no-op; the old `!trimmed` arm swallowed that Enter silently.
      if (!trimmed && !hasAttachments) return
      void commitEdit(trimmed)
      return
    }
    const filePaths = hasAttachments ? attachedFiles : undefined
    // B1 §4-9: while THIS session streams, sends join its FIFO queue instead
    // of being dropped. Accepted items clear the draft (they render as
    // removable chips); an overflow keeps it. Attachments-only sends join
    // too (R2 W2-4): the queue already renders an attachments-only chip, so
    // the old text-only gate was a silent no-op, not a policy.
    if (isQuerying) {
      const accepted = enqueuePrompt(trimmed, hasAttachments ? attachedFiles : [])
      if (accepted) {
        // A-22: an accepted queue join is "sent" in the user's mental model
        // ("what did I just send") — record it for ArrowUp recall now. The
        // drain replay must not re-record it (same single send).
        if (trimmed) recordInputHistory(trimmed)
        setInput('')
        setAttachedFiles([])
      }
      return
    }
    // A-1 fix on the R2 W2-4 accepted-flow: the clear happens SYNCHRONOUSLY
    // (a deferred clear would let a double-Enter race a second identical send
    // through the still-empty gate), but the send's result still decides the
    // draft's fate. Accepted → the draft key is cleared. Rejected (budget /
    // concurrent-query / goal guards — sendMessage resolves false) → the
    // whole draft — text AND attachment chips — comes back to the composer
    // AND parks as the banner's continue target, instead of the message
    // vanishing; the debounced draft write re-persists the key.
    setInput('')
    setAttachedFiles([])
    void sendMessage(trimmed, filePaths).then(ok => {
      if (ok) {
        // A-22: the send was accepted — global input history for ArrowUp
        // recall. Attachments-only sends record nothing (empty text).
        if (trimmed) recordInputHistory(trimmed)
        setBlockedPayload(null)
        if (visibleSessionId) clearDraft(visibleSessionId)
      } else {
        setInput(trimmed)
        setAttachedFiles(filePaths ?? [])
        setBlockedPayload({ text: trimmed, attachments: filePaths ?? [] })
      }
    })
  }

  // GB P2-10a — the "interrupt now" send (composer bolt button /
  // Ctrl+Enter while streaming). Cancel the running turn, then deliver this
  // message the moment the cancel settles — ahead of any FIFO-queued
  // prompts. Slash commands never interrupt (nothing streams for them):
  // they run locally right away, exactly like Enter does.
  const handleSteer = () => {
    const trimmed = input.trim()
    const hasAttachments = attachedFiles.length > 0
    if (!trimmed && !hasAttachments) return
    // A steer is a manual send — re-arms the drain breaker like handleSend.
    drainBlockedRef.current = false
    const slashCommand = trimmed ? parseSlashInput(trimmed) : null
    if (slashCommand) {
      executeSlash(slashCommand)
      setInput('')
      return
    }
    const accepted = steer(trimmed, hasAttachments ? attachedFiles : [])
    if (accepted) {
      setInput('')
      setAttachedFiles([])
      if (visibleSessionId) clearDraft(visibleSessionId)
    }
  }

  // GB P2-10a — two-tier steering. Declared ABOVE the queue-drain effect so
  // an interrupted steer flushes first on the same isQuerying→false commit;
  // the drain below also gates on hasPendingSteer so both never send in one
  // commit (the drain would otherwise burn a queued item against the
  // backend's concurrent-query guard). Round-1 review: the pending steer is
  // parked under its session key (a settle observed on another session
  // never receives it) and a cancel that never settles hands the draft back
  // after 15s with a notice instead of waiting forever.
  const { steer, hasPendingSteer } = useSteerSend({
    visibleSessionId,
    isQuerying,
    cancelQuery,
    sendMessage,
    onSendRejected: (pending, reason) => {
      setInput(pending.text)
      setAttachedFiles(pending.attachments)
      if (reason === 'timeout') toast.error(t('chat.steer.timeout'))
    },
  })

  // B1 §4-9: drain — when this session's run settles and prompts are still
  // queued, auto-send the head. Queued slash commands (if any land here)
  // execute locally like normal; the dequeue ref in AppContext keeps this
  // single-shot even under StrictMode double-invocation. A REJECTED send
  // trips the drain breaker: without it every queued item would be dequeued
  // and burned one after another against the same failing backend. A manual
  // send (or a session switch) resets the breaker.
  const drainBlockedRef = useRef(false)
  useEffect(() => {
    if (isQuerying || promptQueue.length === 0) return
    // GB P2-10a: an interrupted steer owns this settle — it sends first and
    // flips isQuerying back on; the queue drains when THAT run finishes.
    if (hasPendingSteer()) return
    if (drainBlockedRef.current) return
    const item = dequeuePrompt()
    if (!item) return
    const cmd = item.text.trim() ? parseSlashInput(item.text.trim()) : null
    if (cmd) {
      executeSlash(cmd)
      return
    }
    void sendMessage(item.text, item.attachments.length > 0 ? item.attachments : undefined)
      .then(ok => {
        if (!ok) {
          drainBlockedRef.current = true
          // R2 W2-4: the dequeued head was refused before recording — return
          // it to the composer and hold it for the banner's continue instead
          // of silently dropping user content.
          setInput(item.text)
          setAttachedFiles(item.attachments)
          setBlockedPayload({ text: item.text, attachments: item.attachments })
        }
      })
    // `promptQueue` re-triggers the drain for the next item once the new run
    // settles; sendMessage flips isQuerying synchronously during the send.
  }, [isQuerying, promptQueue, dequeuePrompt, executeSlash, sendMessage, hasPendingSteer])

  // Attach files via Tauri's native dialog so the backend receives real
  // absolute paths (the backend reads bytes via std::fs and base64-encodes).
  // The browser <input type="file"> only exposes File objects with opaque
  // "fakepath" paths, which never resolve on disk — that was the dead-button bug.
  //
  // The onAttach contract carries the FULL intended set, so this is a
  // REPLACE, not an append: ChatInput's mergePaths dedupes
  // `[current ∪ new]` before calling (dialog / drag-drop / paste), and a
  // chip's ✕ hands back the remaining list as its removal (A-26 follow-up —
  // the old `[...prev, ...files]` append duplicated every previously
  // attached file on the second attach and resurrected a just-removed chip).
  const handleAttach = (files: string[]) => {
    setAttachedFiles(files)
  }

  const handleDetachAll = () => {
    setAttachedFiles([])
  }

  const composerValue = {
    input, setInput, handleSend, handleSteer,
    attachedFiles, handleAttach, handleDetachAll,
    executeSlash, slashResult, dismissSlashResult,
    editing, cancelEdit,
  }

  // 2026-09-29 provider review §3-A1 (item 2): the old gate read
  // `config.api_key`/`config.provider` — fields DesktopConfig dropped in
  // ADR-0005 — so the banner showed for EVERY user in production. Gate on
  // the reliable `get_provider_status` snapshot instead: only genuinely
  // unconfigured or keyless users see it.
  const showApiKeyBanner = !bannerDismissed && shouldShowApiKeyBanner(providerStatus)

  // ── Layout (2026-09 review) ────────────────────────────────────────────
  //
  // The chat page is one full-width conversation plus the RightDock
  // (documents / diffs / live preview / context) and the terminal drawer
  // with its own toggle (Ctrl+`).

  const workingDir = sessions.find(s => s.id === currentSessionId)?.working_dir
    ?? config?.working_dir
    ?? null

  // P0-B: file chips resolve relative paths against this session's working
  // dir — publish it to the module ref the chip reads (one sync per change).
  useEffect(() => {
    setActiveWorkingDir(workingDir)
  }, [workingDir])

  // P0-B: a code-file chip (non-artifact extension) opens the inline editor
  // pre-loaded with the file. Artifact extensions are handled by the
  // ArtifactLinkHost instead.
  const [editorInitialPath, setEditorInitialPath] = useState<string | null>(null)
  useEffect(() => {
    const open = (e: Event) => {
      const path = (e as CustomEvent<{ path?: string }>).detail?.path
      if (!path) return
      setEditorInitialPath(path)
      setEditorOpen(true)
    }
    window.addEventListener('shannon:open-code-file', open)
    return () => window.removeEventListener('shannon:open-code-file', open)
  }, [])

  // B0 P0-5: the editor keeps unsaved edits in component state only — the
  // modal's close paths (Esc / close button / backdrop) must confirm before
  // discarding them. Editor reports dirtiness through the ref (no re-render
  // churn); the path reset stops a later open from pre-loading a stale file.
  const editorDirtyRef = useRef(false)
  const [confirmDiscardEditor, setConfirmDiscardEditor] = useState(false)
  const closeEditor = useCallback(() => {
    setEditorOpen(false)
    setConfirmDiscardEditor(false)
    editorDirtyRef.current = false
    setEditorInitialPath(null)
  }, [])

  return (
    <ComposerContext.Provider value={composerValue}>
        <div className="flex-1 flex w-full h-full relative">
          {/* Main Chat Canvas — the session list lives in the app sidebar (U1)
              and the session title + RightDock toggle live in the global
              Header (U2); the ChatHeader bar is retired. */}
          <section className="flex-1 flex flex-col relative bg-surface-container-lowest/40 overflow-hidden">
            <ApiKeyBanner
              visible={showApiKeyBanner}
              variant={providerStatus?.active_provider_id ? 'no-key' : 'no-provider'}
              providerName={providerStatus?.display_name ?? providerStatus?.active_provider_id ?? undefined}
              onDismiss={() => setBannerDismissed(true)}
              onOpenSettings={() => navigate('/settings/models')}
            />

            <BudgetBanner
              warning={budgetGuard.warning}
              exceeded={budgetGuard.exceeded}
              clearWarning={budgetGuard.clearWarning}
              clearExceeded={budgetGuard.clearExceeded}
              onContinueOnce={continuePastBudget}
              continueTarget={continueTarget}
              sessionId={currentSessionId}
            />

            {searchOpen && (
              <ChatSearchBar
                messages={messages}
                virtualized={messages.length > VIRTUALIZE_THRESHOLD}
                virtualizer={virtualizer}
                scrollParentRef={scrollParentRef}
                onFlash={flashMessage}
                onClose={closeSearch}
              />
            )}

            <MessageArea
              scrollParentRef={scrollParentRef}
              messagesEndRef={messagesEndRef}
              virtualizer={virtualizer}
              setDiffPath={setDiffPath}
              setDiffPaths={setDiffPaths}
              searchFlashIndex={searchFlashIndex}
              onEditMessage={startEdit}
            />
            <ComposerPanel
              setQuickFixOpen={setQuickFixOpen}
              setEditorOpen={setEditorOpen}
            />
            <TerminalPanel projectDir={workingDir} />
          </section>

          <InlinePanelModal
            open={quickFixOpen}
            onClose={() => setQuickFixOpen(false)}
            title={t('nav.quickFix')}
            panel={QuickFixPanel}
            size="2xl"
            modalClassName="max-w-narrow max-h-[85vh] overflow-y-auto"
            bodyClassName="p-lg"
          />

          <InlinePanelModal
            open={editorOpen}
            onClose={() => {
              // B0 P0-5: dirty draft → confirm before discarding.
              if (editorDirtyRef.current) {
                setConfirmDiscardEditor(true)
                return
              }
              closeEditor()
            }}
            title={t('nav.editor')}
            panel={EditorPanel}
            panelProps={{ initialPath: editorInitialPath, onDirtyChange: (dirty: boolean) => { editorDirtyRef.current = dirty } }}
            size="2xl"
            modalClassName="max-w-5xl h-[90vh] flex flex-col"
            bodyClassName="flex-1 overflow-hidden"
          />

          <RightDock
            open={contextPanelOpen}
            onOpen={() => setContextPanelOpen(true)}
            onClose={() => setContextPanelOpen(false)}
            usage={usage}
            activeToolCalls={activeToolCalls}
            workingDir={workingDir}
            planModeActive={config?.approval_mode === 'plan'}
            diffPath={diffPath}
            onCloseDiff={() => setDiffPath(null)}
            runProcess={runProcess}
          />
          <DiffDialogMulti open={diffPaths !== null} filePaths={diffPaths ?? []} onClose={() => setDiffPaths(null)} />

          {/* B0 P0-5: discard confirmation for unsaved editor edits. */}
          <ConfirmDialog
            open={confirmDiscardEditor}
            title={t('editor.discard.title')}
            message={t('editor.discard.message')}
            confirmLabel={t('editor.discard.confirm')}
            cancelLabel={t('editor.discard.cancel')}
            destructive
            onConfirm={closeEditor}
            onCancel={() => setConfirmDiscardEditor(false)}
          />
        </div>
    </ComposerContext.Provider>
  )
}
