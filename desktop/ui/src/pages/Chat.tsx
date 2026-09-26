import { useState, useRef, useEffect, useCallback, lazy } from 'react'
import { useNavigate, useLocation } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { useVirtualizer } from '@tanstack/react-virtual'
import DiffDialogMulti from '@/components/diff/DiffDialogMulti'
import { useChat } from '@/context/ChatContext'
import { useCatalog } from '@/context/CatalogContext'
import { useSessions } from '@/context/SessionContext'
import { parseSlashInput, type SlashCommand, type SlashResult } from '@/lib/slash/commands'
import { clearDiffStatsCache } from '@/components/chat/diffStats'
import { toastError } from '@/lib/errorToast'
import { setActiveWorkingDir } from '@/lib/fileRefs'
import { useDiskArtifacts } from '@/hooks/useDiskArtifacts'
import { useArtifact } from '@/components/artifact/ArtifactContext'
import { useBudgetGuard } from '@/hooks/useBudgetGuard'
import BudgetBanner from '@/components/chat/BudgetBanner'
import { TerminalPanel } from '@/components/terminal/TerminalPanel'
import RightDock from './chat/RightDock'
import {
  ApiKeyBanner,
  ChatSearchBar,
  ComposerPanel,
  InlinePanelModal,
  MessageArea,
  ComposerContext,
} from './chat'
import { VIRTUALIZE_THRESHOLD } from './chat/MessageArea'
import type { EditingMessageState } from './chat/ComposerContext'

// QuickFix is a chat-inline tool launched from the composer toolbar (it has
// no standalone route); the Editor exists both inline and as a standalone
// route (palette / mod+5). Lazy-loaded so the main chat bundle stays small.
const QuickFixPanel = lazy(() => import('@/pages/QuickFix'))
const EditorPanel = lazy(() => import('@/pages/Editor'))

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

function writeDraft(sessionId: string, text: string, attachments: string[]): void {
  try {
    const payload = JSON.stringify({ text, attachments, updatedAt: Date.now() })
    // Size cap: a runaway draft must not crowd the quota for the dock's
    // persisted keys. Oversized drafts simply stay in-memory.
    if (payload.length > DRAFT_MAX_BYTES) return
    localStorage.setItem(draftKey(sessionId), payload)
  } catch { /* quota / private mode — drafts are best-effort */ }
}

function clearDraft(sessionId: string): void {
  try { localStorage.removeItem(draftKey(sessionId)) } catch { /* noop */ }
}

export default function Chat() {
  const {
    messages, streamingText, isQuerying, usage, activeToolCalls,
    sendMessage, contextPanelOpen, setContextPanelOpen, compactSession,
    promptQueue, dequeuePrompt, enqueuePrompt, rewindSession, checkpoints,
  } = useChat()
  const { sessions, currentSessionId, windowSessionId, createSession } = useSessions()
  const { config } = useCatalog()
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

  // Pre-fill the composer when navigated from elsewhere (e.g. Editor's
  // "Ask AI about this diagnostic" button passes { prefill } in location.state).
  // Guard with a ref so the effect doesn't re-fire on every keystroke that
  // updates `input` — only react to the navigation event itself.
  const prefillApplied = useRef(false)
  useEffect(() => {
    if (prefillApplied.current) return
    const prefill = (location.state as { prefill?: string } | null)?.prefill
    if (prefill) {
      setInput(prefill)
      prefillApplied.current = true
      navigate(location.pathname, { replace: true, state: null })
    }
  }, [location.state, location.pathname, navigate])

  // ── B1 §4-11 / P2-1: per-session drafts ────────────────────────────────
  // The draft (text + attachments) used to be one page-level pair of states
  // that cross-contaminated every session and died with the tab. It now
  // persists per session under `shannon.draft.<id>`: debounced write while
  // typing, synchronous flush on switch, cleared when emptied (send).
  const visibleSessionId = windowSessionId ?? currentSessionId
  useEffect(() => {
    if (!visibleSessionId) return
    const id = window.setTimeout(() => {
      if (!input.trim() && attachedFiles.length === 0) clearDraft(visibleSessionId)
      else writeDraft(visibleSessionId, input, attachedFiles)
    }, DRAFT_DEBOUNCE_MS)
    return () => window.clearTimeout(id)
  }, [input, attachedFiles, visibleSessionId])

  // ── B1 §4-8: message edit (composer-based) ─────────────────────────────
  // One message editable at a time; the composer is prefilled and a banner
  // identifies the target. Sending rewinds to before that turn and resends
  // the edited text with the ORIGINAL attachments (attachment editing is
  // out of scope). Escape/cancel restores the pre-edit draft.
  const [editing, setEditing] = useState<EditingMessageState | null>(null)

  // Restore the incoming session's draft on switch (replacing whatever the
  // previous session left in the composer) and cancel any in-flight message
  // edit — editing is scoped to the session it started in. The prev-ref
  // guard keeps a same-session remount from resetting the composer.
  const prevDraftSessionRef = useRef(visibleSessionId)
  useEffect(() => {
    if (prevDraftSessionRef.current === visibleSessionId) return
    const previousId = prevDraftSessionRef.current
    prevDraftSessionRef.current = visibleSessionId
    // Flush synchronously so the old session's last keystrokes survive.
    // While an edit is in flight the composer holds the MESSAGE text, not
    // the user's draft — flush the pre-edit draft instead, or the switch
    // would permanently overwrite the draft with the edit prefill.
    if (previousId) {
      const prev = editing ? editing.draft : { text: input, attachments: attachedFiles }
      writeDraft(previousId, prev.text, prev.attachments)
    }
    const draft = visibleSessionId ? readDraft(visibleSessionId) : null
    setInput(draft?.text ?? '')
    setAttachedFiles(draft?.attachments ?? [])
    setEditing(null)
    drainBlockedRef.current = false
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

  // P0-4: session-budget advisory/choice bars. "Continue once" resends the
  // last user message with the budget-bypass flag (exempts exactly that
  // send's pre-turn check backend-side).
  const budgetGuard = useBudgetGuard(currentSessionId)
  const continuePastBudget = useCallback(() => {
    const lastUser = [...messages].reverse().find(m => m.role === 'user')
    if (lastUser) void sendMessage(lastUser.content, undefined, { budgetBypass: true })
  }, [messages, sendMessage])
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
  // the edited text with the message's ORIGINAL attachment paths (editing
  // attachments themselves is out of scope). A failed rewind keeps editing
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
    const ok = await sendMessage(newText, editing.attachmentPaths.length > 0 ? editing.attachmentPaths : undefined)
    if (!ok) {
      // The backend rejected the send AFTER the rewind landed (budget
      // guard, concurrent-query guard, …) — the old turn is already gone,
      // so keep the edited text (and its attachments) in the composer
      // instead of discarding them; the debounced draft write persists it.
      setInput(newText)
      setAttachedFiles(editing.attachmentPaths)
      return
    }
    setInput('')
    setAttachedFiles([])
    if (visibleSessionId) clearDraft(visibleSessionId)
  }, [editing, rewindSession, sendMessage, visibleSessionId, t])

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
      if (isQuerying || !trimmed) return
      void commitEdit(trimmed)
      return
    }
    const filePaths = hasAttachments ? attachedFiles : undefined
    // B1 §4-9: while THIS session streams, sends join its FIFO queue instead
    // of being dropped. Accepted items clear the draft (they render as
    // removable chips); an overflow keeps it. The pre-existing edge stands:
    // attachments-only input still no-ops while querying.
    if (isQuerying) {
      if (!trimmed) return
      const accepted = enqueuePrompt(trimmed, hasAttachments ? attachedFiles : [])
      if (accepted) {
        setInput('')
        setAttachedFiles([])
      }
      return
    }
    sendMessage(trimmed, filePaths)
    setInput('')
    setAttachedFiles([])
    if (visibleSessionId) clearDraft(visibleSessionId)
  }

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
    if (drainBlockedRef.current) return
    const item = dequeuePrompt()
    if (!item) return
    const cmd = item.text.trim() ? parseSlashInput(item.text.trim()) : null
    if (cmd) {
      executeSlash(cmd)
      return
    }
    void sendMessage(item.text, item.attachments.length > 0 ? item.attachments : undefined)
      .then(ok => { if (!ok) drainBlockedRef.current = true })
    // `promptQueue` re-triggers the drain for the next item once the new run
    // settles; sendMessage flips isQuerying synchronously during the send.
  }, [isQuerying, promptQueue, dequeuePrompt, executeSlash, sendMessage])

  // Attach files via Tauri's native dialog so the backend receives real
  // absolute paths (the backend reads bytes via std::fs and base64-encodes).
  // The browser <input type="file"> only exposes File objects with opaque
  // "fakepath" paths, which never resolve on disk — that was the dead-button bug.
  const handleAttach = async (files: string[]) => {
    if (files.length > 0) setAttachedFiles(prev => [...prev, ...files])
  }

  const handleDetachAll = () => {
    setAttachedFiles([])
  }

  const composerValue = {
    input, setInput, handleSend,
    attachedFiles, handleAttach, handleDetachAll,
    executeSlash, slashResult, dismissSlashResult,
    editing, cancelEdit,
  }

  const showApiKeyBanner =
    !bannerDismissed &&
    !!config &&
    !config.api_key &&
    config.provider !== 'ollama'

  // ── Layout (2026-09 review) ────────────────────────────────────────────
  //
  // The chat page is one full-width conversation plus the RightDock
  // (documents / diffs / live preview / context) and the terminal drawer.
  // The former workspace toolbar (对话 / Diff / 预览 preset switcher and its
  // grid panels) was retired: the preset triad read as three competing
  // "views" of the conversation and confused first-run users — everything
  // it offered now has a single home in the RightDock tabs, and the
  // terminal keeps its own drawer toggle (Ctrl+`).

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

  return (
    <ComposerContext.Provider value={composerValue}>
        <div className="flex-1 flex w-full h-full relative">
          {/* Main Chat Canvas — the session list lives in the app sidebar (U1)
              and the session title + RightDock toggle live in the global
              Header (U2); the ChatHeader bar is retired. */}
          <section className="flex-1 flex flex-col relative bg-surface-container-lowest/40 overflow-hidden">
            <ApiKeyBanner
              visible={showApiKeyBanner}
              onDismiss={() => setBannerDismissed(true)}
              onOpenSettings={() => navigate('/settings/models')}
            />

            <BudgetBanner
              warning={budgetGuard.warning}
              exceeded={budgetGuard.exceeded}
              clearWarning={budgetGuard.clearWarning}
              clearExceeded={budgetGuard.clearExceeded}
              onContinueOnce={continuePastBudget}
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
            modalClassName="max-w-3xl max-h-[85vh] overflow-y-auto"
            bodyClassName="p-lg"
          />

          <InlinePanelModal
            open={editorOpen}
            onClose={() => setEditorOpen(false)}
            title={t('nav.editor')}
            panel={EditorPanel}
            panelProps={editorInitialPath ? { initialPath: editorInitialPath } : undefined}
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
          />
          <DiffDialogMulti open={diffPaths !== null} filePaths={diffPaths ?? []} onClose={() => setDiffPaths(null)} />
        </div>
    </ComposerContext.Provider>
  )
}
